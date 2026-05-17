use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use crate::types::SymbolIdentifier;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct ShadowedLocal;

impl DiagnosticPass for ShadowedLocal {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for sym in &analysis.ir.symbols {
            let name = match &sym.id {
                SymbolIdentifier::Name(n) => n,
                _ => continue,
            };
            if name.starts_with('_') { continue; }

            // Only check locals in inner scopes (scope > 0)
            let scope_idx = sym.scope_idx;
            if scope_idx.val() == 0 || scope_idx.is_external() { continue; }

            // Walk parent scopes to find a same-named symbol
            let parent = analysis.ir.scopes[scope_idx.val()].parent;
            let Some(parent_scope) = parent else { continue; };

            let sym_id = &sym.id;
            let inner_start = match sym.versions.first() {
                Some(v) => v.def_node.start,
                None => continue,
            };
            let mut si = Some(parent_scope);
            let mut found = false;
            while let Some(s) = si {
                if s.is_external() { break; }
                let Some(scope_obj) = analysis.ir.scopes.get(s.val()) else { break; };
                if let Some(&outer_idx) = scope_obj.symbols.get(sym_id) {
                    // Only count as shadowed if the outer symbol is declared before
                    // the inner one. A later declaration in an outer scope is not
                    // visible at the inner declaration site.
                    let outer_declared_before = analysis.ir.symbols.get(outer_idx.val())
                        .and_then(|s| s.versions.first())
                        .is_some_and(|v| v.def_node.start < inner_start);
                    if outer_declared_before {
                        found = true;
                        break;
                    }
                }
                si = scope_obj.parent;
            }
            if !found { continue; }

            // Emit on the first version's definition site
            let Some(first) = sym.versions.first() else { continue; };
            let Some(range) = analysis.def_name_token_range(tree, first.def_node.start, first.def_node.end, name) else { continue };
            super::SHADOWED_LOCAL.emit(
                diags,
                format!("local '{}' shadows a variable in an outer scope", name),
                u32::from(range.start()) as usize,
                u32::from(range.end()) as usize,
            );
        }
    }
}
