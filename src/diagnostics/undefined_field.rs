use crate::analysis::AnalysisResult;
use crate::types::{Expr, TableIndex, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct UndefinedField;

impl DiagnosticPass for UndefinedField {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for expr in analysis.ir.exprs.iter() {
            let Expr::FieldAccess { table, field, field_range } = expr else { continue };
            let Some((start, end)) = field_range else { continue };
            let Some(table_type) = analysis.resolve_expr_type(*table) else { continue };
            if matches!(table_type, ValueType::Any) { continue; }
            let table_indices: Vec<TableIndex> = match &table_type {
                ValueType::Table(Some(idx)) => vec![*idx],
                ValueType::Union(types) => types.iter().filter_map(|t| match t {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                }).collect(),
                _ => continue,
            };
            if table_indices.is_empty() { continue; }
            // Only emit when at least one table is a @class (matches build-time gate).
            if !table_indices.iter().any(|&idx| analysis.table(idx).class_name.is_some()) { continue; }
            // Direct field?
            if table_indices.iter().any(|&idx| analysis.ir.has_field(idx, field)) { continue; }
            // Inherited field?
            if table_indices.iter().any(|&idx| {
                analysis.table(idx).parent_classes.iter().any(|&pi| analysis.ir.has_field(pi, field))
            }) { continue; }
            let first_idx = table_indices[0];
            let Some(class_name) = analysis.table(first_idx).class_name.clone() else { continue };
            super::UNDEFINED_FIELD.emit(diags, format!("undefined field '{}' on class '{}'", field, class_name), *start as usize, *end as usize);
        }
    }
}
