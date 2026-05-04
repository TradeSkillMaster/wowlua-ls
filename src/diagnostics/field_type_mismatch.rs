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
                let Some(class_field) = class_table.fields.get(field_name) else { continue };
                let Some(ref expected) = class_field.annotation else { continue };

                let Some(actual) = analysis.resolve_expr_type(rhs_field.expr) else { continue };

                // Nil is a valid placeholder in constructors
                if matches!(actual, ValueType::Nil) { continue; }
                // Inline ---@type annotation on the field takes precedence
                if rhs_field.annotation.is_some() { continue; }

                if class_field.lateinit {
                    let stripped = actual.strip_nil();
                    if stripped.is_assignable_to(expected) { continue; }
                    if analysis.is_table_subtype(&stripped, expected) { continue; }
                }

                if actual.is_assignable_to(expected) {
                    continue;
                }

                let Some((start, end)) = rhs_field.def_range.or_else(|| {
                    analysis.ir.table_ranges.iter()
                        .find(|(_, idx)| **idx == rhs_table_idx)
                        .map(|(&(s, e), _)| (s, e))
                }) else { continue };

                if analysis.is_table_subtype(&actual, expected) {
                    analysis.check_excess_structural_fields(
                        excess_inject, &actual, expected, start as usize, end as usize,
                    );
                    continue;
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
        }
    }
}
