use std::collections::{HashMap, HashSet};
use std::path::Path;
use crate::ast::{AstNode, Block, Statement, Expression, FunctionCall, Operator};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use super::{
    AnnotationType, ParamInfo, Visibility,
    extract_annotations, default_visibility_for_name,
};
use super::annotation_types::{parse_overload, OverloadSig};
use super::annotation_scanning::{
    ADDON_NS_NAME, ExternalGlobal, ExternalGlobalKind, FieldValueKind, InferredTypeCategory,
    is_select_varargs, collect_statements_recursive, infer_type_category,
};

/// Unwrap `and`/`or` chains to the effective operand for type inference.
/// `a and b` evaluates to `b` when `a` is truthy, so the effective type is `b`.
/// `a or b` evaluates to `b` when `a` is falsy (the defensive-init pattern
/// `x = x or fallback`), so `b` is the best hint for the field's type.
fn unwrap_logical_chain<'a>(mut expr: Expression<'a>) -> Expression<'a> {
    loop {
        if let Expression::BinaryExpression(ref bin) = expr
            && matches!(bin.kind(), Operator::And | Operator::Or)
        {
            let terms = bin.get_terms();
            if terms.len() == 2 {
                expr = terms[1];
                continue;
            }
        }
        return expr;
    }
}

/// Extract named field kinds from a table constructor for `FieldValueKind::Table`.
fn extract_table_field_kinds(tc: &crate::ast::TableConstructor<'_>) -> Vec<(String, FieldValueKind)> {
    let mut fields = Vec::new();
    for field in tc.fields() {
        if let Some(crate::ast::FieldKind::Named { name, value }) = field.kind() {
            let kind = classify_expression_value_kind(&value);
            fields.push((name, kind));
        }
    }
    fields
}

/// Classify a literal expression (including negated number literals) into a `FieldValueKind`,
/// preserving literal values (string text, number text) when available.
fn classify_literal_value_kind(expr: &Expression<'_>) -> Option<FieldValueKind> {
    match expr {
        Expression::Literal(lit) => {
            if let Some(s) = lit.get_string() { Some(FieldValueKind::String(Some(s))) }
            else if lit.get_bool().is_some() { Some(FieldValueKind::Boolean) }
            else if let Some(n) = lit.get_number() { Some(FieldValueKind::Number(Some(n))) }
            else if lit.is_nil() { Some(FieldValueKind::Nil) }
            else { None }
        }
        Expression::UnaryExpression(u) if matches!(u.kind(), crate::ast::Operator::Subtract) => {
            super::annotation_scanning::extract_number_from_expr(expr)
                .map(|n| FieldValueKind::Number(Some(n)))
        }
        _ => None,
    }
}

/// Classify any expression into a `FieldValueKind`, using the shared
/// `infer_type_category` for operator-to-type mappings. Falls back to
/// `classify_literal_value_kind` first to preserve literal values (e.g. string
/// text, number text), then to `infer_type_category` for expression shapes, and
/// finally handles `Table`/`Function` structurally.
fn classify_expression_value_kind(expr: &Expression<'_>) -> FieldValueKind {
    // Try literal classification first to preserve exact values.
    if let Some(kind) = classify_literal_value_kind(expr) {
        return kind;
    }
    // Delegate to the shared helper for operator-based inference. Table and
    // Function are handled structurally below (Table needs recursive sub-field
    // extraction; Function is a simple tag).
    if let Some(cat) = infer_type_category(expr) {
        return match cat {
            InferredTypeCategory::String => FieldValueKind::String(None),
            InferredTypeCategory::Number => FieldValueKind::Number(None),
            InferredTypeCategory::Boolean => FieldValueKind::Boolean,
            InferredTypeCategory::Nil => FieldValueKind::Nil,
            InferredTypeCategory::Function => FieldValueKind::Function,
            // For tables we need recursive field extraction, handled below.
            InferredTypeCategory::Table => {
                if let Expression::TableConstructor(tc) = expr {
                    FieldValueKind::Table(extract_table_field_kinds(tc))
                } else {
                    FieldValueKind::Unknown
                }
            }
        };
    }
    FieldValueKind::Unknown
}

/// Emit an `ExternalGlobal` as a Method on the namespace/class, resolving
/// visibility from the function's own annotation or the field name convention.
/// When `addon_assigned_fields` is Some, also tracks the field for local-table-
/// method flush (caller passes Some only when `is_addon_root && names.len() == 2`).
fn push_method_global(
    globals: &mut Vec<ExternalGlobal>,
    canonical_name: String,
    mut base: ExternalGlobal,
    field_name: &str,
    intermediates: Vec<String>,
    implicit_protected_prefix: bool,
    addon_assigned_fields: Option<&mut HashSet<String>>,
) {
    let visibility = if base.visibility != Visibility::Public {
        base.visibility
    } else {
        default_visibility_for_name(field_name, implicit_protected_prefix)
    };
    base.name = canonical_name;
    base.kind = ExternalGlobalKind::Method(intermediates, field_name.to_string(), false);
    base.visibility = visibility;
    globals.push(base);
    if let Some(fields) = addon_assigned_fields {
        fields.insert(field_name.to_string());
    }
}

// ── Synthesized return-only overloads (workspace scan) ──────────────────────

/// Coarse synthesized return-position type. Extends
/// `Analysis::synthesized_return_type` in `build_ir.rs`: literals normalize to
/// their generic type (no literal unions), nil stays nil, comparison/`not`
/// expressions produce `boolean`, grouped expressions unwrap, and everything
/// else becomes `any`. The extra cases (comparisons, `not`) go beyond the IR
/// mirror because the workspace scanner operates on AST without full type
/// resolution, so recognizing these common patterns gives cross-file callers
/// better return type info.
fn synth_coarse_return_type(expr: &Expression<'_>) -> AnnotationType {
    match expr {
        Expression::Literal(lit) => {
            if lit.is_nil() { return AnnotationType::Simple("nil".to_string()); }
            if lit.get_string().is_some() { return AnnotationType::Simple("string".to_string()); }
            if lit.get_number().is_some() { return AnnotationType::Simple("number".to_string()); }
            if lit.get_bool().is_some() { return AnnotationType::Simple("boolean".to_string()); }
        }
        // Comparison operators always produce boolean.
        Expression::BinaryExpression(bin) if bin.kind().is_comparison() => {
            return AnnotationType::Simple("boolean".to_string());
        }
        // `not x` always produces boolean.
        Expression::UnaryExpression(un) if matches!(un.kind(), Operator::Not) => {
            return AnnotationType::Simple("boolean".to_string());
        }
        // `-N` (negated number literal) is number.
        Expression::UnaryExpression(un) if matches!(un.kind(), Operator::Subtract) => {
            if super::annotation_scanning::extract_number_from_expr(expr).is_some() {
                return AnnotationType::Simple("number".to_string());
            }
        }
        // Parenthesized expression: unwrap.
        Expression::GroupedExpression(g) => {
            if let Some(inner) = g.get_expression() {
                return synth_coarse_return_type(&inner);
            }
        }
        _ => {}
    }
    AnnotationType::Simple("any".to_string())
}

