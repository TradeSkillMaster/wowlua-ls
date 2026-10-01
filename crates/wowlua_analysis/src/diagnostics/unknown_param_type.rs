use crate::analysis::AnalysisResult;
use crate::ast::*;
use crate::collections::HashSet;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, TextRange};
use crate::types::*;
use super::unknown_local_type::{unknown_type_message, unknown_writes};
use super::{DiagnosticPass, WowDiagnostic};

pub struct UnknownParamType;

impl DiagnosticPass for UnknownParamType {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }
        let sentinel = crate::annotations::AnnotationType::Simple(String::new());
        let mut emit = |label: &str, desc: &str, range: TextRange| {
            super::UNKNOWN_PARAM_TYPE.emit(
                diags,
                format!("{} {}", label, desc),
                u32::from(range.start()) as usize,
                u32::from(range.end()) as usize,
            );
        };
        for (_func_idx, func) in analysis.local_functions() {
            let Some(nid) = func.def_node.node_id else { continue };
            let func_node = SyntaxNode { tree, id: nid };
            let Some(func_def) = FunctionDefinition::cast(func_node) else { continue };
            let Some(params_node) = func_def.params() else { continue };

            let src_params: Vec<(String, TextRange)> = params_node.parameter_tokens().iter()
                .map(|t| (t.text().to_string(), t.text_range()))
                .collect();

            let self_injected = func.args.len() == src_params.len() + 1
                && matches!(&analysis.sym(func.args[0]).id,
                    SymbolIdentifier::Name(n) if n == "self");
            let arg_offset = if self_injected { 1 } else { 0 };

            // Each param: its declaration (the param token), then any reassignments
            // in the body. A repeated name (`function(_, _)`) is one symbol, so its
            // reassignments are checked once.
            let mut params: Vec<(SymbolIndex, String, Option<TextRange>)> = Vec::new();
            if self_injected {
                // Implicit `self` of `function T:m()` has no token; anchor on the name.
                let range = func_def.identifier().map(|ident| ident.syntax().text_range());
                params.push((func.args[0], "self".to_string(), range));
            }
            for (i, (name, range)) in src_params.into_iter().enumerate() {
                let Some(&sym_idx) = func.args.get(i + arg_offset) else { break };
                params.push((sym_idx, name, Some(range)));
            }

            let mut writes_checked: HashSet<SymbolIndex> = HashSet::default();
            // `params` is index-aligned with `func.args` / `func.param_annotations`.
            for (i, (sym_idx, name, decl_range)) in params.iter().enumerate() {
                if sym_idx.is_external() { continue; }
                let sym = analysis.sym(*sym_idx);
                let resolved = analysis.ir.param_decl_type(func, i);
                let annotated = func.param_annotations.get(i).is_some_and(|a| a != &sentinel);
                if let Some(range) = *decl_range
                    && let Some(desc) = unknown_type_message(resolved, annotated)
                {
                    let label = if self_injected && i == 0 {
                        "implicit parameter 'self'".to_string()
                    } else {
                        format!("parameter '{}'", name)
                    };
                    emit(&label, desc, range);
                }
                if !writes_checked.insert(*sym_idx) { continue; }
                for (vi, range, desc) in unknown_writes(analysis, tree, sym, name, annotated) {
                    if vi == 0 { continue; }
                    emit(&format!("parameter '{}'", name), desc, range);
                }
            }
        }
    }
}
