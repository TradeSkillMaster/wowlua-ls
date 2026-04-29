use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, FunctionDefinition};
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct UnusedVararg;

impl DiagnosticPass for UnusedVararg {
    fn visit_node(&self, node: SyntaxNode<'_>, _analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
        if node.kind() != SyntaxKind::FunctionDefinition { return; }
        let Some(func) = FunctionDefinition::cast(node) else { return };
        let Some(params) = func.params() else { return };
        if !params.ellipsis() { return; }
        let Some(body) = func.block() else { return };
        if body_uses_varargs(body.syntax()) { return; }
        let vararg_range = params.syntax().children_with_tokens()
            .find_map(|c| match c {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::ParameterVarArgs => Some(t.text_range()),
                _ => None,
            });
        let Some(vararg_range) = vararg_range else { return };
        let name = func.identifier()
            .and_then(|id| id.names().last().cloned())
            .or_else(|| func.name());
        let message = match name.as_deref() {
            Some(n) => format!("function '{}' declares '...' but never uses it", n),
            None => "function declares '...' but never uses it".to_string(),
        };
        super::UNUSED_VARARG.emit(
            diags,
            message,
            u32::from(vararg_range.start()) as usize,
            u32::from(vararg_range.end()) as usize,
        );
    }
}

fn body_uses_varargs(body: SyntaxNode<'_>) -> bool {
    for child in body.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::TripleDot {
                    return true;
                }
            }
            NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::FunctionDefinition {
                    continue;
                }
                if body_uses_varargs(n) {
                    return true;
                }
            }
        }
    }
    false
}
