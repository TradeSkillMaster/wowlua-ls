use crate::analysis::{AnalysisResult, ancestor_scopes};
use crate::ast::Operator;
use crate::types::{Expr, ExprId, ScopeIndex, SymbolIndex, ValueType};
use super::{DiagnosticPass, WowDiagnostic, is_type_permissive, is_expr_truthiness_uncertain};

pub(crate) struct RedundantCondition;

impl DiagnosticPass for RedundantCondition {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for site in &analysis.ir.condition_sites {
            let Some(cond_type) = analysis.resolve_expr_type(site.expr_id) else { continue };

            if is_type_permissive(&cond_type) { continue; }

            // Skip boolean literals in loop conditions (`while true do`,
            // `repeat...until false`) — these are standard infinite-loop idioms.
            // Non-loop contexts (`if true`, `if false`, `elseif true`) are still
            // flagged as they typically indicate dead code or a forgotten condition.
            if site.is_loop && matches!(analysis.ir.expr(site.expr_id), Expr::Literal(ValueType::Boolean(Some(_)))) {
                continue;
            }

            // Skip expressions whose truthiness can't be reliably determined
            // from static types (lateinit fields, unannotated fields, dynamic
            // indices, unannotated parameters).
            if is_expr_truthiness_uncertain(analysis, site.expr_id) { continue; }

            let label = if cond_type.is_guaranteed_truthy() {
                "truthy"
            } else if cond_type.is_guaranteed_falsy() {
                "falsy"
            } else {
                continue;
            };

            // Suppress when the condition references a variable that is
            // reassigned within a loop body — either an enclosing loop (the
            // variable's type may differ across iterations) or a preceding
            // loop (after the loop the variable could retain its pre-loop
            // value or the in-loop value).
            if is_loop_reassigned_condition(&analysis.ir, site.expr_id, site.start, site.loop_scope) {
                continue;
            }

            let type_str = analysis.format_type_depth(&cond_type, 1);
            super::REDUNDANT_CONDITION.emit(
                diags,
                format!("condition is always {label} (`{type_str}`)"),
                site.start as usize,
                site.end as usize,
            );
        }
    }
}

/// Collect all `SymbolRef`s (with version indices) reachable from an
/// expression by unwrapping narrowing wrappers, `not`, and `and`/`or`
/// operands.
fn collect_symbol_refs(ir: &crate::analysis::Ir, expr_id: ExprId, out: &mut Vec<(SymbolIndex, usize)>) {
    match ir.expr(expr_id) {
        Expr::SymbolRef(sym_idx, ver_idx) => out.push((*sym_idx, *ver_idx)),
        Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::StripTruthy(inner)
        | Expr::Grouped(inner) => collect_symbol_refs(ir, *inner, out),
        Expr::UnaryOp { op: Operator::Not, operand } => collect_symbol_refs(ir, *operand, out),
        Expr::BinaryOp { op: Operator::And | Operator::Or, lhs, rhs } => {
            let (lhs, rhs) = (*lhs, *rhs);
            collect_symbol_refs(ir, lhs, out);
            collect_symbol_refs(ir, rhs, out);
        }
        _ => {}
    }
}

