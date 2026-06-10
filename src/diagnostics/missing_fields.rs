use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct MissingFields;

/// Whether a class field must be present in a constructor (non-nullable, non-function).
fn is_required_field(fi: &FieldInfo) -> bool {
    if fi.lateinit { return false; }
    let Some(ann) = &fi.annotation else { return false };
    let is_nullable = match ann {
        ValueType::Nil => true,
        ValueType::Union(types) => types.contains(&ValueType::Nil),
        _ => false,
    };
    if is_nullable { return false; }
    if matches!(ann, ValueType::Function(_)) { return false; }
    true
}

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
        if !is_required_field(fi) { continue; }
        if !rhs_table.fields.contains_key(field_name.as_str()) {
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

/// Emits a diagnostic only when no union member is fully satisfied by the constructor.
/// For single-class expectations, delegates directly to `check_missing_fields`.
/// For multi-class unions, reports against the best-matching member (fewest missing fields).
fn check_missing_fields_union(
    analysis: &AnalysisResult,
    ctor_idx: TableIndex,
    class_indices: &[TableIndex],
    diags: &mut Vec<WowDiagnostic>,
) {
    if class_indices.len() <= 1 {
        // Single class — use the original path
        if let Some(&class_idx) = class_indices.first() {
            check_missing_fields(analysis, ctor_idx, class_idx, diags);
        }
        return;
    }

    let rhs_table = analysis.ir.table(ctor_idx);
    if rhs_table.fields.is_empty() { return; }

    // Check each union member: if any member is fully satisfied, no diagnostic
    for &class_idx in class_indices {
        let class_table = analysis.table(class_idx);
        if class_table.class_name.is_none() { continue; }

        let has_missing = class_table.fields.iter().any(|(field_name, fi)| {
            is_required_field(fi) && !rhs_table.fields.contains_key(field_name.as_str())
        });

        if !has_missing {
            // Constructor satisfies this union member — no diagnostic
            return;
        }
    }

    // No union member is fully satisfied. Report against the member with
    // the fewest missing fields to give the most helpful message.
    let best = class_indices.iter().copied()
        .filter(|&idx| analysis.table(idx).class_name.is_some())
        .min_by_key(|&class_idx| {
            let class_table = analysis.table(class_idx);
            class_table.fields.iter().filter(|(field_name, fi)| {
                is_required_field(fi) && !rhs_table.fields.contains_key(field_name.as_str())
            }).count()
        });

    if let Some(best_idx) = best {
        check_missing_fields(analysis, ctor_idx, best_idx, diags);
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
        for (&ctor_idx, class_indices) in &analysis.ir.tc_expected_class {
            if ctor_idx.is_external() { continue; }
            check_missing_fields_union(analysis, ctor_idx, class_indices, diags);
        }
    }
}
