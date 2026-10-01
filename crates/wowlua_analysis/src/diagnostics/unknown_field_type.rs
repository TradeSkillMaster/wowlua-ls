use crate::analysis::AnalysisResult;
use crate::types::FieldInfo;
use super::unknown_local_type::unknown_type_message;
use super::{DiagnosticPass, WowDiagnostic};

pub struct UnknownFieldType;

impl DiagnosticPass for UnknownFieldType {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }
        let mut pending: Vec<(&str, &str, &FieldInfo)> = Vec::new();

        for (_table_idx, table) in analysis.local_tables() {
            let Some(class_name) = table.class_name.as_deref() else { continue };
            for (field_name, fi) in &table.fields {
                pending.push((field_name, class_name, fi));
            }
        }

        // Overlay fields (runtime assignments onto external @class tables).
        for (&table_idx, fields) in &analysis.ir.overlay_fields {
            let Some(class_name) = analysis.table(table_idx).class_name.as_deref() else { continue };
            for (field_name, fi) in fields {
                pending.push((field_name, class_name, fi));
            }
        }

        for (field_name, class_name, fi) in pending {
            let Some((start, end)) = fi.def_range else { continue };
            // A `---@field` declaration is the field's type; otherwise the assigned value's.
            let annotated = fi.annotation_type_raw.is_some();
            let ty = if annotated { fi.annotation.clone() } else { analysis.resolve_expr_type(fi.expr) };
            if annotated && ty.is_none() { continue; }
            let Some(desc) = unknown_type_message(ty.as_ref(), annotated) else { continue };
            super::UNKNOWN_FIELD_TYPE.emit(
                diags,
                format!("field '{}' on '{}' {}", field_name, class_name, desc),
                start as usize,
                end as usize,
            );
        }
    }
}
