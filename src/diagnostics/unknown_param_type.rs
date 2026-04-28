use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::ast::*;
use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::*;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "unknown-param-type";

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    if analysis.is_meta { return; }
    let sentinel = crate::annotations::AnnotationType::Simple(String::new());
    for func_idx in 0..analysis.ir.functions.len() {
        let func = &analysis.ir.functions[func_idx];
        let Some(nid) = func.def_node.node_id else { continue };
        let func_node = SyntaxNode { tree, id: nid };
        let Some(func_def) = FunctionDefinition::cast(func_node) else { continue };
        let Some(params_node) = func_def.params() else { continue };

        let src_params: Vec<(String, u32, u32)> = params_node.syntax().children_with_tokens()
            .filter_map(|c| match c {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Parameter => {
                    let r = t.text_range();
                    Some((t.text().to_string(), u32::from(r.start()), u32::from(r.end())))
                }
                _ => None,
            })
            .collect();

        let self_injected = func.args.len() == src_params.len() + 1
            && matches!(&analysis.ir.symbols[func.args[0].val()].id,
                SymbolIdentifier::Name(n) if n == "self");
        let arg_offset = if self_injected { 1 } else { 0 };

        for (i, (name, pstart, pend)) in src_params.iter().enumerate() {
            let arg_i = i + arg_offset;
            if arg_i >= func.args.len() { break; }
            let sym_idx = func.args[arg_i];
            if sym_idx.is_external() { continue; }
            if name == "self" { continue; }
            let annotated = func.param_annotations.get(arg_i)
                .is_some_and(|a| a != &sentinel);
            if annotated { continue; }
            let resolved = analysis.ir.symbols[sym_idx.val()].versions.first()
                .and_then(|v| v.resolved_type.as_ref());
            if resolved.is_some() { continue; }
            check(diags, name, *pstart as usize, *pend as usize);
        }
    }
}

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("parameter '{}' has an unknown type", name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
