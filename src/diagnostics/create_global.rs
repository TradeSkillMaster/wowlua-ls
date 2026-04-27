use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use crate::types::{ScopeIndex, SymbolIdentifier};
use super::WowDiagnostic;

pub(crate) const CODE: &str = "create-global";

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    for sym in &analysis.ir.symbols {
        if sym.scope_idx != ScopeIndex(0) { continue; }
        let name = match &sym.id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => continue,
        };
        if analysis.allowed_write_globals.contains(&name) { continue; }
        if analysis.ir.ext.scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) { continue; }
        if analysis.ir.framexml_enabled
            && analysis.ir.ext.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) { continue; }
        if name.starts_with('_') { continue; }
        let Some(first_ver) = sym.versions.first() else { continue };
        let def_start = first_ver.def_node.start;
        let def_end = first_ver.def_node.end;
        if analysis.is_local_declaration_site(tree, def_start) { continue; }
        let Some(range) = analysis.def_name_token_range(tree, def_start, def_end, &name) else { continue };
        diags.push(WowDiagnostic {
            code: CODE,
            message: format!("implicit global creation '{}'", name),
            severity: DiagnosticSeverity::HINT,
            start: u32::from(range.start()) as usize,
            end: u32::from(range.end()) as usize,
        });
    }
}
