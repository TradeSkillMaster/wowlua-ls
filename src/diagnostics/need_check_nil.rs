use std::collections::HashSet;
use crate::analysis::AnalysisResult;
use crate::types::{Expr, ExprId, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

fn is_nullable(vt: &ValueType) -> bool {
    match vt {
        ValueType::Union(types) => types.contains(&ValueType::Nil),
        ValueType::Nil => true,
        _ => false,
    }
}

fn check_nil_suppressed(analysis: &AnalysisResult, table_expr: ExprId, start: u32) -> bool {
    let Some(scope_idx) = analysis.scope_at_offset(start) else { return true };
    if let Some(sym_idx) = analysis.ir.find_root_symbol(table_expr) {
        if !analysis.is_narrowing_overridden_at(sym_idx, scope_idx, start) && analysis.is_symbol_narrowed(sym_idx, scope_idx) {
            return true;
        }
        if let Some((_, chain)) = analysis.ir.extract_field_chain(table_expr)
            && analysis.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
            return true;
        }
    }
    false
}

pub(crate) fn run_access(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    let mut seen = HashSet::new();
    for (idx, expr) in analysis.ir.exprs.iter().enumerate() {
        let Expr::FieldAccess { table, field_range: Some((start, end)), .. } = expr else { continue };
        let (table, start, end) = (*table, *start, *end);
        if !seen.insert((start, end)) { continue; }
        if analysis.ir.and_guarded_nil_check_exprs.contains(&ExprId(idx)) { continue; }
        let Some(vt) = analysis.resolve_expr_type(table) else { continue };
        if !is_nullable(&vt) { continue; }
        if check_nil_suppressed(analysis, table, start) { continue; }
        let type_str = analysis.format_value_type_depth(&vt, 0);
        check(diags, &type_str, start as usize, end as usize);
    }
    for &(base_expr, start, end) in &analysis.ir.assign_nil_check_bases {
        if !seen.insert((start, end)) { continue; }
        let Some(vt) = analysis.resolve_expr_type(base_expr) else { continue };
        if !is_nullable(&vt) { continue; }
        if check_nil_suppressed(analysis, base_expr, start) { continue; }
        let type_str = analysis.format_value_type_depth(&vt, 0);
        check(diags, &type_str, start as usize, end as usize);
    }
    // Assignment-side entries also land here; their bracket_expr is synthesized and
    // will never appear in and_guarded_nil_check_exprs (assignments aren't expressions).
    for &(bracket_expr, table_expr, start, end) in &analysis.ir.bracket_table_sites {
        if !seen.insert((start, end)) { continue; }
        if analysis.ir.and_guarded_nil_check_exprs.contains(&bracket_expr) { continue; }
        let Some(vt) = analysis.resolve_expr_type(table_expr) else { continue };
        if !is_nullable(&vt) { continue; }
        if check_nil_suppressed(analysis, table_expr, start) { continue; }
        let type_str = analysis.format_value_type_depth(&vt, 0);
        check_index(diags, &type_str, start as usize, end as usize);
    }
}

/// Re-resolves the callee type post-fixpoint. This intentionally suppresses the diagnostic
/// when narrowing resolved after the call was first seen (e.g. a nil guard later in the
/// fixpoint), which is more correct than the prior inline emission.
pub(crate) fn run_callee(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    for expr in analysis.ir.exprs.iter() {
        let Expr::FunctionCall { func: callee, call_range, .. } = expr else { continue };
        let callee = *callee;
        let call_range = *call_range;
        let Some(func_type) = analysis.resolve_expr_type(callee) else { continue };
        let has_nil = match &func_type {
            ValueType::Union(types) => types.iter().any(|t| matches!(t, ValueType::Nil)),
            _ => false,
        };
        let has_func = match &func_type {
            ValueType::Union(types) => types.iter().any(|t| matches!(t, ValueType::Function(_))),
            _ => false,
        };
        if !has_nil || !has_func { continue; }
        let mut suppressed = analysis.ir.and_guarded_call_exprs.contains(&callee);
        if !suppressed
            && let Some(scope_idx) = analysis.scope_at_offset(call_range.0)
            && let Some(sym_idx) = analysis.ir.find_root_symbol(callee)
        {
            if !analysis.is_narrowing_overridden_at(sym_idx, scope_idx, call_range.0) && analysis.is_symbol_narrowed(sym_idx, scope_idx) {
                suppressed = true;
            } else if let Some((_, chain)) = analysis.ir.extract_field_chain(callee)
                && analysis.is_field_chain_narrowed(sym_idx, &chain, scope_idx)
            {
                suppressed = true;
            }
        }
        if suppressed { continue; }
        let type_str = analysis.format_value_type_depth(&func_type, 0);
        check_call(diags, &type_str, call_range.0 as usize, call_range.1 as usize);
    }
}

pub(crate) fn run_length(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    for &(expr_id, start, end) in &analysis.ir.unary_op_sites {
        let Expr::UnaryOp { operand, .. } = analysis.ir.exprs[expr_id.val()] else { continue };
        let Some(operand_type) = analysis.resolve_expr_type(operand) else { continue };
        if !is_nullable(&operand_type) { continue; }
        if check_nil_suppressed(analysis, operand, start) { continue; }
        let type_str = analysis.format_value_type_depth(&operand_type, 0);
        super::NEED_CHECK_NIL.emit(
            diags,
            format!("'#' on possibly-nil value of type '{type_str}'"),
            start as usize,
            end as usize,
        );
    }
}

pub(crate) struct NeedCheckNil;

impl DiagnosticPass for NeedCheckNil {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        run_access(analysis, diags);
        run_callee(analysis, diags);
        run_length(analysis, diags);
    }
}

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, type_str: &str, start: usize, end: usize) {
    super::NEED_CHECK_NIL.emit(diags, format!("field access on possibly-nil value of type '{}'", type_str), start, end);
}

pub(crate) fn check_index(diags: &mut Vec<WowDiagnostic>, type_str: &str, start: usize, end: usize) {
    super::NEED_CHECK_NIL.emit(diags, format!("index access on possibly-nil value of type '{}'", type_str), start, end);
}

pub(crate) fn check_call(diags: &mut Vec<WowDiagnostic>, type_str: &str, start: usize, end: usize) {
    super::NEED_CHECK_NIL.emit(diags, format!("call on possibly-nil value of type '{}'", type_str), start, end);
}

pub(crate) fn check_param(diags: &mut Vec<WowDiagnostic>, param_name: &str, expected: &str, actual: &str, start: usize, end: usize) {
    super::NEED_CHECK_NIL.emit(diags, format!("possibly-nil value passed to parameter '{}': expected `{}`, got `{}`", param_name, expected, actual), start, end);
}
