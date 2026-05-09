use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct FieldTypeMismatch;

impl DiagnosticPass for FieldTypeMismatch {
    fn run_inject(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, excess_inject: &mut Vec<InjectFieldCheck>, diags: &mut Vec<WowDiagnostic>) {
        for fa in &analysis.ir.field_assignments {
            if !fa.had_annotation_at_build { continue; }
            let Some(field_info) = analysis.get_field(fa.table_idx, &fa.field_name) else { continue };
            let Some(ref expected) = field_info.annotation else { continue };
            let Some(actual) = analysis.resolve_expr_type(fa.actual_expr) else { continue };
            if fa.lateinit {
                if matches!(actual, ValueType::Nil) { continue; }
                let stripped = actual.strip_nil();
                if stripped.is_assignable_to(expected) { continue; }
                if analysis.is_table_subtype(&stripped, expected) { continue; }
            }
            if actual.is_assignable_to(expected) {
                continue;
            }
            if analysis.is_table_subtype(&actual, expected) {
                analysis.check_excess_structural_fields(excess_inject, &actual, expected, fa.expr_start as usize, fa.expr_end as usize);
                continue;
            }
            let expected_str = analysis.format_value_type_depth(expected, 1);
            let actual_str = analysis.format_value_type_depth(&actual, 1);
            let mut message = format!("expected `{}` for field '{}', got `{}`", expected_str, fa.field_name, actual_str);
            super::append_structural_mismatch_suffix(&mut message, analysis, &actual, expected);
            super::FIELD_TYPE_MISMATCH.emit(
                diags,
                message,
                fa.expr_start as usize,
                fa.expr_end as usize,
            );
        }

        // Check constructor fields against @class @field annotations.
        // Table constructor fields don't create FieldAssignment records, so the
        // loop above misses them. Walk symbols to find class-typed variables with
        // a constructor RHS and compare each constructor field's type against the
        // class's @field annotation.
        // Only checks version 0 (initial assignment). Reassignments like
        // `obj = { x = "wrong" }` on a class-typed variable are not caught.
        for sym in &analysis.ir.symbols {
            let ver = &sym.versions[0];
            let Some(original_expr) = ver.original_type_source else { continue };
            let Some(type_source) = ver.type_source else { continue };

            let Expr::Literal(ValueType::Table(Some(class_table_idx))) = analysis.ir.expr(type_source) else { continue };
            let class_table = analysis.table(*class_table_idx);
            if class_table.class_name.is_none() { continue; }

            let Some(rhs_table_idx) = analysis.ir.find_table_index(original_expr) else { continue };
            let rhs_table = analysis.ir.table(rhs_table_idx);
            if rhs_table.fields.is_empty() { continue; }

            for (field_name, rhs_field) in &rhs_table.fields {
                if rhs_field.annotation.is_some() { continue; }
                let Some(class_field) = class_table.fields.get(field_name) else { continue };
                let Some(ref expected) = class_field.annotation else { continue };
                check_constructor_field(
                    analysis, field_name, rhs_field.expr, rhs_field.def_range,
                    rhs_table_idx, expected, class_field.lateinit,
                    excess_inject, diags,
                );
            }
        }

        // Phase 3: Check array element constructor fields against @type T[]
        // element annotations. When a variable has @type Shape[] (or
        // @type ClassName[]), each positional element in the table constructor
        // is checked field-by-field against the element type's fields.
        // Only checks version 0 (initial assignment), like Phase 2 above.
        for (&sym_idx, annotation_type) in &analysis.ir.symbol_type_annotations {
            let Some(elem_table_idx) = extract_array_element_table(analysis, annotation_type) else { continue };
            if analysis.table(elem_table_idx).fields.is_empty() { continue; }

            let sym = analysis.sym(sym_idx);
            let ver = &sym.versions[0];
            let Some(original_expr) = ver.original_type_source else { continue };
            let Some(rhs_table_idx) = analysis.ir.find_table_index(original_expr) else { continue };
            let rhs_array_fields = &analysis.ir.table(rhs_table_idx).array_fields;
            if rhs_array_fields.is_empty() { continue; }

            // Clone to release the borrow on analysis.ir before the inner loop
            // calls find_table_index/table/get_field/resolve_expr_type.
            let rhs_array_fields = rhs_array_fields.clone();

            for elem_expr_id in &rhs_array_fields {
                let Some(inner_table_idx) = analysis.ir.find_table_index(*elem_expr_id) else { continue };
                // Collect field data upfront to release the borrow on ir.tables
                // before calling analysis methods in check_constructor_field.
                let inner_fields: Vec<_> = analysis.ir.table(inner_table_idx).fields.iter()
                    .filter(|(_, f)| f.annotation.is_none())
                    .map(|(name, f)| (name.clone(), f.expr, f.def_range))
                    .collect();

                for (field_name, field_expr, def_range) in &inner_fields {
                    let Some(expected_field) = analysis.get_field(elem_table_idx, field_name) else { continue };
                    let Some(ref expected) = expected_field.annotation else { continue };
                    check_constructor_field(
                        analysis, field_name, *field_expr, *def_range,
                        inner_table_idx, expected, expected_field.lateinit,
                        excess_inject, diags,
                    );
                }
            }
        }
    }
}

