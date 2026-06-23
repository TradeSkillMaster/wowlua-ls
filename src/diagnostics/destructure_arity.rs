use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, Assign, Expression, LocalAssign};
use crate::syntax::tree::{SyntaxNode, SyntaxTree};
use crate::types::{
    CallResolution, Expr, ExprId, Function, FunctionIndex, SymbolIdentifier, ValueType,
};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct DestructureArity;

/// Walk LocalAssign and Assign nodes. When the last RHS expression is a
/// function call and the number of LHS variables exceeds the function's
/// return arity, emit `unbalanced-assignments`.
impl DiagnosticPass for DestructureArity {
    fn run(
        &self,
        analysis: &AnalysisResult,
        tree: &SyntaxTree,
        diags: &mut Vec<WowDiagnostic>,
    ) {
        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            if let Some(assign) = LocalAssign::cast(node) {
                let Some(name_list) = assign.name_list() else { continue };
                let lhs_count = name_list.names().len();
                let expressions = assign
                    .expression_list()
                    .map(|el| el.expressions())
                    .unwrap_or_default();
                check_assignment(diags, analysis, lhs_count, &expressions, assign.syntax().text_range());
            } else if let Some(assign) = Assign::cast(node) {
                let Some(var_list) = assign.variable_list() else { continue };
                let lhs_count = var_list.identifiers().len();
                let expressions = assign
                    .expression_list()
                    .map(|el| el.expressions())
                    .unwrap_or_default();
                check_assignment(diags, analysis, lhs_count, &expressions, assign.syntax().text_range());
            }
        }
    }
}

