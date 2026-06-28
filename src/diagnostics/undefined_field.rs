use std::collections::HashSet;
use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, BinaryExpression, Expression, Identifier, IfBranch, LocalAssign, Operator, RepeatUntilLoop, UnaryExpression, WhileLoop};
use crate::syntax::NodeOrToken;
use crate::syntax::syntax_kind::SyntaxKind;
use crate::syntax::tree::{SyntaxNode, SyntaxTree};
use crate::types::{Expr, ExprId, ScopeIndex, SymbolIdentifier, SymbolIndex, TableIndex, ValueType};
use super::{DiagnosticPass, RelatedInfo, WowDiagnostic};

/// A "closed record" is a plain file-local table whose complete field set is
/// statically known, so reading an unknown field is a typo rather than a
/// possibly-runtime-added field. This deliberately excludes:
///   - `@class` tables (handled by the class path above)
///   - EXT-space tables (stub API namespaces and cross-file globals, where
///     incomplete stubs or untracked cross-file writes would cause false
///     positives)
///   - the addon namespace table (open, populated cross-file)
///   - maps/arrays (`key_type`/`value_type`), metatable-backed tables,
///     callables, enums, and placeholders — all of which can hold fields we
///     can't enumerate
///
/// Provenance (the field-access base must be a pure module-private table — see
/// `collect_pure_record_symbols`) is the other half of the contract, checked at
/// the call site.
fn is_closed_record(analysis: &AnalysisResult, idx: TableIndex) -> bool {
    if idx.is_external() { return false; }
    if Some(idx) == analysis.ir.addon_table_idx() { return false; }
    // Any bracket write with a non-string-literal key (`t[k] = v`, `t[i] = v`)
    // makes the field set open: the key isn't statically known, so fields can be
    // added at runtime. A dynamic write in a top-level scope degrades the type to
    // a bare `table`, but one nested in a branch leaves the constructor table
    // intact — so check the recorded bracket writes directly. String-literal keys
    // (`t["a"] = v`) are equivalent to `t.a = v` and stay closed.
    if let Some(writes) = analysis.ir.bracket_key_fields.get(&idx)
        && writes.iter().any(|(key_id, _)| !matches!(analysis.ir.expr(*key_id), Expr::Literal(ValueType::String(_))))
    {
        return false;
    }
    let t = analysis.table(idx);
    t.class_name.is_none()
        && !t.fields.is_empty()
        && t.key_type.is_none()
        && t.value_type.is_none()
        && t.metatable_index.is_none()
        && t.metatable.is_none()
        && t.call_func.is_none()
        && t.parent_classes.is_empty()
        && t.enum_kind == crate::types::EnumKind::NotEnum
        && !t.placeholder
}