/// AST-only mirror of `Analysis::block_always_exits` in `checks.rs`. Used by
/// workspace-scan synthesis to decide whether the body falls through (implying
/// an implicit all-nil return case).
fn synth_block_always_exits(block: &Block<'_>) -> bool {
    let mut ends_with_break = false;
    for child in block.syntax().children_with_tokens() {
        match &child {
            NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BreakKeyword => {
                ends_with_break = true;
            }
            NodeOrToken::Token(tok) if matches!(
                tok.kind(),
                SyntaxKind::Whitespace | SyntaxKind::Newline | SyntaxKind::Comment
            ) => {}
            _ => {
                ends_with_break = false;
            }
        }
    }
    if ends_with_break {
        return true;
    }
    let statements = block.statements();
    let Some(last) = statements.last() else { return false };
    match last {
        Statement::Return(_) => true,
        Statement::FunctionCall(call) => {
            if let Some(ident) = call.identifier() {
                let names = ident.names();
                names.len() == 1 && names[0] == "error"
            } else {
                false
            }
        }
        // `while true do ... end` / `repeat ... until false` with no escaping
        // break never falls through. Mirrors `is_infinite_loop_stmt` in
        // checks.rs — without this, a workspace-scanned function ending in an
        // infinite loop would spuriously gain an implicit-nil tuple that the
        // per-file IR synthesizer wouldn't.
        Statement::While(_) | Statement::Repeat(_) => synth_is_infinite_loop_stmt(last),
        Statement::If(if_chain) => {
            let branches = if_chain.if_branches();
            let else_branch = if_chain.else_branch();
            if else_branch.is_none() {
                return false;
            }
            for branch in &branches {
                if let Some(block) = branch.block() {
                    if !synth_block_always_exits(&block) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            if let Some(eb) = &else_branch {
                if let Some(block) = eb.block() {
                    synth_block_always_exits(&block)
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

/// AST-only mirror of `Analysis::is_infinite_loop_stmt` in `checks.rs`.
fn synth_is_infinite_loop_stmt(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::While(wl) => {
            let Some(cond) = wl.condition() else { return false };
            if !synth_expression_is_literal_bool(&cond, true) { return false; }
            let Some(block) = wl.block() else { return false };
            !synth_node_has_escaping_break(block.syntax())
        }
        Statement::Repeat(rl) => {
            let Some(cond) = rl.condition() else { return false };
            if !synth_expression_is_literal_bool(&cond, false) { return false; }
            let Some(block) = rl.block() else { return false };
            !synth_node_has_escaping_break(block.syntax())
        }
        _ => false,
    }
}

fn synth_expression_is_literal_bool(expr: &Expression<'_>, value: bool) -> bool {
    match expr {
        Expression::Literal(lit) => lit.get_bool() == Some(value),
        Expression::GroupedExpression(g) => g
            .get_expression()
            .as_ref()
            .is_some_and(|inner| synth_expression_is_literal_bool(inner, value)),
        _ => false,
    }
}

/// Look for a `break` whose target is `node`'s containing loop. Stops at
/// nested loops and function bodies (their breaks belong to the inner loop /
/// don't escape this loop).
fn synth_node_has_escaping_break(node: SyntaxNode<'_>) -> bool {
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => {
                if tok.kind() == SyntaxKind::BreakKeyword {
                    return true;
                }
            }
            NodeOrToken::Node(sub) => match sub.kind() {
                SyntaxKind::WhileLoop
                | SyntaxKind::RepeatUntilLoop
                | SyntaxKind::ForCountLoop
                | SyntaxKind::ForInLoop
                | SyntaxKind::FunctionDefinition => {}
                _ => {
                    if synth_node_has_escaping_break(sub) {
                        return true;
                    }
                }
            },
        }
    }
    false
}

/// Walk a function body and collect per-return entries `(Vec<return_expr_types>, is_bare)`.
/// Descends into control-flow blocks (if/else/do/while/for/repeat) but NOT into
/// nested function definitions. Mirrors the per-return collection that
/// `synthesize_correlated_return_overloads` performs against `func.rets`.
fn synth_collect_returns(block: &Block<'_>, out: &mut Vec<(Vec<AnnotationType>, bool)>) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Return(ret) => {
                let exprs: Vec<AnnotationType> = ret.expression_list()
                    .map(|el| el.expressions().iter().map(synth_coarse_return_type).collect())
                    .unwrap_or_default();
                let is_bare = exprs.is_empty();
                out.push((exprs, is_bare));
            }
            Statement::If(chain) => {
                for branch in chain.if_branches() {
                    if let Some(inner) = branch.block() { synth_collect_returns(&inner, out); }
                }
                if let Some(eb) = chain.else_branch()
                    && let Some(inner) = eb.block() { synth_collect_returns(&inner, out); }
            }
            Statement::Do(g) => {
                if let Some(inner) = g.block() { synth_collect_returns(&inner, out); }
            }
            Statement::While(w) => {
                if let Some(inner) = w.block() { synth_collect_returns(&inner, out); }
            }
            Statement::Repeat(r) => {
                if let Some(inner) = r.block() { synth_collect_returns(&inner, out); }
            }
            Statement::ForCountLoop(f) => {
                if let Some(inner) = f.block() { synth_collect_returns(&inner, out); }
            }
            Statement::ForInLoop(f) => {
                if let Some(inner) = f.block() { synth_collect_returns(&inner, out); }
            }
            // Do not descend into nested functions or non-control statements —
            // their returns belong to a different function.
            _ => {}
        }
    }
}

/// If any return statement in the block ends with a function call as its last
/// (or only) expression, return that callee's dotted name (e.g. `"private.Foo"`).
/// This detects the pattern `return g()` or `return x, g()` where the trailing
/// call makes the return arity unknowable at the AST level.  The caller gates
/// invocation on the body-derived return ending with `any`, so `return x, g()`
/// is only reached when `x` itself resolved to `any`.
/// Returns `None` when no such return is found.
fn tail_call_callee_name(block: &Block<'_>) -> Option<String> {
    for stmt in block.statements() {
        match &stmt {
            Statement::Return(ret) => {
                if let Some(el) = ret.expression_list()
                    && let Some(Expression::FunctionCall(call)) = el.expressions().last()
                    && let Some(ident) = call.identifier()
                {
                    let names = ident.names();
                    if !names.is_empty() {
                        return Some(names.join("."));
                    }
                }
            }
            Statement::If(chain) => {
                for branch in chain.if_branches() {
                    if let Some(inner) = branch.block()
                        && let Some(n) = tail_call_callee_name(&inner) { return Some(n); }
                }
                if let Some(eb) = chain.else_branch()
                    && let Some(inner) = eb.block()
                    && let Some(n) = tail_call_callee_name(&inner) { return Some(n); }
            }
            Statement::Do(g) => {
                if let Some(inner) = g.block()
                    && let Some(n) = tail_call_callee_name(&inner) { return Some(n); }
            }
            Statement::While(w) => {
                if let Some(inner) = w.block()
                    && let Some(n) = tail_call_callee_name(&inner) { return Some(n); }
            }
            Statement::Repeat(r) => {
                if let Some(inner) = r.block()
                    && let Some(n) = tail_call_callee_name(&inner) { return Some(n); }
            }
            Statement::ForCountLoop(f) => {
                if let Some(inner) = f.block()
                    && let Some(n) = tail_call_callee_name(&inner) { return Some(n); }
            }
            Statement::ForInLoop(f) => {
                if let Some(inner) = f.block()
                    && let Some(n) = tail_call_callee_name(&inner) { return Some(n); }
            }
            _ => {}
        }
    }
    None
}

/// Follow a tail-call chain within the same file to find the terminal callee's
/// body-derived returns.  Returns `None` when the chain leads outside the file
/// or exceeds the depth limit.
fn resolve_through_tail_calls<'a>(
    callee: &str,
    returns_map: &'a HashMap<String, Vec<AnnotationType>>,
    tail_callees: &HashMap<String, String>,
) -> Option<&'a Vec<AnnotationType>> {
    let mut name = callee;
    for _ in 0..10 {
        match tail_callees.get(name) {
            Some(next) => name = next,
            None => return returns_map.get(name),
        }
    }
    None
}

/// When a function body returns a single identifier that names a local function
/// with a captured signature, build the corresponding `AnnotationType::Fun(...)`
/// so the outer function's return type is preserved cross-file. Checks both
/// the file-level `local_function_sigs` and function definitions within the
/// body (recursing into control-flow blocks). Returns `None` when:
/// - no identifier returns are found
/// - any bare return (implicit nil) exists alongside identifier returns
/// - multiple different identifiers are returned
/// - the identifier doesn't resolve to a known local function
///
/// Note: `AnnotationType::Fun` cannot represent overloads or generics, so those
/// are lost if the inner function has them. This is acceptable since the primary
/// use case is simple factory functions returning typed inner functions.
fn resolve_returned_local_func_type(
    body: &Block<'_>,
    local_function_sigs: &HashMap<String, ExternalGlobal>,
) -> Option<AnnotationType> {
    let mut has_bare_return = false;
    let mut returns = Vec::new();
    synth_collect_returned_identifiers(body, &mut returns, &mut has_bare_return);
    // If any bare return exists, the function can return nil on some paths,
    // so we can't confidently say the return type is just the inner function.
    if has_bare_return { return None; }
    // Only resolve when ALL returns agree on the same single identifier.
    let first = returns.first()?;
    if !returns.iter().all(|n| n == first) { return None; }
    // First check file-level local function signatures.
    if let Some(sig) = local_function_sigs.get(first) {
        return Some(external_global_to_fun_type(sig));
    }
    // Then check local function definitions within this body (recurses into
    // control-flow blocks to match synth_collect_returned_identifiers scope).
    find_local_func_def_in_block(body, first)
}

/// Convert an `ExternalGlobal` function signature to `AnnotationType::Fun(...)`.
/// Note: discards overloads and generics since `AnnotationType::Fun` cannot
/// represent them.
fn external_global_to_fun_type(sig: &ExternalGlobal) -> AnnotationType {
    let params: Vec<ParamInfo> = sig.params.clone();
    let ret_types: Vec<AnnotationType> = sig.returns.clone();
    let is_vararg = sig.params.iter().any(|p| p.name == "...");
    AnnotationType::Fun(params, ret_types, is_vararg)
}

/// Build `AnnotationType::Fun(...)` from a function definition's annotations
/// and parameter list. Note: discards overloads and generics since
/// `AnnotationType::Fun` cannot represent them.
fn func_def_to_fun_type(func: &crate::ast::FunctionDefinition<'_>) -> AnnotationType {
    let annotations = extract_annotations(func.syntax());
    let mut params = Vec::new();
    let mut is_vararg = false;
    if let Some(param_list) = func.params() {
        for name in param_list.parameters() {
            if let Some(ann) = annotations.params.iter().find(|p| p.name == name) {
                params.push(ann.clone());
            } else {
                params.push(ParamInfo { name, typ: AnnotationType::Simple(String::new()), optional: false, description: None });
            }
        }
        if param_list.ellipsis() {
            is_vararg = true;
            if let Some(ann) = annotations.params.iter().find(|p| p.name == "...") {
                params.push(ann.clone());
            }
        }
    }
    AnnotationType::Fun(params, annotations.returns, is_vararg)
}

/// Recursively search a block (and nested control-flow blocks) for a local
/// function definition with the given name. Mirrors the recursion pattern of
/// `synth_collect_returned_identifiers` so both look in the same scopes.
fn find_local_func_def_in_block(block: &Block<'_>, name: &str) -> Option<AnnotationType> {
    for stmt in block.statements() {
        match &stmt {
            Statement::FunctionDefinition(func) if func.is_local() => {
                if func.name().as_deref() == Some(name) {
                    return Some(func_def_to_fun_type(func));
                }
            }
            Statement::LocalAssign(assign) => {
                if let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                    let names = name_list.names();
                    let exprs = expr_list.expressions();
                    if names.len() == 1 && names[0] == name && exprs.len() == 1
                        && let Expression::Function(fd) = &exprs[0] {
                            return Some(func_def_to_fun_type(fd));
                        }
                }
            }
            Statement::If(chain) => {
                for branch in chain.if_branches() {
                    if let Some(inner) = branch.block()
                        && let Some(t) = find_local_func_def_in_block(&inner, name) { return Some(t); }
                }
                if let Some(eb) = chain.else_branch()
                    && let Some(inner) = eb.block()
                    && let Some(t) = find_local_func_def_in_block(&inner, name) { return Some(t); }
            }
            Statement::Do(g) => {
                if let Some(inner) = g.block()
                    && let Some(t) = find_local_func_def_in_block(&inner, name) { return Some(t); }
            }
            Statement::While(w) => {
                if let Some(inner) = w.block()
                    && let Some(t) = find_local_func_def_in_block(&inner, name) { return Some(t); }
            }
            Statement::Repeat(r) => {
                if let Some(inner) = r.block()
                    && let Some(t) = find_local_func_def_in_block(&inner, name) { return Some(t); }
            }
            Statement::ForCountLoop(f) => {
                if let Some(inner) = f.block()
                    && let Some(t) = find_local_func_def_in_block(&inner, name) { return Some(t); }
            }
            Statement::ForInLoop(f) => {
                if let Some(inner) = f.block()
                    && let Some(t) = find_local_func_def_in_block(&inner, name) { return Some(t); }
            }
            _ => {}
        }
    }
    None
}

