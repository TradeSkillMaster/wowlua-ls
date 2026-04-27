use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use crate::types::Expr;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "unused-local";

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    for (sym_idx, name, range) in analysis.iter_local_def_sites(tree) {
        if analysis.referenced_symbols.contains(&sym_idx) { continue; }
        // Skip underscore-prefixed names (Lua convention for intentionally unused)
        if name.starts_with('_') { continue; }
        let start = u32::from(range.start()) as usize;
        let end = u32::from(range.end()) as usize;
        // Emit more specific unused-function for function definitions
        let is_func = analysis.ir.symbols[sym_idx.val()].versions.last()
            .and_then(|v| v.type_source)
            .map(|e| matches!(analysis.expr(e), Expr::FunctionDef(_)))
            .unwrap_or(false);
        if is_func {
            diags.push(WowDiagnostic {
                code: super::unused_function::CODE,
                message: format!("unused function '{}'", name),
                severity: DiagnosticSeverity::HINT,
                start,
                end,
            });
        } else {
            diags.push(WowDiagnostic {
                code: CODE,
                message: format!("unused local '{}'", name),
                severity: DiagnosticSeverity::HINT,
                start,
                end,
            });
        }
    }
}