/// Collect symbols that are pure module-private record tables: a local variable
/// declared `local NAME = { ... }` directly from a table constructor, with
/// exactly one definition (never reassigned). The `local`-with-constructor-RHS
/// requirement is the key guard against false positives — it rejects:
///   - the addon namespace and other vararg-bound locals (`local _, ns = ...`),
///     whose synthetic overlay table is constructor-backed but whose real field
///     set is contributed cross-file;
///   - global assignments (`SavedVar = {}`), which are populated at runtime;
///   - parameters and mixed-origin locals (`local t = _G[k]; if not t then t = {} end`),
///     whose record shapes are back-inferred from reads as well as writes and so
///     are incomplete.
///
/// Only a variable that is *only* ever a same-file table literal has a
/// fully-known field set.
///
/// A candidate is additionally *disqualified if it escapes*: if the variable is
/// ever referenced bare (as a whole value rather than as the `base` of a
/// `base.field` / `base:method()` access), some other code can hold it and add
/// fields we can't see. The classic case is a registry table that is returned
/// from a constructor and whose optional callbacks (`reg.OnUsed`) are set by
/// callers — the field is read defensively (`if reg.OnUsed then`) but never
/// assigned in this file. A bare reference is any single-segment identifier
/// expression naming the variable (a return value, call argument, RHS, table
/// element, operand, or dynamic `var[k]` index). Field/method accesses produce
/// multi-segment identifiers, so the legitimate `local private = {}; function
/// private.X()` pattern never escapes.
fn collect_pure_record_symbols(analysis: &AnalysisResult, tree: &SyntaxTree) -> HashSet<SymbolIndex> {
    let mut candidates = HashSet::new();
    let mut escaped = HashSet::new();
    for node in SyntaxNode::new_root(tree).descendants() {
        if node.kind() == SyntaxKind::LocalAssignStatement {
            if let Some(assign) = LocalAssign::cast(node) {
                let rhs: Vec<Expression<'_>> = assign.expression_list()
                    .map(|el| el.expressions())
                    .unwrap_or_default();
                if let Some(name_list) = assign.name_list() {
                    for (i, token) in name_list.name_tokens().iter().enumerate() {
                        if !matches!(rhs.get(i), Some(Expression::TableConstructor(_))) { continue; }
                        let start = u32::from(token.text_range().start());
                        let Some((sym_idx, _, _)) = analysis.find_symbol_at(tree, start) else { continue };
                        if !sym_idx.is_external() && analysis.sym(sym_idx).versions.len() == 1 {
                            candidates.insert(sym_idx);
                        }
                    }
                }
            }
        } else if node.kind().is_identifier()
            // Skip identifiers nested inside a larger access chain: parser2 splits
            // `a.b` into a `DotAccess` wrapping a `NameRef(a)`, and that inner
            // `NameRef` is the *base* of an access, not a bare reference. Only a
            // top-level single-segment identifier is a true bare use of the value.
            && node.parent().is_none_or(|p| !p.kind().is_identifier())
            && let Some(ident) = Identifier::cast(node)
            && ident.names().len() == 1
        {
            // Bare single-segment reference: the variable used as a whole value.
            let tokens = AnalysisResult::collect_name_tokens_recursive(node);
            if let Some(first) = tokens.first() {
                let start = u32::from(first.text_range().start());
                if let Some((sym_idx, _, _)) = analysis.find_symbol_at(tree, start) {
                    escaped.insert(sym_idx);
                }
            }
        }
    }
    candidates.retain(|s| !escaped.contains(s));
    candidates
}

/// If the field-access base `table_expr` is a direct reference to a pure
/// module-private record symbol, return its constructor table index and variable name.
fn closed_record_base(
    analysis: &AnalysisResult,
    table_expr: ExprId,
    pure_records: &HashSet<SymbolIndex>,
) -> Option<(TableIndex, String)> {
    let mut e = table_expr;
    while let Expr::Grouped(inner) = analysis.ir.expr(e) { e = *inner; }
    let Expr::SymbolRef(sym_idx, _) = *analysis.ir.expr(e) else { return None };
    if !pure_records.contains(&sym_idx) { return None; }
    let sym = analysis.sym(sym_idx);
    let SymbolIdentifier::Name(name) = &sym.id else { return None };
    let name = name.clone();
    let ts = sym.versions[0].type_source?;
    match *analysis.ir.expr(ts) {
        Expr::TableConstructor(idx) => Some((idx, name)),
        _ => None,
    }
}

/// True when the field-access base `expr`, or any table in its base chain,
/// resolves to an `open_mixin` class (a `CreateFromMixins`-derived mixin) or a
/// class that inherits one. Walks `FieldAccess`/`Grouped` bases to the root so a
/// nested access like `self.Foo.Bar` is permissive whenever `self` is an open
/// mixin — the whole nested-frame chain is dynamic. Stops at non-field bases
/// (e.g. a `GetParent()` call result), which are checked on their own merits.
fn base_chain_is_open_mixin(analysis: &AnalysisResult, mut expr: ExprId) -> bool {
    // Depth guard against pathological/cyclic IR.
    for _ in 0..64 {
        if let Some(vt) = analysis.resolve_expr_type(expr) {
            let mut idxs: Vec<TableIndex> = Vec::new();
            super::collect_class_indices(&vt, &mut idxs);
            if idxs.iter().any(|&i| {
                analysis.table(i).open_mixin
                    || analysis.table(i).parent_classes.iter()
                        .any(|&pi| analysis.table(pi).open_mixin)
            }) {
                return true;
            }
        }
        match analysis.ir.expr(expr) {
            Expr::FieldAccess { table, .. } => expr = *table,
            Expr::Grouped(inner) | Expr::StripNil(inner) | Expr::StripFalsy(inner) => expr = *inner,
            _ => return false,
        }
    }
    false
}