/// Collect all single-identifier return expressions from a block (recursing
/// into control flow but not into nested functions). Also tracks whether any
/// bare return (no expression list, or empty expression list) is encountered,
/// which signals the function can return nil on some paths.
fn synth_collect_returned_identifiers(block: &Block<'_>, out: &mut Vec<String>, has_bare_return: &mut bool) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Return(ret) => {
                match ret.expression_list() {
                    None => { *has_bare_return = true; }
                    Some(el) => {
                        let exprs = el.expressions();
                        if exprs.is_empty() {
                            *has_bare_return = true;
                        } else if exprs.len() == 1
                            && let Expression::Identifier(ident) = &exprs[0] {
                                let names = ident.names();
                                if names.len() == 1 {
                                    out.push(names[0].clone());
                                }
                            }
                    }
                }
            }
            Statement::If(chain) => {
                for branch in chain.if_branches() {
                    if let Some(inner) = branch.block() { synth_collect_returned_identifiers(&inner, out, has_bare_return); }
                }
                if let Some(eb) = chain.else_branch()
                    && let Some(inner) = eb.block() { synth_collect_returned_identifiers(&inner, out, has_bare_return); }
            }
            Statement::Do(g) => {
                if let Some(inner) = g.block() { synth_collect_returned_identifiers(&inner, out, has_bare_return); }
            }
            Statement::While(w) => {
                if let Some(inner) = w.block() { synth_collect_returned_identifiers(&inner, out, has_bare_return); }
            }
            Statement::Repeat(r) => {
                if let Some(inner) = r.block() { synth_collect_returned_identifiers(&inner, out, has_bare_return); }
            }
            Statement::ForCountLoop(f) => {
                if let Some(inner) = f.block() { synth_collect_returned_identifiers(&inner, out, has_bare_return); }
            }
            Statement::ForInLoop(f) => {
                if let Some(inner) = f.block() { synth_collect_returned_identifiers(&inner, out, has_bare_return); }
            }
            _ => {}
        }
    }
}

/// AST-only mirror of `Analysis::synthesize_correlated_return_overloads` in
/// `build_ir.rs`. Walks the given function body and returns synthesized
/// return-only overloads when the body matches the all-set-or-all-nil pattern.
///
/// This runs during workspace scanning (before IR construction) so that
/// cross-file method calls — which resolve through `PreResolvedGlobals` to an
/// external `Function` — pick up the synthesized overloads alongside the
/// per-file IR-level synthesis. Without this, a call like `self:_Helper()` in
/// a `DefineClassType`-registered class resolves to the external function,
/// which has no overloads and therefore no sibling narrowing.
///
/// Precondition: caller has already verified `annotations.returns.is_empty()`
/// (no `@return` annotations) and that no existing overloads have
/// `is_return_only` set.
/// Synthesize correlated return-only overloads from pre-collected body returns.
/// The caller collects returns via [`synth_collect_returns`] once and can reuse
/// the collection for other purposes (e.g. `implicit_nil_return` detection)
/// without walking the body twice.
fn synthesize_return_only_overloads_from(
    returns: Vec<(Vec<AnnotationType>, bool)>,
    body: &Block<'_>,
) -> Vec<OverloadSig> {
    // Split explicit multi-value returns from bare / empty returns.
    let implicit_nil = returns.iter().any(|(_, is_bare)| *is_bare)
        || !synth_block_always_exits(body);

    // `is_bare` <=> `exprs.is_empty()` (set together in `synth_collect_returns`),
    // so dropping bare returns alone fully partitions the list.
    let explicit: Vec<Vec<AnnotationType>> = returns.into_iter()
        .filter(|(_, is_bare)| !*is_bare)
        .map(|(exprs, _)| exprs)
        .collect();

    // Match build_ir: need ≥2 signatures total (counting implicit_nil as one).
    if explicit.len() + if implicit_nil { 1 } else { 0 } < 2 { return Vec::new(); }

    // Compute max arity across all explicit returns (must be ≥ 2).
    // Shorter returns are padded with nil — consistent with Lua trailing-nil
    // semantics, and matching the relaxation in build_ir's synthesis path.
    let max_arity = explicit.iter().map(|t| t.len()).max().unwrap_or(0);
    if max_arity < 2 { return Vec::new(); }

    let mut tuples: Vec<Vec<AnnotationType>> = explicit.into_iter().map(|mut tuple| {
        // When the last element is `any` (an unresolved expression — typically a
        // function call), pad with `any` rather than `nil`. In Lua, a tail-call
        // `return someFunc()` passes through all of the callee's return values,
        // so positions beyond the explicit count are unknown, not definitely nil.
        let pad = if tuple.last().is_some_and(|t| matches!(t, AnnotationType::Simple(s) if s == "any")) {
            AnnotationType::Simple("any".to_string())
        } else {
            AnnotationType::Simple("nil".to_string())
        };
        while tuple.len() < max_arity {
            tuple.push(pad.clone());
        }
        tuple
    }).collect();
    if implicit_nil {
        tuples.push(vec![AnnotationType::Simple("nil".to_string()); max_arity]);
    }

    // Dedupe by tuple; require ≥ 2 distinct signatures.
    let mut emitted: Vec<Vec<AnnotationType>> = Vec::new();
    for returns in tuples {
        if emitted.iter().any(|e| e == &returns) { continue; }
        emitted.push(returns);
    }
    if emitted.len() < 2 { return Vec::new(); }

    emitted.into_iter().map(|returns| OverloadSig {
        params: Vec::new(),
        returns,
        is_vararg: false,
        is_return_only: true,
    }).collect()
}