/// Shared logic for both local and non-local assignments. Checks whether
/// the last RHS expression is a function call whose return arity is less
/// than the number of LHS variables receiving values from it.
fn check_assignment(
    diags: &mut Vec<WowDiagnostic>,
    analysis: &AnalysisResult,
    lhs_count: usize,
    expressions: &[Expression<'_>],
    assign_range: crate::syntax::tree::TextRange,
) {
    // Only handle multi-return: more LHS targets than RHS expressions,
    // with the last RHS being a function call.
    if lhs_count <= expressions.len() { return; }
    let Some(Expression::FunctionCall(call)) = expressions.last() else { return };
    let call_range = call.syntax().text_range();
    let call_start = u32::from(call_range.start());
    let call_end = u32::from(call_range.end());

    let Some((expr_id, cr)) = find_call_resolution(analysis, call_start, call_end) else {
        return;
    };
    let func = analysis.func(cr.func_idx);
    let Some(arity) = return_arity(analysis, func, cr.func_idx, &cr) else { return };

    // Variables receiving values from the call = lhs_count - (expressions.len() - 1)
    let vars_from_call = lhs_count - (expressions.len() - 1);
    if vars_from_call > arity {
        let func_name = find_call_name(analysis, expr_id);
        let msg = format_message(func_name.as_deref(), arity, vars_from_call);
        super::UNBALANCED_ASSIGNMENTS.emit(
            diags,
            msg,
            u32::from(assign_range.start()) as usize,
            u32::from(assign_range.end()) as usize,
        );
    }
}

/// Find the IR `FunctionCall` expr at `ret_index=0` that matches the given
/// AST call range, and return its ExprId + CallResolution.
fn find_call_resolution(
    analysis: &AnalysisResult,
    call_start: u32,
    call_end: u32,
) -> Option<(ExprId, CallResolution)> {
    for (expr_id, expr) in analysis.local_exprs() {
        if let Expr::FunctionCall {
            call_range: (s, e),
            ret_index: 0,
            ..
        } = expr
            && *s == call_start && *e == call_end
        {
            if let Some(cr) = analysis.ir.call_resolutions.get(&expr_id) {
                return Some((expr_id, cr.clone()));
            }
            // No call_resolutions entry — this happens for calls that the
            // resolver couldn't fully process (e.g. no type-args to bind).
            // Fall back to resolving the callee type directly.
            if let Expr::FunctionCall { func: callee, .. } = expr
                && let Some(func_idx) = resolve_callee(analysis, *callee)
            {
                return Some((
                    expr_id,
                    CallResolution {
                        func_idx,
                        expected_args: Vec::new(),
                        generic_subs: Vec::new(),
                        projected_f_idx: None,
                        is_expansion: false,
                        first_arg_range: None,
                        receiver_param_subs: std::collections::HashMap::new(),
                        receiver_table_idx: None,
                    },
                ));
            }
            return None;
        }
    }
    None
}

/// Resolve a callee expression to a FunctionIndex.
fn resolve_callee(analysis: &AnalysisResult, callee: ExprId) -> Option<FunctionIndex> {
    let callee_type = analysis.resolve_expr_type(callee)?;
    match callee_type {
        ValueType::Function(Some(idx)) => Some(idx),
        ValueType::Table(Some(table_idx)) => {
            if let Some(fi) = analysis.table(table_idx).call_func {
                Some(fi)
            } else {
                analysis.resolve_constructor_func(table_idx)
            }
        }
        _ => None,
    }
}

/// Compute the return arity of a function at a call site.
/// Returns `None` when the arity is unknown or unbounded (vararg return).
fn return_arity(
    analysis: &AnalysisResult,
    func: &Function,
    func_idx: FunctionIndex,
    call_resolution: &CallResolution,
) -> Option<usize> {
    // Vararg return: no upper bound
    if func.has_vararg_return {
        return None;
    }

    // Any non-return-only overload with vararg tail: no upper bound
    if func.overloads.iter().any(|o| !o.is_return_only && o.has_vararg_tail) {
        return None;
    }

    // Handle returns<F> projections: resolve F from generic subs
    if !func.return_projections.is_empty() {
        return return_arity_with_projection(analysis, func, call_resolution);
    }

    // Annotated returns — take the max of primary annotations and overloads.
    // This is conservative: if any overload returns more values, we allow
    // that many variables. We could instead warn per-overload, but that
    // requires call-site overload selection which isn't available here.
    if !func.return_annotations.is_empty() {
        let base = func.return_annotations.len();
        let max_overload = max_non_return_only_overload_arity(func);
        return Some(base.max(max_overload));
    }

    // Overloads only (no primary return annotations)
    if func.overloads.iter().any(|o| !o.is_return_only) {
        return Some(max_non_return_only_overload_arity(func));
    }

    // Explicit void return
    if func.explicit_void_return {
        return Some(0);
    }

    // Inferred from body: max FunctionRet index + 1.
    // Skip when the return expression at the max slot is a FunctionCall or VarArgs,
    // because those can pass through more values than the expression count suggests
    // (e.g. `return bar()` where bar returns 2 values creates only 1 FunctionRet).
    if !func.rets.is_empty() {
        let max_slot = func
            .rets
            .iter()
            .filter_map(|&sym_idx| {
                if let SymbolIdentifier::FunctionRet(fi, idx) = &analysis.sym(sym_idx).id
                    && *fi == func_idx
                {
                    Some((*idx, sym_idx))
                } else {
                    None
                }
            })
            .max_by_key(|(idx, _)| *idx);
        if let Some((max_idx, max_sym)) = max_slot {
            // Check if the expression at the max slot is a tail call or varargs
            let is_open_ended = analysis.sym(max_sym).versions.first()
                .and_then(|v| v.type_source)
                .is_some_and(|expr_id| matches!(
                    analysis.expr(expr_id),
                    Expr::FunctionCall { .. } | Expr::VarArgs(..)
                ));
            if is_open_ended {
                return None; // arity unknown — tail call may return more values
            }
            return Some(max_idx + 1);
        }
    }

    // Body was analyzed (same-file) and falls through with no return statements:
    // the function genuinely returns 0 values.
    if func.implicit_nil_return {
        return Some(0);
    }

    // Unknown
    None
}

/// Compute return arity when the function uses `returns<F>` projections.
///
/// Currently resolves only the first `Return` projection found. Functions
/// with multiple independent projections are rare; if needed, this could
/// be extended to combine arity from all projections.
fn return_arity_with_projection(
    analysis: &AnalysisResult,
    func: &Function,
    call_resolution: &CallResolution,
) -> Option<usize> {
    // Use projected_f_idx if available (set during call resolution)
    let f_idx = call_resolution.projected_f_idx.or_else(|| {
        // Try to resolve F from generic_subs (uses the first Return projection)
        func.return_projections.values().find_map(|proj| {
            if let crate::types::ProjectionKind::Return(name, _) = proj {
                call_resolution.generic_subs.iter().find_map(|(gname, vt, _)| {
                    if gname == name
                        && let ValueType::Function(Some(f)) = vt
                    {
                        Some(*f)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
    })?;

    let f = analysis.func(f_idx);
    if f.has_vararg_return {
        return None;
    }

    // When the last `returns<F>` projection is at or beyond the last return
    // annotation, higher return slots expand into F's returns.  This covers
    // both the common single-annotation pattern (`@return returns<F>`) and
    // mixed patterns like pcall (`@return boolean`, `@return returns<F>`).
    let last_proj = func.return_projections.keys().max().copied().unwrap_or(0);
    let expansion_possible = last_proj + 1 >= func.return_annotations.len();

    if expansion_possible {
        // Effective arity = slots before projection + F's return arity
        let f_arity = if !f.return_annotations.is_empty() {
            f.return_annotations.len()
        } else if !f.rets.is_empty() {
            f.rets
                .iter()
                .filter_map(|&sym_idx| {
                    if let SymbolIdentifier::FunctionRet(_, idx) = &analysis.sym(sym_idx).id {
                        Some(*idx + 1)
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(0)
        } else {
            return None;
        };
        // e.g. pcall: last_proj=1 (the `returns<F>` slot) + f_arity=3
        // → 4 total (boolean, ret1, ret2, ret3)
        return Some(last_proj + f_arity);
    }

    // Multiple return annotations where expansion isn't possible:
    // projections substitute types but don't add extra slots.
    Some(func.return_annotations.len())
}

/// Max return count across non-return-only overloads (0 if none).
fn max_non_return_only_overload_arity(func: &Function) -> usize {
    func.overloads
        .iter()
        .filter(|o| !o.is_return_only && !o.has_vararg_tail)
        .map(|o| o.returns.len())
        .max()
        .unwrap_or(0)
}

fn format_message(func_name: Option<&str>, arity: usize, vars: usize) -> String {
    let ret_word = if arity == 1 { "value" } else { "values" };
    let var_word = if vars == 1 { "variable" } else { "variables" };
    if let Some(name) = func_name {
        format!("{name}() returns {arity} {ret_word} but assigned to {vars} {var_word}")
    } else {
        format!("function returns {arity} {ret_word} but assigned to {vars} {var_word}")
    }
}

/// Try to extract a human-readable function name from the callee expression.
fn find_call_name(analysis: &AnalysisResult, expr_id: ExprId) -> Option<String> {
    if let Expr::FunctionCall { func: callee, .. } = analysis.expr(expr_id) {
        match analysis.expr(*callee) {
            Expr::SymbolRef(sym_idx, _) => {
                if let SymbolIdentifier::Name(n) = &analysis.sym(*sym_idx).id {
                    return Some(n.clone());
                }
            }
            Expr::FieldAccess { field, .. } => {
                return Some(field.clone());
            }
            _ => {}
        }
    }
    None
}
