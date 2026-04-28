use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::types::*;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "missing-fields";

pub(crate) fn run(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    for sym in &analysis.ir.symbols {
        let ver = &sym.versions[0];
        let Some(original_expr) = ver.original_type_source else { continue };
        let Some(type_source) = ver.type_source else { continue };

        let Expr::Literal(ValueType::Table(Some(class_table_idx))) = analysis.ir.expr(type_source) else { continue };
        let class_table = analysis.table(*class_table_idx);
        let Some(class_name) = &class_table.class_name else { continue };

        let Some(rhs_table_idx) = analysis.ir.find_table_index(original_expr) else { continue };
        let rhs_table = analysis.ir.table(rhs_table_idx);
        if rhs_table.fields.is_empty() { continue; }

        let Some(&(start, end)) = analysis.ir.table_ranges.iter()
            .find(|(_, idx)| **idx == rhs_table_idx)
            .map(|(range, _)| range) else { continue };

        let mut missing: Vec<&str> = Vec::new();
        for (field_name, fi) in &class_table.fields {
            let Some(ann) = &fi.annotation else { continue };
            let is_nullable = match ann {
                ValueType::Nil => true,
                ValueType::Union(types) => types.contains(&ValueType::Nil),
                _ => false,
            };
            if is_nullable { continue; }
            if matches!(ann, ValueType::Function(_)) { continue; }
            if !rhs_table.fields.contains_key(field_name) {
                missing.push(field_name);
            }
        }
        if !missing.is_empty() {
            missing.sort();
            let fields_str = missing.join("', '");
            let message = if missing.len() == 1 {
                format!("missing required field '{}' in class '{}'", fields_str, class_name)
            } else {
                format!("missing required fields '{}' in class '{}'", fields_str, class_name)
            };
            diags.push(WowDiagnostic {
                code: CODE,
                message,
                severity: DiagnosticSeverity::WARNING,
                start: start as usize,
                end: end as usize,
            });
        }
    }
}
