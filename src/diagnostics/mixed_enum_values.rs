use crate::analysis::AnalysisResult;
use crate::types::{EnumFieldClassification, EXT_BASE};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct MixedEnumValues;

impl DiagnosticPass for MixedEnumValues {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (idx, table) in analysis.ir.tables.iter().enumerate() {
            if !table.enum_kind.is_enum() { continue; }
            if idx >= EXT_BASE { continue; }
            let Some(class_name) = &table.class_name else { continue };
            let Some(&(start, end)) = analysis.ir.class_def_ranges.get(class_name) else { continue };
            if table.fields.is_empty() { continue; }

            let resolved: Vec<_> = table.fields.values()
                .map(|f| analysis.resolve_expr_type(f.expr))
                .collect();
            let classification = EnumFieldClassification::from_types(
                resolved.iter().map(|v| v.as_ref())
            );

            if classification.has_other {
                super::MIXED_ENUM_VALUES.emit(
                    diags,
                    format!("enum '{}' has non-number, non-string values; enum values must be numbers or strings", class_name),
                    start as usize,
                    end as usize,
                );
            } else if classification.has_number && classification.has_string {
                super::MIXED_ENUM_VALUES.emit(
                    diags,
                    format!("enum '{}' has mixed value types; all values must be numbers or all strings", class_name),
                    start as usize,
                    end as usize,
                );
            }
        }
    }
}