/// Collect field-name-token start offsets that sit in a defensive
/// "membership-test" position — a field read whose purpose is to probe whether
/// the field *exists* before using it — so `undefined-field` is suppressed there.
/// This is the idiom WoW addons use to guard optional / version-specific API:
///   `obj.Method and obj:Method()`, `if obj.Field then ... obj:Field ... end`,
///   `frame.Custom or frame.Fallback`, `not cache.X or cache.X.y`.
///
/// Two shapes are recognized:
///   1. **Probe reads** — a field-read chain consumed purely as a boolean: an
///      `if`/`while`/`repeat` condition, an operand of `not`, an operand of
///      `and`/`or`, or a field compared with `==`/`~=`. The whole chain is being
///      probed, so every field token in it is suppressed.
///   2. **Guarded accesses** — an access protected by such a probe: the right
///      operand of `and` (`obj.M and obj:M()`), the right operand of `or` after
///      `not` (`not c.X or c.X.y`), or the body of an `if`/`while` whose
///      condition probed the same path. Only accesses whose dotted path *exactly*
///      matches a probed path are suppressed, so a *deeper* access on a
///      now-known field (`if o.cfg then o.cfg.typo end`) is still checked.
fn collect_membership_suppressions(tree: &SyntaxTree) -> HashSet<u32> {
    let mut suppress = HashSet::new();
    for node in SyntaxNode::new_root(tree).descendants() {
        match node.kind() {
            SyntaxKind::IfBranch => {
                let Some(branch) = IfBranch::cast(node) else { continue };
                let mut guards = Vec::new();
                if let Some(cond) = branch.expression() {
                    analyze_bool(&cond, true, &mut suppress, &mut guards);
                }
                if let Some(block) = branch.block() {
                    suppress_guarded_exact(block.syntax(), &guards, &mut suppress);
                }
            }
            SyntaxKind::WhileLoop => {
                let Some(wl) = WhileLoop::cast(node) else { continue };
                let mut guards = Vec::new();
                if let Some(cond) = wl.condition() {
                    analyze_bool(&cond, true, &mut suppress, &mut guards);
                }
                if let Some(block) = wl.block() {
                    suppress_guarded_exact(block.syntax(), &guards, &mut suppress);
                }
            }
            SyntaxKind::RepeatUntilLoop => {
                // The `until` condition runs *after* the body, so it cannot guard
                // the body — only suppress the probe reads in the condition.
                let Some(rl) = RepeatUntilLoop::cast(node) else { continue };
                if let Some(cond) = rl.condition() {
                    analyze_bool(&cond, true, &mut suppress, &mut Vec::new());
                }
            }
            SyntaxKind::BinaryExpression | SyntaxKind::UnaryExpression => {
                // Boolean / probe expressions in *value* context, e.g.
                // `local x = a or b.c` or `local h = obj.field ~= nil`. Process each
                // only from its outermost boolean node (so chains aren't analyzed
                // twice) and skip condition positions (handled above). `==`/`~=`
                // comparisons are dispatched here too so the assignment form of an
                // existence check is suppressed exactly like the `if` form.
                if (is_bool_op_node(&node) || is_eq_comparison_node(&node))
                    && is_bool_root(&node)
                    && !is_condition_position(&node)
                    && let Some(expr) = Expression::cast(node)
                {
                    analyze_bool(&expr, false, &mut suppress, &mut Vec::new());
                }
            }
            _ => {}
        }
    }
    suppress
}

