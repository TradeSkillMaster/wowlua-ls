use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct UnknownLocalType;

impl DiagnosticPass for UnknownLocalType {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }
        for (sym_idx, name, range) in analysis.iter_local_def_sites(tree) {
            // `_` is the conventional throwaway name; its type is intentionally ignored.
            if name == "_" { continue; }
            let sym = analysis.sym(sym_idx);
            let Some(ver) = sym.versions.first() else { continue };
            if ver.resolved_type.is_some() { continue; }
            super::UNKNOWN_LOCAL_TYPE.emit(
                diags,
                format!("local '{}' has an unknown type", name),
                u32::from(range.start()) as usize,
                u32::from(range.end()) as usize,
            );
        }
    }
}
