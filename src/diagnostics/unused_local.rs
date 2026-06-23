use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use crate::types::Expr;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct UnusedLocal;

impl DiagnosticPass for UnusedLocal {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (sym_idx, name, range) in analysis.iter_local_def_sites(tree) {
            if analysis.referenced_symbols.contains(&sym_idx) { continue; }
            // Skip underscore-prefixed names (Lua convention for intentionally unused)
            if name.starts_with('_') { continue; }
            let start = u32::from(range.start()) as usize;
            let end = u32::from(range.end()) as usize;
            // Emit more specific unused-function for function definitions
            let is_func = analysis.sym(sym_idx).versions.last()
                .and_then(|v| v.type_source)
                .map(|e| matches!(analysis.expr(e), Expr::FunctionDef(_)))
                .unwrap_or(false);
            if is_func {
                super::UNUSED_FUNCTION.emit(
                    diags,
                    format!("unused function '{}'", name),
                    start,
                    end,
                );
            } else {
                super::UNUSED_LOCAL.emit(
                    diags,
                    format!("unused local '{}'", name),
                    start,
                    end,
                );
            }
        }
    }
}