/// Recursively walk a boolean/value expression, suppressing field reads in
/// membership-test position and collecting the dotted paths proven to exist when
/// the expression evaluates truthy (`out_guards`, used to guard `and`-RHS and
/// `if`/`while` bodies).
///
/// `boolean_ctx` is true when the expression's *value* is consumed purely as a
/// boolean (a condition, a `not` operand, or a non-final `and`/`or` operand). In
/// that case a bare field-read chain is itself a probe; in value context only the
/// short-circuiting / guarded operands are.
fn analyze_bool(
    expr: &Expression<'_>,
    boolean_ctx: bool,
    suppress: &mut HashSet<u32>,
    out_guards: &mut Vec<Vec<String>>,
) {
    match unwrap_grouped(expr) {
        Expression::BinaryExpression(bin) => {
            let terms = bin.get_terms();
            match bin.kind() {
                Operator::And => {
                    let mut lg = Vec::new();
                    if let Some(l) = terms.first() {
                        analyze_bool(l, true, suppress, &mut lg);
                    }
                    if let Some(r) = terms.get(1) {
                        // The RHS only runs when the LHS was truthy, so any access
                        // re-reading a path the LHS proved present is guarded.
                        suppress_guarded_exact(r.syntax(), &lg, suppress);
                        let mut rg = Vec::new();
                        analyze_bool(r, boolean_ctx, suppress, &mut rg);
                        out_guards.append(&mut rg);
                    }
                    out_guards.append(&mut lg);
                }
                Operator::Or => {
                    // `or` is the fallback idiom: every operand is an optional
                    // read. A `not X` operand additionally proves `X` exists in
                    // the operands that follow (`not c.X or c.X.y`).
                    let neg = terms.first().and_then(negation_guard_path);
                    if let Some(l) = terms.first() {
                        analyze_bool(l, true, suppress, &mut Vec::new());
                    }
                    if let Some(r) = terms.get(1) {
                        if let Some(g) = &neg {
                            suppress_guarded_exact(r.syntax(), std::slice::from_ref(g), suppress);
                        }
                        analyze_bool(r, true, suppress, &mut Vec::new());
                    }
                    // `a or b` being truthy proves nothing specific about either.
                }
                Operator::Equals | Operator::NotEquals => {
                    // Comparing a field chain with `==`/`~=` (commonly against
                    // `nil`) probes the field's presence — in both condition and
                    // value context (`if x.f ~= nil then` and `local h = x.f ~= nil`
                    // are the same existence check), so this is not gated on
                    // `boolean_ctx`.
                    for t in &terms {
                        mark_membership(t, suppress, out_guards);
                    }
                }
                _ => {}
            }
        }
        Expression::UnaryExpression(un) if un.kind() == Operator::Not => {
            if let Some(inner) = un.get_terms().first() {
                analyze_bool(inner, true, suppress, &mut Vec::new());
            }
            // `not X` truthy → X falsy, so it proves no positive guard here.
        }
        Expression::Identifier(ident) => {
            if boolean_ctx {
                mark_membership_ident(&ident, suppress, out_guards);
            }
        }
        // Method/function calls in boolean position are *not* membership reads
        // (calling a missing method errors at runtime), so they are left to warn.
        _ => {}
    }
}

/// Suppress a field-read chain operand and record its prefixes as guard paths.
fn mark_membership(expr: &Expression<'_>, suppress: &mut HashSet<u32>, out_guards: &mut Vec<Vec<String>>) {
    if let Expression::Identifier(ident) = unwrap_grouped(expr) {
        mark_membership_ident(&ident, suppress, out_guards);
    }
}

fn mark_membership_ident(ident: &Identifier<'_>, suppress: &mut HashSet<u32>, out_guards: &mut Vec<Vec<String>>) {
    // A chain containing a call isn't a pure field-read probe.
    if ident.contains_call() {
        return;
    }
    let path = ident.names();
    if path.len() < 2 {
        return; // a bare name is `undefined-global`, not `undefined-field`
    }
    suppress_chain_tokens(ident.syntax(), suppress);
    // Every prefix (length >= 2) is proven present when the chain is truthy.
    for k in 2..=path.len() {
        out_guards.push(path[..k].to_vec());
    }
}

/// Suppress the trailing field token of every field access within `node`'s chain
/// (a probe reads every level of the chain it dereferences).
fn suppress_chain_tokens(node: SyntaxNode<'_>, suppress: &mut HashSet<u32>) {
    for d in node.descendants() {
        if matches!(d.kind(), SyntaxKind::DotAccess | SyntaxKind::MethodCall)
            && let Some(s) = trailing_field_token_start(&d)
        {
            suppress.insert(s);
        }
    }
}

