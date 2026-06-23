use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, FunctionDefinition, Return};
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use crate::types::SymbolIdentifier;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct IncompleteSignatureDoc;

impl DiagnosticPass for IncompleteSignatureDoc {
    /// Walk function definitions; emit a HINT for each source-level parameter without
    /// an `@param` annotation, and for any function whose body returns a value but
    /// has no `@return` annotation.
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }

        let sentinel = crate::annotations::AnnotationType::Simple(String::new());

        for (_func_idx, func) in analysis.local_functions() {
            let Some(nid) = func.def_node.node_id else { continue };

            let has_return_ann = !func.return_annotations.is_empty()
                || func.returns_self
                || func.returns_built;
            let has_param_ann = func.param_annotations.iter().any(|a| a != &sentinel)
                || func.vararg_annotation.is_some();
            if !has_param_ann && !has_return_ann { continue; }

            let func_node = SyntaxNode { tree, id: nid };
            let Some(func_def) = FunctionDefinition::cast(func_node) else { continue };
            let Some(params_node) = func_def.params() else { continue };

            let mut src_params: Vec<(String, u32, u32)> = Vec::new();
            let mut vararg_range: Option<(u32, u32)> = None;
            for child in params_node.syntax().children_with_tokens() {
                if let NodeOrToken::Token(t) = child {
                    let r = t.text_range();
                    let start = u32::from(r.start());
                    let end = u32::from(r.end());
                    match t.kind() {
                        SyntaxKind::Parameter => src_params.push((t.text().to_string(), start, end)),
                        SyntaxKind::ParameterVarArgs => vararg_range = Some((start, end)),
                        _ => {}
                    }
                }
            }

            let self_injected = func.args.len() == src_params.len() + 1
                && matches!(&analysis.sym(func.args[0]).id,
                    SymbolIdentifier::Name(n) if n == "self");

            let arg_offset = if self_injected { 1 } else { 0 };
            for (i, (name, pstart, pend)) in src_params.iter().enumerate() {
                let arg_i = i + arg_offset;
                if arg_i >= func.args.len() { break; }
                let annotated = func.param_annotations.get(arg_i)
                    .is_some_and(|a| a != &sentinel);
                if annotated { continue; }
                push_missing_param(diags, name, *pstart as usize, *pend as usize);
            }
            if let Some((vstart, vend)) = vararg_range
                && func.vararg_annotation.is_none()
            {
                push_missing_param(diags, "...", vstart as usize, vend as usize);
            }

            if has_return_ann { continue; }
            let body_returns_value = func_def.block().is_some_and(|block| {
                block.syntax().descendants().any(|desc| {
                    let Some(ret) = Return::cast(desc) else { return false };
                    let in_nested_fn = ret.syntax().ancestors().any(|anc| {
                        anc.kind() == SyntaxKind::FunctionDefinition && anc.id != nid
                    });
                    if in_nested_fn { return false; }
                    let Some(expr_list) = ret.expression_list() else { return false };
                    !expr_list.expressions().is_empty()
                })
            });
            if !body_returns_value { continue; }

            // Span the `function` keyword — stable and ends on a token boundary.
            let kw_range = func_def.syntax().children_with_tokens().find_map(|c| {
                if let NodeOrToken::Token(t) = c
                    && t.kind() == SyntaxKind::FunctionKeyword {
                        return Some(t.text_range());
                    }
                None
            }).unwrap_or_else(|| func_def.syntax().text_range());
            let start = u32::from(kw_range.start()) as usize;
            let end = u32::from(kw_range.end()) as usize;
            push_missing_return(diags, start, end);
        }
    }
}

fn push_missing_param(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    super::INCOMPLETE_SIGNATURE_DOC.emit(
        diags,
        format!("parameter '{}' has no '@param' annotation", name),
        start,
        end,
    );
}

fn push_missing_return(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    super::INCOMPLETE_SIGNATURE_DOC.emit(
        diags,
        "function returns a value but has no '@return' annotation".to_string(),
        start,
        end,
    );
}
