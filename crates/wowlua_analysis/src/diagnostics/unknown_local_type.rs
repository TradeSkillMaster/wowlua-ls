use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub struct UnknownLocalType;

impl DiagnosticPass for UnknownLocalType {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }
        for (sym_idx, name, range) in analysis.iter_local_def_sites(tree) {
            // `_` is the conventional throwaway name; its type is intentionally ignored.
            if name == "_" { continue; }
            let sym = analysis.sym(sym_idx);
            let Some(ver) = sym.versions.first() else { continue };
            if ver.resolved_type.is_some() { continue; }
            // A forward declaration with no initializer (`local x`) begins as an
            // untyped nil placeholder, so version 0 carries no resolved type even
            // when a later assignment — e.g. in every branch of an if/else — gives
            // the local a concrete type that the LS resolves at every use site.
            // `type_source.is_none()` identifies that no-initializer case (a present
            // initializer that simply couldn't be typed keeps version 0's
            // `type_source`, and stays flagged). When any later version resolved to a
            // type, the local is typed and this is not an unknown-type site.
            if ver.type_source.is_none()
                && sym.versions.iter().skip(1).any(|v| v.resolved_type.is_some())
            {
                continue;
            }
            super::UNKNOWN_LOCAL_TYPE.emit(
                diags,
                format!("local '{}' has an unknown type", name),
                u32::from(range.start()) as usize,
                u32::from(range.end()) as usize,
            );
        }
    }
}