/// Within `region`, suppress field accesses whose dotted path exactly matches one
/// of `guards` (a path proven present by a preceding probe).
fn suppress_guarded_exact(region: SyntaxNode<'_>, guards: &[Vec<String>], suppress: &mut HashSet<u32>) {
    if guards.is_empty() {
        return;
    }
    for d in region.descendants() {
        if matches!(d.kind(), SyntaxKind::DotAccess | SyntaxKind::MethodCall)
            && let Some(ident) = Identifier::cast(d)
        {
            let path = ident.names();
            if path.len() >= 2
                && guards.contains(&path)
                && let Some(s) = trailing_field_token_start(&d)
            {
                suppress.insert(s);
            }
        }
    }
}

/// Start offset of the field name token directly owned by an access node — the
/// `Name` after the chain's final `.`/`:` (matches the IR's `field_range`).
fn trailing_field_token_start(node: &SyntaxNode<'_>) -> Option<u32> {
    let mut seen_sep = false;
    for c in node.children_with_tokens() {
        match c {
            NodeOrToken::Token(t) if matches!(t.kind(), SyntaxKind::Dot | SyntaxKind::Colon) => {
                seen_sep = true;
            }
            NodeOrToken::Token(t) if seen_sep && t.kind() == SyntaxKind::Name => {
                return Some(u32::from(t.text_range().start()));
            }
            _ => {}
        }
    }
    None
}

/// If `expr` is `not <field-read chain>`, return that chain's dotted path.
fn negation_guard_path(expr: &Expression<'_>) -> Option<Vec<String>> {
    if let Expression::UnaryExpression(un) = unwrap_grouped(expr)
        && un.kind() == Operator::Not
        && let Some(inner) = un.get_terms().first()
        && let Expression::Identifier(ident) = unwrap_grouped(inner)
        && !ident.contains_call()
    {
        let path = ident.names();
        if path.len() >= 2 {
            return Some(path);
        }
    }
    None
}

/// Peel `GroupedExpression` wrappers (`(expr)`) to the inner expression.
fn unwrap_grouped<'a>(expr: &Expression<'a>) -> Expression<'a> {
    let mut e = *expr;
    while let Expression::GroupedExpression(g) = e {
        match g.get_expression() {
            Some(inner) => e = inner,
            None => break,
        }
    }
    e
}

/// True for an `and`/`or` binary expression or a `not` unary expression.
fn is_bool_op_node(node: &SyntaxNode<'_>) -> bool {
    match node.kind() {
        SyntaxKind::BinaryExpression =>
            BinaryExpression::cast(*node).is_some_and(|b| matches!(b.kind(), Operator::And | Operator::Or)),
        SyntaxKind::UnaryExpression =>
            UnaryExpression::cast(*node).is_some_and(|u| u.kind() == Operator::Not),
        _ => false,
    }
}

/// True for an `==`/`~=` comparison.
fn is_eq_comparison_node(node: &SyntaxNode<'_>) -> bool {
    node.kind() == SyntaxKind::BinaryExpression
        && BinaryExpression::cast(*node)
            .is_some_and(|b| matches!(b.kind(), Operator::Equals | Operator::NotEquals))
}

/// The nearest ancestor that isn't a `GroupedExpression`.
fn skip_grouping_parent<'a>(node: &SyntaxNode<'a>) -> Option<SyntaxNode<'a>> {
    let mut p = node.parent();
    while let Some(pp) = p {
        if pp.kind() == SyntaxKind::GroupedExpression {
            p = pp.parent();
        } else {
            return Some(pp);
        }
    }
    None
}

/// True when `node` is the outermost boolean node of its chain (its non-grouping
/// parent is not itself a boolean node), so the chain is analyzed exactly once.
fn is_bool_root(node: &SyntaxNode<'_>) -> bool {
    skip_grouping_parent(node).is_none_or(|p| !is_bool_op_node(&p))
}

/// True when `node` is (the root of) an `if`/`while`/`repeat` condition — those
/// are handled by their statement nodes (which also guard the body).
fn is_condition_position(node: &SyntaxNode<'_>) -> bool {
    matches!(
        skip_grouping_parent(node).map(|p| p.kind()),
        Some(SyntaxKind::Condition | SyntaxKind::RepeatUntilLoop)
    )
}