/// Check whether the condition references a variable that is reassigned inside
/// a loop body — either an enclosing loop (value may differ across iterations)
/// or a preceding loop (post-loop value could be the pre-loop or in-loop one).
fn is_loop_reassigned_condition(
    ir: &crate::analysis::Ir,
    expr_id: ExprId,
    offset: u32,
    loop_scope_hint: Option<ScopeIndex>,
) -> bool {
    let mut sym_refs = Vec::new();
    collect_symbol_refs(ir, expr_id, &mut sym_refs);
    if sym_refs.is_empty() { return false; }

    let local_syms: Vec<_> = sym_refs.iter().filter_map(|&(idx, _)| {
        if idx.is_external() { return None; }
        Some((idx, ir.sym(idx)))
    }).collect();

    // Case 1: condition is inside a loop (or in while/repeat...until position
    // where ancestor-walking won't find the loop body — use the stored hint).
    let enclosing_loop = loop_scope_hint.or_else(|| {
        let cond_scope = ir.scope_at_offset(offset)?;
        find_enclosing_loop(ir, cond_scope)
    });

    if enclosing_loop.is_some_and(|loop_scope| {
        local_syms.iter().any(|(_, sym)| {
            sym.versions.iter().any(|ver| {
                is_scope_inside(ir, ver.created_in_scope, loop_scope)
            })
        })
    }) {
        return true;
    }

    // Case 2: condition is after a preceding loop. A variable defined before
    // the loop but reassigned inside it may hold either its pre-loop or
    // in-loop value. Only suppress when the loop ends before the condition.
    if local_syms.iter().any(|(_, sym)| {
        sym.versions.iter().any(|ver| {
            // Find the innermost loop enclosing this version's creation scope.
            let Some(ver_loop) = find_enclosing_loop(ir, ver.created_in_scope) else { return false };
            // Only suppress when the symbol was defined outside that loop.
            if is_scope_inside(ir, sym.scope_idx, ver_loop) { return false; }
            // Only suppress when the loop precedes the condition — a loop
            // appearing after the condition cannot affect the condition's value.
            let Some(&(_, loop_end, _)) = ir.block_scopes.iter().find(|&&(_, _, s)| s == ver_loop) else { return false };
            loop_end <= offset
        })
    }) {
        return true;
    }

    // Case 3: transitive — the condition references a variable whose defining
    // expression depends on a loop-reassigned variable (one level deep).
    // Handles patterns like `local result = expr and loopVar or nil`.
    sym_refs.iter().any(|&(sym_idx, ver_idx)| {
        sym_def_has_loop_reassigned_dep(ir, sym_idx, ver_idx, offset)
    })
}

/// Find the nearest ancestor scope (inclusive) that is a loop body.
fn find_enclosing_loop(ir: &crate::analysis::Ir, scope: ScopeIndex) -> Option<ScopeIndex> {
    ancestor_scopes(&ir.scopes, scope).find(|&s| ir.scopes[s.val()].is_loop)
}

/// Returns true if `scope` is `container` or a descendant of `container`.
fn is_scope_inside(ir: &crate::analysis::Ir, scope: ScopeIndex, container: ScopeIndex) -> bool {
    ancestor_scopes(&ir.scopes, scope).any(|s| s == container)
}

/// One level of transitive expansion: check if the visible version's defining
/// expression references symbols that have preceding loop versions.  This
/// handles cases like `local result = expr and loopVar or nil`.
///
/// Deeper chains (e.g. `local a = loopVar; local b = a; if b then`) are not
/// followed — the second indirection is a known limitation.
fn sym_def_has_loop_reassigned_dep(
    ir: &crate::analysis::Ir,
    sym_idx: SymbolIndex,
    ver_idx: usize,
    offset: u32,
) -> bool {
    if sym_idx.is_external() { return false; }
    let sym = ir.sym(sym_idx);
    let Some(ver) = sym.versions.get(ver_idx) else { return false };
    let Some(src) = ver.type_source else { return false };

    let mut dep_refs = Vec::new();
    collect_symbol_refs(ir, src, &mut dep_refs);
    dep_refs.iter().any(|&(dep_sym_idx, _)| {
        if dep_sym_idx.is_external() { return false; }
        let dep_sym = ir.sym(dep_sym_idx);
        dep_sym.versions.iter().any(|dep_ver| {
            let Some(ver_loop) = find_enclosing_loop(ir, dep_ver.created_in_scope) else { return false };
            if is_scope_inside(ir, dep_sym.scope_idx, ver_loop) { return false; }
            let Some(&(_, loop_end, _)) = ir.block_scopes.iter().find(|&&(_, _, s)| s == ver_loop) else { return false };
            loop_end <= offset
        })
    })
}
