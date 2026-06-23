use std::collections::HashSet;
use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, Expression, Identifier, LocalAssign};
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

pub(crate) struct UndefinedField;

impl DiagnosticPass for UndefinedField {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let pure_records = collect_pure_record_symbols(analysis, tree);
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
