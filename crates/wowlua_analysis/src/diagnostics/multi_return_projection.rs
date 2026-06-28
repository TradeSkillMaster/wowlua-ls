use std::collections::{HashMap, HashSet};
use crate::analysis::AnalysisResult;
use crate::types::Expr;
use super::{DiagnosticPass, WowDiagnostic};

pub struct MultiReturnProjection;

impl DiagnosticPass for MultiReturnProjection {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        // Collect call_range starts for call resolutions that have a projection
        // and could reach the expansion check.  We only need to scan exprs for
        // these call sites, so the map stays small even in large files.
        let relevant: HashSet<u32> = analysis.ir.call_resolutions.iter()
            .filter(|(_, cr)| cr.projected_f_idx.is_some() && !cr.is_expansion)
            .filter_map(|(expr_id, _)| {
                if let Expr::FunctionCall { call_range, .. } = analysis.ir.expr(*expr_id) {
                    Some(call_range.0)
                } else {
                    None
                }
            })
            .collect();

        // Build a map: call_range_start → max ret_index seen at that call site.
        // This lets us detect when the caller captures multiple return slots via
        // expansion (e.g. `local a, b = wrap(f)` has ret_index 0 and 1).
        let mut max_ret_at_call: HashMap<u32, usize> = HashMap::new();
        for (_, expr) in analysis.local_exprs() {
            if let Expr::FunctionCall { call_range, ret_index, .. } = expr
                && relevant.contains(&call_range.0)
            {
                let entry = max_ret_at_call.entry(call_range.0).or_insert(0);
                if *ret_index > *entry {
                    *entry = *ret_index;
                }
            }
        }

        for (expr_id, cr) in analysis.ir.call_resolutions.iter() {
            let Some(f_idx) = cr.projected_f_idx else { continue };
            if cr.is_expansion { continue; }
            // Skip when the projection has an offset param (returns<F, index>) —
            // the caller intentionally selects a specific return position.
            let caller = analysis.func(cr.func_idx);
            let has_offset = caller.return_projections.values()
                .any(|p| matches!(p, crate::types::ProjectionKind::Return(_, Some(_))));
            if has_offset { continue; }
            // Skip when the projection is at a non-zero return slot — the
            // function explicitly has prefix returns and intends to pass
            // through F's full return set (e.g. pcall's `@return boolean`
            // followed by `@return returns<F>`).
            let proj_at_nonzero = caller.return_projections.keys().max()
                .is_some_and(|&max_idx| max_idx > 0);
            if proj_at_nonzero { continue; }
            let f = analysis.func(f_idx);
            let f_extra_returns = f.return_annotations.len().saturating_sub(1);
            if f_extra_returns == 0 { continue; }
            // Skip when the call site actually captures all of F's extra returns
            // via expansion slots (e.g. `local a, b = wrap(f)` uses ret_index 1).
            // If expr_id unexpectedly isn't a FunctionCall, skip defensively.
            let Expr::FunctionCall { call_range, .. } = analysis.ir.expr(*expr_id) else { continue };
            let max_ret = max_ret_at_call.get(&call_range.0).copied().unwrap_or(0);
            if max_ret >= f_extra_returns { continue; }
            if let Some(&(start, end)) = cr.first_arg_range.as_ref() {
                check_emit(diags, start as usize, end as usize);
            }
        }
    }
}

pub fn check_emit(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    super::MULTI_RETURN_PROJECTION.emit(diags, "returns<F> projects only column 0; F has multiple return values and the extras are discarded".to_string(), start, end);
}
