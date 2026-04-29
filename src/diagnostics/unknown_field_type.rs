use crate::analysis::AnalysisResult;
use crate::types::{ExprId, FieldInfo, TableIndex};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct UnknownFieldType;

impl DiagnosticPass for UnknownFieldType {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }
        let mut pending: Vec<(String, String, ExprId, u32, u32)> = Vec::new();

        for table_idx in 0..analysis.ir.tables.len() {
            let table = analysis.table(TableIndex(table_idx));
            let Some(class_name) = table.class_name.clone() else { continue };
            for (field_name, fi) in &table.fields {
                if fi.annotation_type_raw.is_some() { continue; }
                let Some((start, end)) = fi.def_range else { continue };
                pending.push((field_name.clone(), class_name.clone(), fi.expr, start, end));
            }
        }

        // Overlay fields (runtime assignments onto external @class tables).
        // Clone each FieldInfo because the resolve_expr_type call below reads
        // `&self`, so we can't hold a borrow into `ir.overlay_fields`
        // across it.
        let overlay_tables: Vec<TableIndex> = analysis.ir.overlay_fields.keys().copied().collect();
        for table_idx in overlay_tables {
            let Some(class_name) = analysis.table(table_idx).class_name.clone() else { continue };
            let fields: Vec<(String, FieldInfo)> = analysis.ir.overlay_fields.get(&table_idx)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            for (field_name, fi) in fields {
                if fi.annotation_type_raw.is_some() { continue; }
                let Some((start, end)) = fi.def_range else { continue };
                pending.push((field_name, class_name.clone(), fi.expr, start, end));
            }
        }

        for (field_name, class_name, expr_id, start, end) in pending {
            if analysis.resolve_expr_type(expr_id).is_some() { continue; }
            super::UNKNOWN_FIELD_TYPE.emit(diags, format!("field '{}' on '{}' has an unknown type", field_name, class_name), start as usize, end as usize);
        }
    }
}
