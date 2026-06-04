use crate::analysis::AnalysisResult;
use super::{DiagnosticPass, WowDiagnostic, is_type_permissive};

pub(crate) struct RedundantCondition;

impl DiagnosticPass for RedundantCondition {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for site in &analysis.ir.condition_sites {
            let Some(cond_type) = analysis.resolve_expr_type(site.expr_id) else { continue };

            if is_type_permissive(&cond_type) { continue; }

            if cond_type.is_guaranteed_truthy() {
                let type_str = analysis.format_type_depth(&cond_type, 1);
                super::REDUNDANT_CONDITION.emit(
                    diags,
                    format!("condition is always truthy (`{type_str}`)"),
                    site.start as usize,
                    site.end as usize,
                );
            } else if cond_type.is_guaranteed_falsy() {
                let type_str = analysis.format_type_depth(&cond_type, 1);
                super::REDUNDANT_CONDITION.emit(
                    diags,
                    format!("condition is always falsy (`{type_str}`)"),
                    site.start as usize,
                    site.end as usize,
                );
            }
        }
    }
}
