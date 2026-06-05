use crate::analysis::AnalysisResult;
use crate::ast::Operator;
use crate::types::{Expr, ExprId, ValueType};
use super::{DiagnosticPass, WowDiagnostic, is_type_permissive};

pub(crate) struct RedundantLogical;

/// Unwrap StripNil / StripFalsy / Grouped wrappers to reach the underlying
/// expression. Narrowing (e.g. nil-guard scopes) can wrap the LHS in these,
/// but the suppression checks need to see the original expression.
fn unwrap_to_inner(exprs: &[Expr], mut id: ExprId) -> ExprId {
    loop {
        match &exprs[id.val()] {
            Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::StripTruthy(inner) | Expr::Grouped(inner) => {
                id = *inner;
            }
            _ => return id,
        }
    }
}

/// Returns true when the LHS expression is a field access to a `lateinit` (`T!`)
/// field. Such fields are typed non-nil for the language server (so accessing them
/// doesn't require a nil check), but can be nil at runtime until first initialized.
/// The `x = x or default` idiom is exactly how such fields get initialized, so we
/// must not flag the `or` as redundant.
fn lhs_is_lateinit_field(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner(&analysis.ir.exprs, lhs);
    let Expr::FieldAccess { table, field, .. } = &analysis.ir.exprs[lhs.val()] else { return false };
    let Some(table_type) = analysis.resolve_expr_type(*table) else { return false };
    let table_type = table_type.into_strip_opaque();
    any_table_has_lateinit_field(analysis, &table_type, field)
}

/// Recursively checks whether any table in a (possibly union/intersection) type
/// has a lateinit field with the given name.
fn any_table_has_lateinit_field(analysis: &AnalysisResult, ty: &ValueType, field: &str) -> bool {
    match ty {
        ValueType::Table(Some(idx)) => analysis.get_field(*idx, field).is_some_and(|fi| fi.lateinit),
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| any_table_has_lateinit_field(analysis, t, field))
        }
        _ => false,
    }
}

/// Returns true when the LHS is a bracket index into a dictionary/array
/// (`table<K, V>` or `V[]`) that resolves through the dictionary's `value_type`
/// rather than a known named field. Such a lookup resolves to the element type `V`
/// (typed non-nil for the LS), but a runtime lookup of a missing key — or an
/// out-of-bounds array index — returns nil. The `tbl[k] or default` idiom is the
/// standard way to supply a fallback for an absent key, so the `or` is not
/// redundant.
///
/// If the bracket access has a literal key that matches a declared field on the
/// table (e.g. `cfg["name"]` where `name` is a `@field`), the field is guaranteed
/// to exist, so we do NOT suppress — the `or` really is redundant in that case.
fn lhs_is_dynamic_index(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner(&analysis.ir.exprs, lhs);
    let Expr::BracketIndex { table, literal_key, .. } = &analysis.ir.exprs[lhs.val()] else { return false };
    let literal_key = literal_key.clone();
    let Some(table_type) = analysis.resolve_expr_type(*table) else { return false };
    let table_type = table_type.into_strip_opaque();
    // If the literal key matches a declared field, the access is to a known field
    // rather than a dynamic dictionary lookup — don't suppress.
    if let Some(ref lk) = literal_key
        && any_table_has_named_field(analysis, &table_type, lk) {
            return false;
    }
    any_table_has_value_type(analysis, &table_type)
}

/// Recursively checks whether any table in a (possibly union/intersection) type
/// has a declared field with the given name.
fn any_table_has_named_field(analysis: &AnalysisResult, ty: &ValueType, field: &str) -> bool {
    match ty {
        ValueType::Table(Some(idx)) => analysis.get_field(*idx, field).is_some(),
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| any_table_has_named_field(analysis, t, field))
        }
        _ => false,
    }
}

/// Recursively checks whether any table in a (possibly union/intersection) type
/// is a dictionary/array (has an element `value_type`).
fn any_table_has_value_type(analysis: &AnalysisResult, ty: &ValueType) -> bool {
    match ty {
        ValueType::Table(Some(idx)) => analysis.table(*idx).value_type.is_some(),
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| any_table_has_value_type(analysis, t))
        }
        _ => false,
    }
}

/// Returns true when the LHS is a field access where field existence is not
/// schema-guaranteed. This covers two cases:
/// 1. Bare (non-`@class`) tables: field types are inferred from writes, not
///    declared via `@field`, so the field may be nil at runtime.
/// 2. `@class` tables where the field is NOT directly annotated on the table
///    itself (ignoring parent classes). Even if a parent class declares the
///    field, Lua class inheritance only provides type information — it does not
///    guarantee the field has been initialized on the receiver object.
///
/// In both cases, `x.field = x.field or default` is a standard Lua idiom and
/// the `or` is not redundant.
fn lhs_is_field_without_annotation(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner(&analysis.ir.exprs, lhs);
    let Expr::FieldAccess { table, field, .. } = &analysis.ir.exprs[lhs.val()] else { return false };
    let table_expr = *table;
    let field = field.as_str();
    let Some(table_type) = analysis.resolve_expr_type(table_expr) else { return false };
    let table_type = table_type.into_strip_opaque();
    any_table_lacks_direct_field_annotation(analysis, &table_type, field)
}

