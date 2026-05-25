use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct MissingFields;

fn check_missing_fields(
    analysis: &AnalysisResult,
    ctor_idx: TableIndex,
    class_idx: TableIndex,
    diags: &mut Vec<WowDiagnostic>,
) {
    let rhs_table = analysis.ir.table(ctor_idx);
    if rhs_table.fields.is_empty() { return; }

    let class_table = analysis.table(class_idx);
    let Some(class_name) = &class_table.class_name else { return };

    let Some(&(start, end)) = analysis.ir.table_ranges.iter()
        .find(|(_, idx)| **idx == ctor_idx)
        .map(|(range, _)| range) else { return };

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
        super::MISSING_FIELDS.emit(diags, message, start as usize, end as usize);
    }
}

impl DiagnosticPass for MissingFields {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        // Pass 1: Symbols with @class type annotation assigned a table constructor
        for sym in &analysis.ir.symbols {
            let ver = &sym.versions[0];
            let Some(original_expr) = ver.original_type_source else { continue };
            let Some(type_source) = ver.type_source else { continue };

            let Expr::Literal(ValueType::Table(Some(class_table_idx))) = analysis.ir.expr(type_source) else { continue };

            let Some(rhs_table_idx) = analysis.ir.find_table_index(original_expr) else { continue };
            // Skip constructors already covered by tc_expected_class (Pass 2)
            if analysis.ir.tc_expected_class.contains_key(&rhs_table_idx) { continue; }

            check_missing_fields(analysis, rhs_table_idx, *class_table_idx, diags);
        }

        // Pass 2: Table constructors with expected class from tc_expected_class
        // (covers nested constructors in table<K,V>, function args, bracket assignments)
        for (&ctor_idx, &class_idx) in &analysis.ir.tc_expected_class {
            if ctor_idx.is_external() { continue; }
            check_missing_fields(analysis, ctor_idx, class_idx, diags);
        }
    }
}
