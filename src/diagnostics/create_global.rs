use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use crate::types::{ScopeIndex, SymbolIdentifier};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct CreateGlobal;

impl DiagnosticPass for CreateGlobal {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (_, sym) in analysis.local_symbols() {
            if sym.scope_idx != ScopeIndex(0) { continue; }
            let name = match &sym.id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => continue,
            };
            if analysis.allowed_write_globals.contains(&name) { continue; }
            if analysis.allow_slash_commands && name.starts_with("SLASH_") { continue; }
            if analysis.allow_binding_globals
                && (name.starts_with("BINDING_HEADER_") || name.starts_with("BINDING_NAME_"))
            {
                continue;
            }
            if let Some(&sym_idx) = analysis.ir.ext.scope0_symbols.get(&SymbolIdentifier::Name(name.clone()))
                && analysis.is_stub_symbol(sym_idx) { continue; }
            if analysis.ir.framexml_enabled
                && analysis.ir.ext.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) { continue; }
            if name.starts_with('_') { continue; }
            if analysis.explicit_globals.contains(&name) { continue; }
            let Some(first_ver) = sym.versions.first() else { continue; };
            let def_start = first_ver.def_node.start;
            let def_end = first_ver.def_node.end;
            if analysis.is_local_declaration_site(tree, def_start) { continue; }
            let Some(range) = analysis.def_name_token_range(tree, def_start, def_end, &name) else { continue };
            super::CREATE_GLOBAL.emit(
                diags,
                format!("implicit global creation '{}'", name),
                u32::from(range.start()) as usize,
                u32::from(range.end()) as usize,
            );
        }
    }
}