/// Check a single constructor field's actual type against an expected annotation type.
/// Shared by Phase 2 (class constructor fields) and Phase 3 (array element fields).
#[allow(clippy::too_many_arguments)]
fn check_constructor_field(
    analysis: &AnalysisResult,
    field_name: &str,
    field_expr: ExprId,
    def_range: Option<(u32, u32)>,
    fallback_table_idx: TableIndex,
    expected: &ValueType,
    lateinit: bool,
    excess_inject: &mut Vec<InjectFieldCheck>,
    diags: &mut Vec<WowDiagnostic>,
) {
    let Some(actual) = analysis.resolve_expr_type(field_expr) else { return };
    if matches!(actual, ValueType::Nil) { return; }

    if lateinit {
        let stripped = actual.strip_nil();
        if stripped.is_assignable_to(expected) { return; }
        if analysis.is_table_subtype(&stripped, expected) { return; }
    }

    if actual.is_assignable_to(expected) { return; }

    let Some((start, end)) = def_range.or_else(|| {
        analysis.ir.table_ranges.iter()
            .find(|(_, idx)| **idx == fallback_table_idx)
            .map(|(&(s, e), _)| (s, e))
    }) else { return };

    if analysis.is_table_subtype(&actual, expected) {
        analysis.check_excess_structural_fields(
            excess_inject, &actual, expected, start as usize, end as usize,
        );
        return;
    }

    let expected_str = analysis.format_value_type_depth(expected, 1);
    let actual_str = analysis.format_value_type_depth(&actual, 1);
    let mut message = format!("expected `{}` for field '{}', got `{}`", expected_str, field_name, actual_str);
    super::append_structural_mismatch_suffix(&mut message, analysis, &actual, expected);
    super::FIELD_TYPE_MISMATCH.emit(
        diags,
        message,
        start as usize,
        end as usize,
    );
}

/// Extract the element table index from an array annotation type.
/// For `@type T[]`, returns the TableIndex of T.
/// For unions like `@type T[] | nil`, finds the first array member.
fn extract_array_element_table(analysis: &AnalysisResult, vt: &ValueType) -> Option<TableIndex> {
    match vt {
        ValueType::Table(Some(idx)) => {
            let t = analysis.table(*idx);
            if t.value_type_annotated
                && let Some(ValueType::Table(Some(elem_idx))) = &t.value_type {
                    return Some(*elem_idx);
                }
            None
        }
        ValueType::Union(parts) => parts.iter().find_map(|p| extract_array_element_table(analysis, p)),
        _ => None,
    }
}
