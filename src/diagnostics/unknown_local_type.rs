use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "unknown-local-type";

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    if analysis.is_meta { return; }
    for (sym_idx, name, range) in analysis.iter_local_def_sites(tree) {
        let sym = &analysis.ir.symbols[sym_idx.val()];
        let Some(ver) = sym.versions.first() else { continue };
        if ver.resolved_type.is_some() { continue; }
        diags.push(WowDiagnostic {
            code: CODE,
            message: format!("local '{}' has an unknown type", name),
            severity: DiagnosticSeverity::HINT,
            start: u32::from(range.start()) as usize,
            end: u32::from(range.end()) as usize,
        });
    }
}
