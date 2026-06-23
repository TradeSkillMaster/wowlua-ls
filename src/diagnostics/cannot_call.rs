use crate::analysis::AnalysisResult;
use crate::types::{Expr, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct CannotCall;

fn is_callable(ty: &ValueType, analysis: &AnalysisResult) -> bool {
    match ty {
        ValueType::Function(_) | ValueType::FunctionSig(_) => true,
        ValueType::Any | ValueType::TypeVariable(_) => true,
        ValueType::Table(Some(table_idx)) => {
            is_table_callable(*table_idx, analysis)
        }
        ValueType::Table(None) => false, // bare `table` — not callable
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| is_callable(t, analysis))
        }
        ValueType::OpaqueAlias(_, inner) => is_callable(inner, analysis),
        _ => false,
    }
}

fn is_table_callable(table_idx: crate::types::TableIndex, analysis: &AnalysisResult) -> bool {
    let table = analysis.table(table_idx);
    if table.call_func.is_some() || analysis.resolve_constructor_func(table_idx).is_some() {
        return true;
    }
    // Check raw metatable for __call
    if table.metatable.is_some_and(|mt| analysis.table(mt).fields.contains_key("__call")) {
        return true;
    }
    // For external tables, check the local class overlay which may have call_func
    // set from setmetatable() resolution during per-file analysis.
    if table_idx.is_external()
        && let Some(class_name) = &table.class_name
        && let Some(&local_idx) = analysis.ir.classes.get(class_name.as_str())
        && local_idx != table_idx
    {
        return is_table_callable(local_idx, analysis);
    }
    false
}

impl DiagnosticPass for CannotCall {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (_, expr) in analysis.local_exprs() {
            let Expr::FunctionCall { func: callee, ret_index, call_range, .. } = expr else { continue };
            if *ret_index != 0 { continue; }
            let Some(callee_type) = analysis.resolve_expr_type(*callee) else { continue };
            if is_callable(&callee_type, analysis) { continue; }
            let type_str = analysis.format_type_depth(&callee_type, 1);
            super::CANNOT_CALL.emit(
                diags,
                format!("cannot call a value of type '{type_str}'"),
                call_range.0 as usize,
                call_range.1 as usize,
            );
        }
    }
}
