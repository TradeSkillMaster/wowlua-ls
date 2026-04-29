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
            super::FIELD_TYPE_MISMATCH.emit(
                diags,
                format!("expected `{}` for field '{}', got `{}`", expected_str, fa.field_name, actual_str),
                fa.expr_start as usize,
                fa.expr_end as usize,
            );
        }
    }
}