/// Build a complete [`ExternalGlobal`] capturing a function definition's full
/// signature: params (merged with `@param` annotations), `@return`/body-derived
/// returns (including tail-call resolution and `...VarArgs` fallback), overloads
/// (including synthesized correlated return-only overloads), `@deprecated`/
/// `@nodiscard`/visibility/generics/etc., and the function's byte range.
///
/// The returned global has placeholder `name` (empty) and `kind`
/// ([`ExternalGlobalKind::Function`]); callers override them via struct-update
/// syntax (`ExternalGlobal { name, kind, ..base }`) to emit the function as a
/// top-level global, a method on a table/namespace, or a namespace field
/// assigned a local function (`ns.f = f`).
fn build_func_external(
    func: &crate::ast::FunctionDefinition<'_>,
    anno_node: SyntaxNode<'_>,
    is_colon: bool,
    correlated_return_overloads: bool,
    file_func_returns: &HashMap<String, Vec<AnnotationType>>,
    file_func_tail_callee: &HashMap<String, String>,
    owned_path: Option<&std::path::Path>,
) -> ExternalGlobal {
    // Annotations may live on an enclosing statement rather than the function
    // node itself (e.g. `---@param ...\nlocal f = function() end`), so the
    // caller specifies which node to scan for `---@` comments.
    let mut annotations = extract_annotations(anno_node);
    let mut overloads: Vec<OverloadSig> = annotations.overloads.iter()
        .filter_map(|s| parse_overload(s)).collect();
    // Collect body returns once (when no @return annotations) for
    // both overload synthesis and implicit_nil_return detection.
    let body_returns = if annotations.returns.is_empty() {
        func.block().map(|body| {
            let mut returns = Vec::new();
            synth_collect_returns(&body, &mut returns);
            returns
        })
    } else {
        None
    };
    // Implicit nil return: every return in the body is bare (no
    // expressions), or the body has no return statements at all.
    let implicit_nil_return = body_returns.as_ref()
        .is_some_and(|returns| returns.iter().all(|(_, is_bare)| *is_bare));
    // Derive primary return types from body when no @return
    // annotations exist.  Picks the max-arity explicit return to
    // determine the number of return slots and their coarse types.
    // When multiple returns share max arity, widen each position:
    // positions where all paths agree keep that type, positions
    // where paths disagree (e.g. nil vs any) widen to `any`.
    let body_derived_returns: Vec<AnnotationType> = body_returns.as_ref()
        .and_then(|returns| {
            let non_bare: Vec<_> = returns.iter()
                .filter(|(_, is_bare)| !*is_bare)
                .collect();
            let max_arity = non_bare.iter()
                .map(|(exprs, _)| exprs.len())
                .max()?;
            let max_returns: Vec<&Vec<AnnotationType>> = non_bare.iter()
                .filter(|(exprs, _)| exprs.len() == max_arity)
                .map(|(exprs, _)| exprs)
                .collect();
            if max_returns.len() == 1 {
                return Some(max_returns[0].clone());
            }
            // Multiple returns with same max arity: widen types
            // at each position so cross-file callers see the
            // combined return type.
            let mut result = Vec::with_capacity(max_arity);
            for i in 0..max_arity {
                let first = &max_returns[0][i];
                if max_returns[1..].iter().all(|r| r[i] == *first) {
                    result.push(first.clone());
                } else {
                    result.push(AnnotationType::Simple("any".to_string()));
                }
            }
            Some(result)
        })
        .unwrap_or_default();
    // When the body-derived return ends with `any` (typically from
    // a tail-call function call), the actual return arity is unknown
    // at scan time.  Try to resolve the callee within the same file
    // to get concrete return types; otherwise fall back to VarArgs.
    //
    // NOTE: The widening above can also produce a trailing `any` when
    // paths disagree at the last position, but that is harmless —
    // `tail_call_callee_name` gates on the AST actually ending with a
    // tail call, so a widened `any` won't trigger the VarArgs fallback
    // spuriously.
    let tail_callee = if body_derived_returns.last()
        .is_some_and(|t| matches!(t, AnnotationType::Simple(s) if s == "any"))
    {
        func.block().and_then(|body| tail_call_callee_name(&body))
    } else {
        None
    };
    // Synthesize correlated return-only overloads from the pre-collected
    // returns.  Matches the per-file IR synthesis so cross-file call sites
    // also see the synthesized overloads.
    if correlated_return_overloads
        && !overloads.iter().any(|o| o.is_return_only)
        && let Some(body) = func.block()
        && let Some(returns) = body_returns {
            overloads.extend(synthesize_return_only_overloads_from(returns, &body));
        }
    // Populate returns from body-derived types when no @return annotations
    // exist.  This gives cross-file callers the correct return arity and
    // coarse types (comparisons → boolean, literals → their type, everything
    // else → any).
    let is_body_derived = annotations.returns.is_empty() && !body_derived_returns.is_empty();
    if is_body_derived {
        if let Some(callee) = &tail_callee {
            // Tail-call return: resolve through same-file functions to find
            // the terminal callee's concrete return types.
            if let Some(resolved) = resolve_through_tail_calls(
                callee, file_func_returns, file_func_tail_callee,
            ) {
                annotations.returns = resolved.clone();
            } else {
                // Callee not in this file; keep VarArgs(any) to signal
                // open-ended arity.
                let last = body_derived_returns.last().unwrap().clone();
                let mut returns = body_derived_returns;
                let len = returns.len();
                returns[len - 1] = AnnotationType::VarArgs(Box::new(last));
                annotations.returns = returns;
            }
        } else {
            annotations.returns = body_derived_returns;
        }
    }
    let range = func.syntax().text_range();
    let def_start = u32::from(range.start());
    let def_end = u32::from(range.end());
    // Merge @param annotations with actual parameter names.
    let params = if let Some(param_list) = func.params() {
        let actual_params: Vec<String> = param_list.parameters().into_iter()
            .filter(|n| !is_colon || n != "self")
            .collect();
        let mut ps: Vec<ParamInfo> = actual_params.iter()
            .map(|n| {
                if let Some(ann) = annotations.params.iter().find(|p| &p.name == n) {
                    ann.clone()
                } else {
                    ParamInfo { name: n.clone(), typ: AnnotationType::Simple(String::new()), optional: false, description: None }
                }
            })
            .collect();
        if param_list.ellipsis() {
            if let Some(ann) = annotations.params.iter().find(|p| p.name == "...") {
                ps.push(ann.clone());
            } else {
                ps.push(ParamInfo { name: "...".to_string(), typ: AnnotationType::Simple(String::new()), optional: false, description: None });
            }
        }
        ps
    } else if !annotations.params.is_empty() {
        std::mem::take(&mut annotations.params)
    } else { Vec::new() };
    ExternalGlobal {
        name: String::new(),
        kind: ExternalGlobalKind::Function,
        params,
        returns: annotations.returns,
        return_names: annotations.return_names,
        return_descriptions: annotations.return_descriptions,
        overloads,
        doc: annotations.doc,
        deprecated: annotations.deprecated,
        nodiscard: annotations.nodiscard,
        constructor: annotations.constructor,
        visibility: annotations.visibility,
        generics: annotations.generics,
        defclass: annotations.defclass,
        defclass_parent: annotations.defclass_parent,
        source_path: owned_path.map(|p| p.to_path_buf()),
        def_start,
        def_end,
        builds_field: annotations.builds_field,
        built_name: annotations.built_name,
        built_extends: annotations.built_extends,
        type_narrows: annotations.type_narrows,
        type_narrows_class: annotations.type_narrows_class,
        string_value: None,
        number_value: None,
        is_override: false,
        see: annotations.see,
        flavors: 0,
        flavor_guard: annotations.flavor_guard,
        implicit_nil_return,
        narrows_arg: annotations.narrows_arg,
        requires: annotations.requires,
        body_derived_returns: is_body_derived,
    }
}

// ── Dynamic global prefix scanning ──────────────────────────────────────────

/// Minimum length for a string literal in `_G["PREFIX"..k]` to be considered
/// a valid dynamic global prefix. Short prefixes risk masking real
/// `undefined-global` diagnostics.
const MIN_DYNAMIC_PREFIX_LEN: usize = 3;

