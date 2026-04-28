use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "multi-return-projection";

pub(crate) fn run(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    for cr in analysis.ir.call_resolutions.values() {
        let Some(f_idx) = cr.projected_f_idx else { continue };
        if cr.is_expansion { continue; }
        let f = analysis.func(f_idx);
        if f.return_annotations.len() > 1
            && let Some(&(start, end)) = cr.first_arg_range.as_ref()
        {
            check_emit(diags, start as usize, end as usize);
        }
    }
}

pub(crate) fn check_emit(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "returns<F> projects only column 0; F has multiple return values and the extras are discarded".to_string(),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