/// Recursively checks whether any table in a (possibly union/intersection) type
/// either has no `@class` declaration, or is a `@class` table that does NOT have
/// the given field directly authored (via `@field`) on itself.
///
/// Prescan copies annotated fields from parent classes into child tables, so a
/// child's `fields` HashMap can contain an inherited entry with `annotation.is_some()`.
/// To distinguish direct from inherited, we check whether ANY parent class also
/// has the same annotated field. If so, the child's entry is a copy and the field
/// is not schema-guaranteed on the child itself.
fn any_table_lacks_direct_field_annotation(analysis: &AnalysisResult, ty: &ValueType, field: &str) -> bool {
    match ty {
        ValueType::Table(Some(idx)) => {
            let table = analysis.table(*idx);
            // Bare table (no @class) — fields are never schema-guaranteed.
            if table.class_name.is_none() { return true; }
            // No annotated field at all → suppress.
            let Some(fi) = table.fields.get(field) else { return true; };
            if fi.annotation.is_none() { return true; }
            // The field has an annotation — check if it was inherited from a parent.
            // If any parent also has this annotated field, the child's entry is a
            // prescan copy, not a direct `@field` declaration.
            table.parent_classes.iter().any(|&parent_idx| {
                analysis.table(parent_idx).fields.get(field)
                    .is_some_and(|pfi| pfi.annotation.is_some())
            })
        }
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| any_table_lacks_direct_field_annotation(analysis, t, field))
        }
        _ => false,
    }
}

/// Returns true when the LHS is a symbol reference whose initial definition
/// (version 0) resolved to a type that is not guaranteed truthy. This catches
/// the common `x = x or default` idiom inside loops where `x` starts as nil
/// but the fixpoint resolution makes the merged version appear always truthy
/// after branch merges in the loop body. Only version 0 is checked — if the
/// initial definition is truthy but some intermediate reassignment is nilable,
/// that's a different pattern that doesn't need this suppression.
fn lhs_initial_version_is_nilable(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner(&analysis.ir.exprs, lhs);
    let Expr::SymbolRef(sym_idx, ver_idx) = &analysis.ir.exprs[lhs.val()] else { return false };
    let sym_idx = *sym_idx;
    let ver_idx = *ver_idx;
    if sym_idx.is_external() || ver_idx == 0 { return false; }
    let sym = &analysis.ir.symbols[sym_idx.val()];
    match &sym.versions[0].resolved_type {
        Some(t) if !t.is_guaranteed_truthy() => true,
        None => true,
        _ => false,
    }
}

/// Returns true when the LHS is a bare symbol that has been genuinely
/// reassigned (has a version with a different `def_node` from the initial
/// declaration). This pattern is ubiquitous in Lua for nullable accumulators
/// and conditional initialization in loops, e.g.:
///
///   local x = nil
///   for ... do x = x and f(x) or v end
///   local y = nil; if cond then y = val end; if y and ... then ... end
///
/// The LS processes loop bodies once, so it sees the initial nil version at the
/// `and` site. But on subsequent iterations (or after the conditional branch),
/// the variable may hold a non-nil value.
fn lhs_symbol_has_reassignment(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner(&analysis.ir.exprs, lhs);
    let Expr::SymbolRef(sym_idx, _) = &analysis.ir.exprs[lhs.val()] else { return false };
    let sym_idx = *sym_idx;
    if sym_idx.is_external() { return false; }
    let sym = analysis.sym(sym_idx);
    // Check that at least one version comes from a genuine reassignment (different
    // def_node) rather than from narrowing (which reuses the original def_node).
    let Some(v0) = sym.versions.first() else { return false };
    let v0_start = v0.def_node.start;
    sym.versions.iter().skip(1).any(|v| v.def_node.start != v0_start)
}

/// Returns true when the LHS of an `or` is itself an `and` expression, i.e.
/// `x and y or z`. This is the standard Lua ternary idiom: the developer
/// intends `or z` as the else-branch (fallback when `x` is falsy at runtime).
/// Even if the LS resolves `x` as always truthy — making `x and y` always
/// evaluate to `y` — the `or z` expresses a conditional intent that should not
/// be flagged as redundant.
fn lhs_is_and_expression(exprs: &[Expr], lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner(exprs, lhs);
    matches!(&exprs[lhs.val()], Expr::BinaryOp { op: Operator::And, .. })
}