/// Check if a bracket-access node has `_G` as its base expression.
fn is_g_bracket_access(node: SyntaxNode<'_>) -> bool {
    for child in node.children_with_tokens() {
        match &child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::LeftSquareBracket => break,
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name && t.text() == "_G" => {
                return true;
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::NameRef => {
                for c in n.children_with_tokens() {
                    if let NodeOrToken::Token(t) = c
                        && t.kind() == SyntaxKind::Name
                        && t.text() == "_G"
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Scan a file for `_G["PREFIX" .. k] = value` patterns and return glob patterns
/// like `"PREFIX*"` for each detected prefix. These patterns are added as
/// workspace-wide allowed globals so that reads of `PREFIX<anything>` in other
/// files don't false-positive as `undefined-global`.
///
/// Detection is deliberately conservative:
///
/// - Only `_G[<concat>]` on the LHS of an assignment is recognized.
/// - The concatenation must have a string literal operand of at least 3 chars.
/// - Fully dynamic writes (`_G[k] = v` with no literal part) are ignored.
pub fn scan_dynamic_global_prefixes(root: SyntaxNode<'_>) -> Vec<String> {
    let Some(block) = Block::cast(root) else { return Vec::new(); };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);

    let mut seen = std::collections::HashSet::new();
    let mut prefixes = Vec::new();
    for stmt in &all_stmts {
        if let Statement::Assign(assign) = stmt
            && let Some(var_list) = assign.variable_list()
        {
            for ident in var_list.identifiers() {
                if ident.syntax().kind() != SyntaxKind::BracketAccess {
                    continue;
                }
                if !is_g_bracket_access(ident.syntax()) {
                    continue;
                }
                if let Some((literal, is_prefix)) = crate::ast::extract_bracket_concat_string_literal(ident.syntax())
                    && literal.len() >= MIN_DYNAMIC_PREFIX_LEN
                {
                    let pattern = if is_prefix {
                        format!("{}*", literal)
                    } else {
                        format!("*{}", literal)
                    };
                    if seen.insert(pattern.clone()) {
                        prefixes.push(pattern);
                    }
                }
            }
        }
    }
    prefixes
}

// ── Global declaration scanning ─────────────────────────────────────────────

pub fn scan_file_globals(root: SyntaxNode<'_>, source_path: Option<&Path>) -> Vec<ExternalGlobal> {
    scan_file_globals_with_synth(root, source_path, true, false).0
}

/// Variant of [`scan_file_globals`] that lets the caller disable workspace-level
/// synthesis of correlated return-only overloads for a specific file. The LSP /
/// CLI paths consult `inference.correlated_return_overloads` per-file; stub
/// generation leaves it on.
/// Returns `(globals, addon_ns_class_name)`.
/// `addon_ns_class_name` is `Some(class_name)` when the addon namespace variable
/// (the second value from `...`) also has a `@class` annotation, establishing a
/// relationship between the addon namespace table and a named class.
pub(crate) fn scan_file_globals_with_synth(
    root: SyntaxNode<'_>,
    source_path: Option<&Path>,
    correlated_return_overloads: bool,
    implicit_protected_prefix: bool,
) -> (Vec<ExternalGlobal>, Option<String>) {
    let owned_path = source_path.map(|p| p.to_path_buf());
    let Some(block) = Block::cast(root) else { return (Vec::new(), None); };

    let mut addon_ns_var: Option<String> = None;
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                if names.len() >= 2 && exprs.len() == 1 && matches!(exprs[0], Expression::VarArgs(_)) {
                    addon_ns_var = Some(names[1].clone());
                    break;
                }
                // local ns = select(2, ...)
                if !names.is_empty() && exprs.len() == 1
                    && let Some(n) = is_select_varargs(&exprs[0])
                        && n == 2 {
                            addon_ns_var = Some(names[0].clone());
                            break;
                        }
            }
    }

    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);

    // Track local aliases to known tables (e.g. `local str = string`, `local tab = table`)
    let mut local_aliases: HashMap<String, String> = HashMap::new();
    // Track local variables assigned table constructors (e.g. `local Locale = {}`)
    let mut local_tables: HashSet<String> = HashSet::new();
    // Track local functions (e.g. `local function Foo()` or `local Foo = function()`)
    let mut local_functions: HashSet<String> = HashSet::new();
    // Track ALL local variable names so we can skip field/method assignments on
    // non-class, non-table locals (e.g. `local frame = CreateFrame(...); frame.x = 1`
    // should not create a phantom global class "frame").
    let mut local_vars: HashSet<String> = HashSet::new();
    // Track local variables annotated with @class (e.g. local LibTSMCore = {} ---@class LibTSMCore)
    let mut class_vars: HashMap<String, String> = HashMap::new();
    // Track locals with @type annotations so field assignments on them are emitted
    // under the annotated class name (cross-file overlay tracking).
    let mut local_type_vars: HashMap<String, String> = HashMap::new();
    for stmt in &all_stmts {
        if let Statement::LocalAssign(assign) = stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                for name in &names {
                    local_vars.insert(name.clone());
                }
                if names.len() == 1 && exprs.len() == 1 {
                    if let Expression::Identifier(ident) = &exprs[0] {
                        let rhs_names = ident.names();
                        if rhs_names.len() == 1 {
                            local_aliases.insert(names[0].clone(), rhs_names[0].clone());
                        }
                    }
                    if matches!(&exprs[0], Expression::TableConstructor(_)) {
                        local_tables.insert(names[0].clone());
                    }
                    if matches!(&exprs[0], Expression::Function(_)) {
                        local_functions.insert(names[0].clone());
                    }
                }

                // @class annotation (preceding or inline trailing comment)
                let annotations = extract_annotations(assign.syntax());
                let class_name = annotations.class
                    .or_else(|| super::extract_inline_class(assign.syntax()));
                if let Some(class_name) = class_name {
                    if names.len() == 1 {
                        class_vars.insert(names[0].clone(), class_name);
                    } else if names.len() >= 2 {
                        // @class on multi-assignment annotates the first variable
                        // (e.g. `---@class Lib \n local Lib, oldMinor = LibStub:NewLibrary(...)`)
                        // Also handle addon namespace in last position.
                        if addon_ns_var.as_deref() == Some(names.last().unwrap().as_str()) {
                            class_vars.insert(names.last().unwrap().clone(), class_name.clone());
                        }
                        class_vars.insert(names[0].clone(), class_name);
                    }
                } else if names.len() == 1 && !class_vars.contains_key(&names[0]) {
                    // @type annotation → track as local_type_vars for overlay field emission.
                    // Also populate class_vars when the variable name matches the type name
                    // (e.g. `---@type Glider \n local Glider = ns.GliderUI`), so methods
                    // defined on the local are associated with the class cross-file.
                    // Only Simple types (and the Simple member of Intersection) are matched.
                    let type_name = match &annotations.var_type {
                        Some(AnnotationType::Simple(s)) => Some(s.clone()),
                        Some(AnnotationType::Intersection(members)) => {
                            members.iter().find_map(|m| {
                                if let AnnotationType::Simple(s) = m { Some(s.clone()) } else { None }
                            })
                        }
                        _ => None,
                    };
                    if let Some(type_name) = type_name {
                        if names[0] == type_name {
                            class_vars.insert(names[0].clone(), type_name.clone());
                        }
                        local_type_vars.insert(names[0].clone(), type_name);
                    }
                    // Defclass-style calls: `local X = Y:Init("ClassName")` or `local X = DefineClass("ClassName")`
                    if exprs.len() == 1
                        && let Expression::FunctionCall(call) = &exprs[0] {
                            if let Some(cn) = extract_string_arg_from_call_chain(call) {
                                class_vars.insert(names[0].clone(), cn);
                            } else if let Some(cn) = extract_first_string_arg(call) {
                                class_vars.insert(names[0].clone(), cn);
                            }
                        }
                }
            }
        if let Statement::FunctionDefinition(func) = stmt
            && func.is_local()
            && let Some(name) = func.name() {
                local_vars.insert(name.clone());
                local_functions.insert(name);
            }
        // Track @class annotations on global assignments (e.g. `---@class Foo\nMyMixin = {}`)
        // so that methods defined on the global are emitted under the class name.
        // Also handles `local name, AddOn = ... \n ---@class Foo \n AddOn = LibStub(...)`:
        // the variable is in local_vars but still needs class_vars tracking for field routing.
        if let Statement::Assign(assign) = stmt
            && let (Some(var_list), Some(expr_list)) = (assign.variable_list(), assign.expression_list()) {
                let idents = var_list.identifiers();
                let exprs = expr_list.expressions();
                if idents.len() == 1 && exprs.len() == 1 {
                    let names = idents[0].names();
                    if names.len() == 1 {
                        let annotations = extract_annotations(assign.syntax());
                        let class_name = annotations.class
                            .or_else(|| super::extract_inline_class(assign.syntax()));
                        if let Some(class_name) = class_name {
                            class_vars.insert(names[0].clone(), class_name);
                        }
                    }
                }
            }
    }

    // Track return types of same-file function definitions (e.g. `---@return Foo \n function X.bar()`)
    // so that `local x = X.bar(); Class.field = x` propagates `Foo` as the field type cross-file.
    let mut func_return_types: HashMap<String, AnnotationType> = HashMap::new();
    for stmt in &all_stmts {
        if let Statement::FunctionDefinition(func) = stmt {
            if func.is_local() { continue; }
            let Some(ident) = func.identifier() else { continue };
            let func_names = ident.names();
            if func_names.is_empty() { continue; }
            let annotations = extract_annotations(func.syntax());
            if let Some(ret) = annotations.returns.into_iter().next()
                && !matches!(&ret, AnnotationType::Simple(s) if s.is_empty()) {
                    func_return_types.insert(func_names.join("."), ret);
            }
        }
    }

    // Track local variable return types from annotated function calls
    let mut local_return_types: HashMap<String, AnnotationType> = HashMap::new();
    // Track local variables assigned from function calls whose return type isn't known
    // locally (e.g. stub/external methods).  Stores (canonicalized callee chain, first
    // string arg) so that `ns.Field = localVar` can emit FieldValueKind::FunctionCall
    // instead of FieldRef, letting build_on_stubs resolve the return type.
    let mut local_call_origins: HashMap<String, (Vec<String>, Option<String>)> = HashMap::new();
    for stmt in &all_stmts {
        if let Statement::LocalAssign(assign) = stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                if names.len() == 1 && exprs.len() == 1 && !class_vars.contains_key(&names[0])
                    && let Expression::FunctionCall(call) = &exprs[0]
                    && let Some(call_ident) = call.identifier() {
                        let call_names = call_ident.names();
                        if !call_names.is_empty() {
                            let func_key = call_names.join(".");
                            if let Some(ret_type) = func_return_types.get(&func_key) {
                                local_return_types.insert(names[0].clone(), ret_type.clone());
                            } else {
                                // Return type not known from same-file definitions;
                                // store the call origin so build_on_stubs can resolve it.
                                let mut callee_chain = call_names;
                                if !callee_chain.is_empty() {
                                    if addon_ns_var.as_deref() == Some(callee_chain[0].as_str()) {
                                        callee_chain[0] = ADDON_NS_NAME.to_string();
                                    } else if let Some(cn) = class_vars.get(&callee_chain[0]) {
                                        callee_chain[0] = cn.clone();
                                    } else if let Some(tn) = local_type_vars.get(&callee_chain[0]) {
                                        callee_chain[0] = tn.clone();
                                    }
                                }
                                let first_string_arg = call.arguments().and_then(|al| {
                                    let args = al.expressions();
                                    if let Some(Expression::Literal(lit)) = args.first() {
                                        lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                    } else {
                                        None
                                    }
                                });
                                local_call_origins.insert(names[0].clone(), (callee_chain, first_string_arg));
                            }
                        }
                    }
            }
    }

    // Pre-pass: collect body-derived returns for ALL functions in this file
    // (including private/local ones), keyed by dotted name.  Also track which
    // functions are tail-call wrappers and their callee names.  This enables
    // tail-call resolution within the same file so cross-file callers see the
    // concrete return types of the terminal callee rather than just `any`.
    let mut file_func_returns: HashMap<String, Vec<AnnotationType>> = HashMap::new();
    let mut file_func_tail_callee: HashMap<String, String> = HashMap::new();
    for stmt in &all_stmts {
        if let Statement::FunctionDefinition(func) = stmt {
            let func_names = if let Some(ident) = func.identifier() {
                ident.names()
            } else if let Some(name) = func.name() {
                vec![name]
            } else {
                continue;
            };
            if func_names.is_empty() { continue; }
            let key = func_names.join(".");
            let ann = extract_annotations(func.syntax());
            if !ann.returns.is_empty() {
                file_func_returns.insert(key, ann.returns);
                continue;
            }
            if let Some(body) = func.block() {
                let mut returns = Vec::new();
                synth_collect_returns(&body, &mut returns);
                let body_derived: Vec<AnnotationType> = returns.iter()
                    .filter(|(_, is_bare)| !*is_bare)
                    .max_by_key(|(exprs, _)| exprs.len())
                    .map(|(exprs, _)| exprs.clone())
                    .unwrap_or_default();
                if !body_derived.is_empty() {
                    if body_derived.last()
                        .is_some_and(|t| matches!(t, AnnotationType::Simple(s) if s == "any"))
                        && let Some(callee) = tail_call_callee_name(&body)
                    {
                        file_func_tail_callee.insert(key.clone(), callee);
                    }
                    file_func_returns.insert(key, body_derived);
                }
            }
        }
    }

    // Capture full signatures of local functions (both `local function f()` and
    // `local f = function()`) keyed by name. Assigning a local function to a
    // namespace/class field (`ns.f = f`) then re-uses the captured signature so
    // the params/returns survive cross-file, instead of degrading to a bare
    // `function` type. Must run after `file_func_returns`/`file_func_tail_callee`
    // are populated so body-derived returns resolve through tail calls.
    let mut local_function_sigs: HashMap<String, ExternalGlobal> = HashMap::new();
    for stmt in &all_stmts {
        match stmt {
            Statement::FunctionDefinition(func) if func.is_local() => {
                if let Some(name) = func.name() {
                    local_function_sigs.insert(name, build_func_external(
                        func, func.syntax(), false, correlated_return_overloads,
                        &file_func_returns, &file_func_tail_callee, owned_path.as_deref(),
                    ));
                }
            }
            Statement::LocalAssign(assign) => {
                if let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                    let names = name_list.names();
                    let exprs = expr_list.expressions();
                    if names.len() == 1 && exprs.len() == 1
                        && let Expression::Function(fd) = &exprs[0] {
                            local_function_sigs.insert(names[0].clone(), build_func_external(
                                fd, assign.syntax(), false, correlated_return_overloads,
                                &file_func_returns, &file_func_tail_callee, owned_path.as_deref(),
                            ));
                        }
                }
            }
            _ => {}
        }
    }

    let mut globals = Vec::new();

    // Detect CreateFrame/CreateFont/CreateFontFamily calls with string-literal
    // name arguments. These calls implicitly create a named global as a side
    // effect; registering it eliminates false `undefined-global` at read sites
    // in other files (mirrors how xml_scan.rs registers name= frames as
    // ExternalGlobal). Runs before the main statement loop so these entries
    // appear first in the globals vec and win initial registration in
    // build_on_stubs — the explicit `returns` type is needed because the
    // deferred resolution path filters out CreateFrame's generic TypeVariable.
    for stmt in &all_stmts {
        let call = match stmt {
            Statement::FunctionCall(c) => Some(*c),
            Statement::LocalAssign(a) => {
                a.expression_list().and_then(|el| {
                    el.expressions().into_iter().find_map(|e| {
                        if let Expression::FunctionCall(c) = e { Some(c) } else { None }
                    })
                })
            }
            Statement::Assign(a) => {
                a.expression_list().and_then(|el| {
                    el.expressions().into_iter().find_map(|e| {
                        if let Expression::FunctionCall(c) = e { Some(c) } else { None }
                    })
                })
            }
            _ => None,
        };
        if let Some(ref call) = call
            && let Some((name, frame_type)) = extract_createframe_named_global(call)
        {
            // These are all top-level WoW API globals (CreateFrame, CreateFont,
            // CreateFontFamily); no addon_ns / class_vars normalization is needed
            // since the callee is always a plain global name.
            let callee_names = call.identifier()
                .map(|id| id.names())
                .unwrap_or_default();
            let range = call.syntax().text_range();
            globals.push(ExternalGlobal {
                name,
                kind: ExternalGlobalKind::Variable(FieldValueKind::FunctionCall(
                    callee_names,
                    Some(frame_type.clone()),
                )),
                params: Vec::new(),
                returns: vec![AnnotationType::Simple(frame_type)],
                return_names: Vec::new(),
                return_descriptions: Vec::new(),
                overloads: Vec::new(),
                doc: None,
                deprecated: false,
                nodiscard: false,
                constructor: false,
                visibility: Visibility::Public,
                generics: Vec::new(),
                defclass: None,
                defclass_parent: None,
                source_path: owned_path.clone(),
                def_start: u32::from(range.start()),
                def_end: u32::from(range.end()),
                builds_field: None,
                built_name: None,
                built_extends: false,
                type_narrows: None,
                type_narrows_class: None,
                string_value: None,
                number_value: None,
                is_override: false,
                see: Vec::new(),
                flavors: 0,
                flavor_guard: 0,
                implicit_nil_return: false,
                narrows_arg: None,
                requires: Vec::new(),
                body_derived_returns: false,
            });
        }
    }

    // Track field names assigned on the addon table in this file (e.g. ns.LibTSMApp = ...)
    // Used to gate 3-part chains so we don't inject fields onto unrelated external classes
    let mut addon_assigned_fields: HashSet<String> = HashSet::new();
    // Buffer methods defined on local tables (e.g. function Locale.GetTable())
    // so they can be emitted when the local table is assigned to the addon ns
    let mut local_table_methods: HashMap<String, Vec<ExternalGlobal>> = HashMap::new();
    // Map local table var name → addon field name (e.g. "Locale" → "Locale" from ns.Locale = Locale)
    let mut local_table_to_addon_field: HashMap<String, String> = HashMap::new();

    for stmt in &all_stmts {
        match stmt {
            Statement::FunctionDefinition(func) => {
                // Parser2 emits simple function names as bare Name tokens (no Identifier node).
                // Fall back to func.name() when identifier() returns None.
                let (mut names, is_colon_opt) = if let Some(ident) = func.identifier() {
                    (ident.names(), Some(ident.is_call_to_self()))
                } else if let Some(name) = func.name() {
                    (vec![name], Some(false))
                } else {
                    continue;
                };
                // Redirect _G.func to a top-level global (matches build_ir.rs behavior)
                if names.len() >= 2 && names[0] == "_G" && !local_vars.contains(&names[0]) {
                    names.remove(0);
                }
                let is_colon = is_colon_opt.unwrap_or(false);
                {
                    let base = build_func_external(
                        func, func.syntax(), is_colon, correlated_return_overloads,
                        &file_func_returns, &file_func_tail_callee, owned_path.as_deref(),
                    );
                    // Local functions are file-scoped, not cross-file globals
                    // (multi-name branch needs no check — Lua syntax forbids `local function a.b()`)
                    if names.len() == 1 && !func.is_local() {
                        globals.push(ExternalGlobal { name: names[0].clone(), ..base });
                    } else if names.len() >= 2 {
                        let root_name = &names[0];
                        let method_name = &names[names.len() - 1];
                        let intermediates: Vec<String> = names[1..names.len()-1].to_vec();
                        // Skip methods on locals that aren't class-typed or table constructors
                        if local_vars.contains(root_name) && !class_vars.contains_key(root_name) && !local_tables.contains(root_name) && addon_ns_var.as_deref() != Some(root_name.as_str()) {
                            continue;
                        }
                        // Buffer methods defined on local tables (any depth) for later
                        // flushing onto the addon namespace. At flush time the local name
                        // is rewritten to the addon field alias and prepended to the
                        // buffered intermediates, so `function Db.A:Foo()` + `ns.Db = Db`
                        // resolves as `ns.Db.A:Foo()`.
                        if local_tables.contains(root_name) && !class_vars.contains_key(root_name) && addon_ns_var.as_deref() != Some(root_name.as_str()) {
                            local_table_methods.entry(root_name.clone()).or_default().push(ExternalGlobal {
                                // name left empty; set when flushed onto the addon ns
                                kind: ExternalGlobalKind::Method(intermediates.clone(), method_name.clone(), is_colon),
                                ..base.clone()
                            });
                        } else {
                            let canonical_name = if addon_ns_var.as_deref() == Some(root_name.as_str()) {
                                ADDON_NS_NAME.to_string()
                            } else if let Some(class_name) = class_vars.get(root_name) {
                                class_name.clone()
                            } else { root_name.clone() };
                            globals.push(ExternalGlobal {
                                name: canonical_name,
                                kind: ExternalGlobalKind::Method(intermediates, method_name.clone(), is_colon),
                                ..base
                            });
                        }
                    }
                }
            }
            Statement::Assign(assign) => {
                if let (Some(var_list), Some(expr_list)) = (assign.variable_list(), assign.expression_list()) {
                    let idents = var_list.identifiers();
                    let exprs = expr_list.expressions();
                    if idents.len() == 1 && exprs.len() == 1 {
                        let mut names = idents[0].names();
                        // Redirect _G.field to a top-level global (matches build_ir.rs behavior)
                        if names.len() >= 2 && names[0] == "_G" && !local_vars.contains(&names[0]) {
                            names.remove(0);
                        }
                        if names.len() == 1 {
                            let range = assign.syntax().text_range();
                            let effective = unwrap_logical_chain(exprs[0]);
                            let (kind, string_value, number_value) = if let Some(vk) = classify_literal_value_kind(&effective) {
                                let sv = if let Expression::Literal(lit) = &effective {
                                    lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                } else { None };
                                let nv = super::annotation_scanning::extract_number_from_expr(&effective);
                                (ExternalGlobalKind::Variable(vk), sv, nv)
                            } else { match &effective {
                                Expression::TableConstructor(_) => (ExternalGlobalKind::Table, None, None),
                                Expression::Function(_) => (ExternalGlobalKind::Variable(FieldValueKind::Function), None, None),
                                Expression::Identifier(ident) => {
                                    let mut rhs_names = ident.names();
                                    if rhs_names.len() == 2 {
                                        let table_name = local_aliases.get(&rhs_names[0])
                                            .cloned().unwrap_or_else(|| rhs_names[0].clone());
                                        (ExternalGlobalKind::FieldRef(table_name, rhs_names[1].clone()), None, None)
                                    } else if rhs_names.len() >= 2 {
                                        // Multi-part reference (e.g. Enum.BagIndex.Backpack)
                                        if addon_ns_var.as_deref() == Some(rhs_names[0].as_str()) {
                                            rhs_names[0] = ADDON_NS_NAME.to_string();
                                        } else if let Some(cn) = class_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = cn.clone();
                                        } else if let Some(type_name) = local_type_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = type_name.clone();
                                        }
                                        (ExternalGlobalKind::Variable(FieldValueKind::FieldRef(rhs_names)), None, None)
                                    } else if rhs_names.len() == 1 {
                                        (ExternalGlobalKind::Variable(FieldValueKind::FieldRef(rhs_names)), None, None)
                                    } else {
                                        (ExternalGlobalKind::Variable(FieldValueKind::Unknown), None, None)
                                    }
                                }
                                Expression::FunctionCall(call) => {
                                    if let Some(call_ident) = call.identifier() {
                                        let mut callee_names = call_ident.names();
                                        if !callee_names.is_empty() {
                                            if addon_ns_var.as_deref() == Some(callee_names[0].as_str()) {
                                                callee_names[0] = ADDON_NS_NAME.to_string();
                                            } else if let Some(class_name) = class_vars.get(&callee_names[0]) {
                                                callee_names[0] = class_name.clone();
                                            } else if let Some(type_name) = local_type_vars.get(&callee_names[0]) {
                                                callee_names[0] = type_name.clone();
                                            }
                                        }
                                        let first_string_arg = call.arguments().and_then(|al| {
                                            let args = al.expressions();
                                            if let Some(Expression::Literal(lit)) = args.first() {
                                                lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                            } else {
                                                None
                                            }
                                        });
                                        (ExternalGlobalKind::Variable(FieldValueKind::FunctionCall(callee_names, first_string_arg)), None, None)
                                    } else {
                                        (ExternalGlobalKind::Variable(FieldValueKind::Unknown), None, None)
                                    }
                                }
                                Expression::BinaryExpression(bin) => {
                                    let vk = match bin.kind() {
                                        Operator::Concatenate => FieldValueKind::String(None),
                                        op if op.is_arithmetic() => FieldValueKind::Number(None),
                                        op if op.is_comparison() => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    };
                                    (ExternalGlobalKind::Variable(vk), None, None)
                                }
                                Expression::UnaryExpression(un) => {
                                    let vk = match un.kind() {
                                        Operator::ArrayLength | Operator::Subtract => FieldValueKind::Number(None),
                                        Operator::Not => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    };
                                    (ExternalGlobalKind::Variable(vk), None, None)
                                }
                                _ => (ExternalGlobalKind::Variable(FieldValueKind::Unknown), None, None),
                            }};
                            // Extract @type or @class annotation for the variable
                            let annotations = extract_annotations(assign.syntax());
                            let returns: Vec<AnnotationType> = if let Some(class_name) = class_vars.get(&names[0]) {
                                vec![AnnotationType::Simple(class_name.clone())]
                            } else {
                                annotations.var_type.into_iter().collect()
                            };
                            globals.push(ExternalGlobal {
                                name: names[0].clone(), kind,
                                params: Vec::new(), returns, return_names: Vec::new(), return_descriptions: Vec::new(), overloads: Vec::new(),
                                doc: None, deprecated: false, nodiscard: false, constructor: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                                string_value, number_value,
                                is_override: false,
                                see: Vec::new(),
                                flavors: 0, flavor_guard: annotations.flavor_guard,
                                implicit_nil_return: false,
                                narrows_arg: None,
                                requires: Vec::new(),
                                body_derived_returns: false,
                            });
                        } else if names.len() >= 2 {
                            // Skip bracket-element writes (e.g. `ns.field[123] = true`):
                            // these write to an element OF the table, not to the field itself.
                            if idents[0].has_non_string_bracket_tail() { continue; }
                            let root_name = &names[0];
                            let is_addon_root = addon_ns_var.as_deref() == Some(root_name.as_str());
                            // Skip field assignments on locals that aren't class-typed, table constructors, or @type-annotated
                            if local_vars.contains(root_name) && !class_vars.contains_key(root_name) && !local_tables.contains(root_name) && !local_type_vars.contains_key(root_name) && !is_addon_root {
                                continue;
                            }
                            // Drop 3+ part chains for local non-class variables to avoid
                            // fabricating sub-tables on e.g. local Frame instances.
                            // Allow: addon-ns roots, local @class vars, and non-local
                            // (global) roots — build_on_stubs/mod.rs decides whether to
                            // create sub-tables based on class vs non-class status via
                            // `is_deep_class_global()`. This trades some extra entries
                            // (global class roots emit deep chains that are later skipped)
                            // for simpler filter logic here — the downstream guard is the
                            // single source of truth for class-global deep-path suppression.
                            if names.len() >= 3 && !is_addon_root && local_vars.contains(root_name) && !class_vars.contains_key(root_name) { continue; }
                            let intermediates: Vec<String> = names[1..names.len()-1].to_vec();
                            let field_name = names[names.len()-1].clone();
                            let canonical_name = if is_addon_root {
                                ADDON_NS_NAME.to_string()
                            } else if let Some(class_name) = class_vars.get(root_name) {
                                class_name.clone()
                            } else if let Some(type_name) = local_type_vars.get(root_name) {
                                type_name.clone()
                            } else { root_name.clone() };
                            let annotations = extract_annotations(assign.syntax());
                            // Unwrap `and`/`or` chains so `ns.B = X and X.Y`
                            // infers from the effective operand (Y), not the whole chain.
                            let effective = unwrap_logical_chain(exprs[0]);
                            let value_kind = if let Some(vk) = classify_literal_value_kind(&effective) {
                                vk
                            } else { match &effective {
                                Expression::TableConstructor(tc) => FieldValueKind::Table(extract_table_field_kinds(tc)),
                                Expression::Function(_) => FieldValueKind::Function,
                                Expression::FunctionCall(call) => {
                                    if let Some(ident) = call.identifier() {
                                        let mut callee_names = ident.names();
                                        // Canonicalize root of callee chain
                                        if !callee_names.is_empty() {
                                            if addon_ns_var.as_deref() == Some(callee_names[0].as_str()) {
                                                callee_names[0] = ADDON_NS_NAME.to_string();
                                            } else if let Some(class_name) = class_vars.get(&callee_names[0]) {
                                                callee_names[0] = class_name.clone();
                                            } else if let Some(type_name) = local_type_vars.get(&callee_names[0]) {
                                                callee_names[0] = type_name.clone();
                                            }
                                        }
                                        // Extract first string literal argument (for defclass resolution)
                                        // For method chains like a.b("x"):c("y"), use the innermost
                                        // call's arg ("x") instead of the outermost ("y")
                                        let first_string_arg = {
                                            let innermost_args = ident.syntax().descendants()
                                                .filter(|n| n.kind() == SyntaxKind::FunctionCall)
                                                .last()
                                                .and_then(crate::ast::FunctionCall::cast)
                                                .and_then(|fc| fc.arguments());
                                            let arg_list = innermost_args.or_else(|| call.arguments());
                                            arg_list.and_then(|al| {
                                                let args = al.expressions();
                                                if let Some(Expression::Literal(lit)) = args.first() {
                                                    lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                                } else {
                                                    None
                                                }
                                            })
                                        };
                                        FieldValueKind::FunctionCall(callee_names, first_string_arg)
                                    } else {
                                        FieldValueKind::Unknown
                                    }
                                }
                                Expression::Identifier(ident) => {
                                    let mut rhs_names = ident.names();
                                    if rhs_names.len() == 1 && local_functions.contains(&rhs_names[0]) {
                                        FieldValueKind::Function
                                    } else if rhs_names.len() == 1 && local_tables.contains(&rhs_names[0]) {
                                        FieldValueKind::Table(vec![])
                                    } else if rhs_names.len() == 1 {
                                        if let Some((callee_chain, first_string_arg)) = local_call_origins.get(&rhs_names[0]) {
                                            // Local was assigned from a function call whose return type
                                            // isn't known locally — forward the call origin so
                                            // build_on_stubs can resolve the return type cross-file.
                                            FieldValueKind::FunctionCall(callee_chain.clone(), first_string_arg.clone())
                                        } else {
                                            // Single-name reference (e.g. a local or global);
                                            // preserve so cross-file resolution can look it up
                                            // in scope0 symbols / stubs.
                                            FieldValueKind::FieldRef(rhs_names)
                                        }
                                    } else if rhs_names.len() >= 2 {
                                        // Canonicalize root for field references (e.g. Util.FRAME → Banking.Util.FRAME)
                                        if addon_ns_var.as_deref() == Some(rhs_names[0].as_str()) {
                                            rhs_names[0] = ADDON_NS_NAME.to_string();
                                        } else if let Some(cn) = class_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = cn.clone();
                                        } else if let Some(type_name) = local_type_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = type_name.clone();
                                        }
                                        FieldValueKind::FieldRef(rhs_names)
                                    } else {
                                        FieldValueKind::Unknown
                                    }
                                }
                                Expression::BinaryExpression(bin) => {
                                    match bin.kind() {
                                        Operator::Concatenate => FieldValueKind::String(None),
                                        op if op.is_arithmetic() => FieldValueKind::Number(None),
                                        op if op.is_comparison() => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    }
                                }
                                Expression::UnaryExpression(un) => {
                                    match un.kind() {
                                        Operator::ArrayLength | Operator::Subtract => FieldValueKind::Number(None),
                                        Operator::Not => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    }
                                }
                                _ => FieldValueKind::Unknown,
                            }};
                            let returns = if let Some(ref var_type) = annotations.var_type {
                                vec![var_type.clone()]
                            } else if let Some(ref class_name) = annotations.class {
                                // @enum Name or @class Name above a field assignment
                                vec![AnnotationType::Simple(class_name.clone())]
                            } else if let Expression::TableConstructor(tc) = &effective {
                                extract_table_literal_annotation(tc)
                                    .map_or_else(Vec::new, |tl| vec![tl])
                            } else if let Expression::Identifier(ident) = &effective {
                                let rhs_names = ident.names();
                                if rhs_names.len() == 1 {
                                    if let Some(class_name) = class_vars.get(&rhs_names[0]) {
                                        vec![AnnotationType::Simple(class_name.clone())]
                                    } else if let Some(type_name) = local_type_vars.get(&rhs_names[0]) {
                                        vec![AnnotationType::Simple(type_name.clone())]
                                    } else if let Some(ret_type) = local_return_types.get(&rhs_names[0]) {
                                        vec![ret_type.clone()]
                                    } else { Vec::new() }
                                } else { Vec::new() }
                            } else { Vec::new() };
                            // When the RHS is a single local function with a captured
                            // signature, emit a method on the namespace/class so the full
                            // signature (params/returns/etc.) is preserved cross-file,
                            // matching `function ns.field(...)`. Otherwise the field would
                            // degrade to a bare `function` type.
                            if let Expression::Identifier(rhs_ident) = &effective {
                                let rhs_names = rhs_ident.names();
                                if rhs_names.len() == 1
                                    && let Some(base) = local_function_sigs.get(&rhs_names[0]) {
                                        // Merge assignment-level annotations (doc comment,
                                        // @flavor guard) that override the function-level
                                        // ones, so `--- @flavor retail\nns.f = impl` works.
                                        let mut merged = base.clone();
                                        if annotations.doc.is_some() { merged.doc = annotations.doc; }
                                        if annotations.flavor_guard != 0 { merged.flavor_guard = annotations.flavor_guard; }
                                        if annotations.deprecated { merged.deprecated = true; }
                                        let track = if is_addon_root && names.len() == 2 {
                                            Some(&mut addon_assigned_fields)
                                        } else { None };
                                        push_method_global(
                                            &mut globals, canonical_name, merged,
                                            &field_name, intermediates, implicit_protected_prefix, track,
                                        );
                                        continue;
                                    }
                            }
                            // When the RHS is a function literal, build its full signature
                            // directly so params/returns survive cross-file (same as when a
                            // local function is assigned by name above).
                            if let Expression::Function(func_lit) = &effective {
                                let mut base = build_func_external(
                                    func_lit, assign.syntax(), false, correlated_return_overloads,
                                    &file_func_returns, &file_func_tail_callee, owned_path.as_deref(),
                                );
                                // Post-process: when body-derived returns resolved to
                                // [any] but the body actually returns a local function
                                // identifier, replace with the proper fun(...) type.
                                if base.returns.len() == 1
                                    && matches!(&base.returns[0], AnnotationType::Simple(s) if s == "any")
                                    && let Some(body) = func_lit.block()
                                    && let Some(ret_func_type) = resolve_returned_local_func_type(&body, &local_function_sigs)
                                {
                                    base.returns = vec![ret_func_type];
                                }
                                let track = if is_addon_root && names.len() == 2 {
                                    Some(&mut addon_assigned_fields)
                                } else { None };
                                push_method_global(
                                    &mut globals, canonical_name, base,
                                    &field_name, intermediates, implicit_protected_prefix, track,
                                );
                                continue;
                            }
                            let range = assign.syntax().text_range();
                            globals.push(ExternalGlobal {
                                name: canonical_name,
                                kind: ExternalGlobalKind::TableField(intermediates, field_name.clone(), value_kind),
                                params: Vec::new(), returns, return_names: Vec::new(), return_descriptions: Vec::new(), overloads: Vec::new(),
                                doc: annotations.doc, deprecated: false, nodiscard: false, constructor: false,
                                visibility: default_visibility_for_name(&field_name, implicit_protected_prefix), generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                                string_value: None, number_value: None,
                                is_override: false,
                                see: Vec::new(),
                                flavors: 0, flavor_guard: annotations.flavor_guard,
                                implicit_nil_return: false,
                                narrows_arg: None,
                                requires: Vec::new(),
                                body_derived_returns: false,
                            });
                            // For depth-2 assignments on the addon ns, track the assigned field
                            // name so methods on buffered local tables can be flushed post-loop.
                            if is_addon_root && names.len() == 2 {
                                addon_assigned_fields.insert(field_name.clone());
                                if let Expression::Identifier(rhs_ident) = &effective {
                                    let rhs_names = rhs_ident.names();
                                    if rhs_names.len() == 1 && local_tables.contains(&rhs_names[0]) {
                                        local_table_to_addon_field.insert(rhs_names[0].clone(), field_name.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Flush buffered local table methods onto their addon namespace sub-tables.
    // The local root name is rewritten to the addon field alias and prepended to
    // the buffered intermediates — e.g. `function Db.A.B:Foo()` buffered with
    // path=["A","B"], once flushed via `ns.Db = Db`, becomes a Method under the
    // addon ns with path=["Db","A","B"], which walk_deep_path then resolves as
    // `ns.Db.A.B:Foo()` (auto-creating any missing intermediate sub-tables).
    for (local_name, addon_field) in &local_table_to_addon_field {
        if let Some(methods) = local_table_methods.remove(local_name) {
            for mut m in methods {
                m.name = ADDON_NS_NAME.to_string();
                if let ExternalGlobalKind::Method(ref path, ref mname, is_colon) = m.kind {
                    let mut new_path = Vec::with_capacity(path.len() + 1);
                    new_path.push(addon_field.clone());
                    new_path.extend_from_slice(path);
                    m.kind = ExternalGlobalKind::Method(new_path, mname.clone(), is_colon);
                }
                globals.push(m);
            }
        }
    }

    let addon_ns_class = addon_ns_var.as_ref()
        .and_then(|var| class_vars.get(var))
        .cloned();
    (globals, addon_ns_class)
}

/// Walk a function call chain to find the outermost colon-call with a string literal first argument.
/// For `Y:Init("ClassName")` returns `Some("ClassName")`.
/// For `Y:From("Z"):Include("ClassName")` returns `Some("ClassName")` (outermost call's arg).
/// Only matches colon-calls (`:Method(...)`) to avoid false positives on dot-calls like `Enum.New(...)`.
/// Returns None if no matching call is found.
fn extract_string_arg_from_call_chain(call: &FunctionCall<'_>) -> Option<String> {
    // Check if this call uses colon syntax (method call)
    let ident = call.identifier()?;
    let is_colon = ident.is_call_to_self();
    if is_colon
        && let Some(arg_list) = call.arguments() {
            let args = arg_list.expressions();
            if let Some(Expression::Literal(lit)) = args.first()
                && let Some(s) = lit.get_string() {
                    let name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
        }
    // Check nested call in the identifier (for method chains)
    let nested = ident.syntax().children().find_map(FunctionCall::cast)?;
    extract_string_arg_from_call_chain(&nested)
}

/// Extract the first string literal argument from a plain (non-colon) function call.
/// For `DefineClass("MyComp")` returns `Some("MyComp")`.
/// Used alongside `extract_string_arg_from_call_chain` to populate `class_vars`
/// for locals assigned from factory-style function calls.
fn extract_first_string_arg(call: &FunctionCall<'_>) -> Option<String> {
    let arg_list = call.arguments()?;
    let args = arg_list.expressions();
    if let Some(Expression::Literal(lit)) = args.first()
        && let Some(s) = lit.get_string() {
            let name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    None
}

/// Extract a non-empty string literal value from an expression, stripping quotes.
fn string_literal_value(expr: &Expression<'_>) -> Option<String> {
    if let Expression::Literal(lit) = expr {
        let s = lit.get_string()?.trim_matches(|c| c == '"' || c == '\'').to_string();
        if !s.is_empty() { return Some(s); }
    }
    None
}

/// Check if a function call is `CreateFrame(type, "name", ...)`,
/// `CreateFont("name")`, or `CreateFontFamily("name", ...)` with
/// string-literal name arguments.
/// Returns `(global_name, resolved_type)` if matched.
fn extract_createframe_named_global(call: &FunctionCall<'_>) -> Option<(String, String)> {
    let ident = call.identifier()?;
    if ident.is_call_to_self() { return None; }
    let names = ident.names();
    if names.len() != 1 { return None; }
    let args = call.arguments()?.expressions();

    match names[0].as_str() {
        "CreateFrame" => {
            // CreateFrame(frameType, "name", ...)
            if args.len() < 2 { return None; }
            let frame_type = string_literal_value(&args[0])?;
            let name = string_literal_value(&args[1])?;
            Some((name, frame_type))
        }
        "CreateFont" | "CreateFontFamily" => {
            // CreateFont("name") / CreateFontFamily("name", ...)
            if args.is_empty() { return None; }
            let name = string_literal_value(&args[0])?;
            Some((name, "Font".to_string()))
        }
        _ => None,
    }
}

/// Extract named fields from a table constructor as a `TableLiteral` annotation.
/// Returns `None` if the table has no named fields.  Used by the global scanner
/// and bare self-field scanner to preserve table shape across files.
/// Positional and bracket-keyed entries are intentionally skipped — only
/// `Name = expr` fields map to `TableLiteral` field entries.
pub(crate) fn extract_table_literal_annotation(tc: &crate::ast::TableConstructor<'_>) -> Option<AnnotationType> {
    let mut fields = Vec::new();
    for field in tc.fields() {
        if let Some(crate::ast::FieldKind::Named { name, value }) = field.kind() {
            // Check for per-field ---@type annotation first (preceding-line or trailing)
            let field_type = if let Some(at) = super::annotation_scanning::extract_inline_type_from_node(field.syntax()) {
                at
            } else {
                match &value {
                    Expression::Literal(lit) => {
                        if lit.get_string().is_some() { AnnotationType::Simple("string".into()) }
                        else if lit.get_number().is_some() { AnnotationType::Simple("number".into()) }
                        else if lit.get_bool().is_some() { AnnotationType::Simple("boolean".into()) }
                        else if lit.is_nil() { AnnotationType::Simple("nil".into()) }
                        else { AnnotationType::Simple("any".into()) }
                    }
                    Expression::UnaryExpression(u) if matches!(u.kind(), crate::ast::Operator::Subtract) => {
                        if super::annotation_scanning::extract_number_from_expr(&value).is_some() {
                            AnnotationType::Simple("number".into())
                        } else {
                            AnnotationType::Simple("any".into())
                        }
                    }
                    Expression::TableConstructor(nested) => {
                        extract_table_literal_annotation(nested)
                            .unwrap_or_else(|| AnnotationType::Simple("table".into()))
                    }
                    Expression::Function(_) => AnnotationType::Simple("function".into()),
                    _ => AnnotationType::Simple("any".into()),
                }
            };
            fields.push((name, field_type));
        }
    }
    if fields.is_empty() { None } else { Some(AnnotationType::TableLiteral(fields)) }
}