pub(crate) struct UndefinedField;

impl DiagnosticPass for UndefinedField {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let pure_records = collect_pure_record_symbols(analysis, tree);
        let membership_suppressed = collect_membership_suppressions(tree);
        for (_, expr) in analysis.local_exprs() {
            let Expr::FieldAccess { table, field, field_range } = expr else { continue };
            let Some((start, end)) = field_range else { continue };
            let Some(table_type) = analysis.resolve_expr_type(*table) else { continue };
            if matches!(table_type, ValueType::Any) { continue; }
            // For unions, recurse into intersection/opaque-alias members so
            // mixin patterns like `(Frame & Template) | AceEvent-3.0` check
            // all tables. Top-level intersections are skipped — they're concrete
            // instances that commonly receive untracked runtime fields.
            let mut table_indices: Vec<TableIndex> = Vec::new();
            match &table_type {
                ValueType::Table(Some(idx)) => table_indices.push(*idx),
                ValueType::Union(_) => super::collect_class_indices(&table_type, &mut table_indices),
                _ => continue,
            }
            if table_indices.is_empty() { continue; }
            // `Derived = CreateFromMixins(Base)` mixin classes are dynamic
            // runtime-field-receiving instances (parentKey children attached at
            // frame creation, fields set on `self` in inherited methods). Treat
            // any access rooted at such a mixin permissively, like a top-level
            // `Frame & Template` intersection — including chains (`self.A.B.C`),
            // since the nested frame children are equally dynamic. Walks the
            // base chain to its root so `self.Foo.Bar:Method()` is skipped when
            // `self` is an open mixin.
            if base_chain_is_open_mixin(analysis, *table) { continue; }
            if table_indices.iter().any(|&idx| analysis.ir.has_field(idx, field)) { continue; }
            // Inherited field?
            if table_indices.iter().any(|&idx| {
                analysis.table(idx).parent_classes.iter().any(|&pi| analysis.ir.has_field(pi, field))
            }) { continue; }
            // _G global-env redirect: field access on _G resolves against scope-0 symbols
            if table_indices.iter().any(|&idx| analysis.ir.is_global_env(idx)) {
                let sym_id = SymbolIdentifier::Name(field.clone());
                if analysis.get_symbol(&sym_id, ScopeIndex(0)).is_some() {
                    continue;
                }
            }
            // Only emit when at least one table is a @class.
            let Some(class_name) = table_indices.iter()
                .find_map(|&idx| analysis.table(idx).class_name.clone())
            else {
                // Closed-record fallback: a plain file-local table whose entire
                // field set is statically known (the `local private = {}; function
                // private.X()` module pattern). Accessing a field never assigned on
                // it is almost certainly a typo. Requires the access base to be a
                // pure module-private table (see `collect_pure_record_symbols`).
                if let Some((idx, var_name)) = closed_record_base(analysis, *table, &pure_records)
                    && table_indices.contains(&idx)
                    && is_closed_record(analysis, idx)
                {
                    super::UNDEFINED_FIELD.emit(diags, format!("undefined field '{}' on '{}'", field, var_name), *start as usize, *end as usize);
                }
                continue;
            };
            // A field read used as a defensive existence check (`obj.M and
            // obj:M()`, `if obj.F then ... obj:F ... end`) is probing whether the
            // field exists — not a typo — so don't report it as undefined. This is
            // applied only on the `@class` path: a closed module-private record
            // has a fully-known field set that can't grow at runtime, so an unknown
            // field there is always a typo, even inside a condition.
            if membership_suppressed.contains(start) { continue; }
            // Related info: point to the @class declaration if it's in the current file.
            let related = analysis.ir.class_def_ranges.get(&class_name)
                .map(|&(cs, ce)| vec![RelatedInfo {
                    file_path: None,
                    start: cs as usize,
                    end: ce as usize,
                    message: "Class declared here".to_string(),
                }])
                .unwrap_or_default();
            super::UNDEFINED_FIELD.emit_with_related(diags, format!("undefined field '{}' on class '{}'", field, class_name), *start as usize, *end as usize, related);
        }
    }
}