/// Returns true when the LHS is a reference to a function parameter that has no
/// explicit `@param` annotation. Such parameters get their type from backward
/// inference (call-site propagation), which may resolve to a non-nil type like
/// `Frame` even though the parameter is intended to be optional. The
/// `param = param or default` idiom is the standard Lua way to express default
/// parameter values, so the `or` is not redundant.
fn lhs_is_unannotated_param(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner(&analysis.ir.exprs, lhs);
    let Expr::SymbolRef(sym_idx, _) = &analysis.ir.exprs[lhs.val()] else { return false };
    let sym_idx = *sym_idx;
    if sym_idx.is_external() { return false; }
    for func in &analysis.ir.functions {
        if let Some(pos) = func.args.iter().position(|&s| s == sym_idx) {
            return func.param_annotations.get(pos)
                .is_none_or(|ann| matches!(ann, crate::annotations::AnnotationType::Simple(s) if s.is_empty()));
        }
    }
    false
}

impl DiagnosticPass for RedundantLogical {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for site in &analysis.ir.binary_op_sites {
            let Expr::BinaryOp { op, lhs, .. } = analysis.ir.exprs[site.expr_id.val()] else { continue };

            if !matches!(op, Operator::Or | Operator::And) { continue; }

            let Some(lhs_type) = analysis.resolve_expr_type(lhs) else { continue };

            if is_type_permissive(&lhs_type) { continue; }

            // Skip the Lua ternary idiom `x and y or z`: the `or z` is the
            // else-branch and shouldn't be flagged even if the LS thinks
            // `x and y` is always truthy.
            if matches!(op, Operator::Or) && lhs_is_and_expression(&analysis.ir.exprs, lhs) { continue; }

            // Skip lateinit (`T!`) field accesses: they're non-nil for the LS but
            // get initialized via the `x = x or default` idiom at runtime.
            if matches!(op, Operator::Or) && lhs_is_lateinit_field(analysis, lhs) { continue; }

            // Skip dictionary/array bracket lookups: the element type is non-nil
            // for the LS, but a missing key / out-of-bounds index returns nil at
            // runtime, so `tbl[k] or default` is a legitimate fallback.
            if matches!(op, Operator::Or) && lhs_is_dynamic_index(analysis, lhs) { continue; }

            // Skip unannotated function parameters: their type comes from backward
            // inference which may not account for nil/missing arguments. The
            // `param = param or default` idiom is standard for optional params.
            if matches!(op, Operator::Or) && lhs_is_unannotated_param(analysis, lhs) { continue; }

            // Skip field accesses where the field lacks a direct @field annotation:
            // bare tables (no @class) and @class tables where the field is only
            // inherited or code-discovered. In both cases the field may be nil at
            // runtime, so `x.field = x.field or default` is a legitimate idiom.
            if matches!(op, Operator::Or) && lhs_is_field_without_annotation(analysis, lhs) { continue; }

            // Skip symbols whose initial definition (version 0) was nil/falsy:
            // loop-body branch merges can make a later version appear always
            // truthy, but the variable may still be nil on the first iteration.
            // The `x = x or default` pattern inside a loop is not redundant.
            if matches!(op, Operator::Or) && lhs_initial_version_is_nilable(analysis, lhs) { continue; }

            // Skip nil/false-initialized variables that have been reassigned:
            // the variable may hold a non-nil value after a loop iteration or
            // conditional branch, even though the LS sees the initial falsy
            // version at this expression site. Only `And` needs this guard —
            // `redundant-or` fires on guaranteed-truthy LHS, which doesn't
            // apply to nil/false-initialized variables.
            if matches!(op, Operator::And) && lhs_type.is_guaranteed_falsy()
                && lhs_symbol_has_reassignment(analysis, lhs) { continue; }

            match op {
                Operator::Or if lhs_type.is_guaranteed_truthy() => {
                    let type_str = analysis.format_type_depth(&lhs_type, 1);
                    super::REDUNDANT_OR.emit(
                        diags,
                        format!("`or` is redundant \u{2014} left side is always truthy (`{type_str}`)"),
                        site.op_start as usize,
                        site.op_end as usize,
                    );
                }
                Operator::And if lhs_type.is_guaranteed_falsy() => {
                    let type_str = analysis.format_type_depth(&lhs_type, 1);
                    super::REDUNDANT_AND.emit(
                        diags,
                        format!("`and` is redundant \u{2014} left side is always falsy (`{type_str}`)"),
                        site.op_start as usize,
                        site.op_end as usize,
                    );
                }
                _ => {}
            }
        }
    }
}
