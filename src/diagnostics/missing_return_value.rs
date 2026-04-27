use lsp_types::DiagnosticSeverity;
use std::collections::HashMap;

use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, Expression, ExpressionList};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use crate::types::ValueType;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "missing-return-value";

/// Walk return statements; emit one of three diagnostics depending on counts:
/// - implicit-nil-return when bare `return` in a function with all-optional @return
/// - missing-return-value when fewer expressions than required
/// - redundant-return-value when more expressions than declared (no `...T`)
pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    let root = SyntaxNode::new_root(tree);

    let func_by_start: HashMap<u32, usize> = analysis.ir.functions.iter().enumerate()
        .filter_map(|(i, f)| f.def_node.node_id.map(|_| (f.def_node.start, i)))
        .collect();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::ReturnStatement { continue; }
        let Some(func_idx) = analysis.find_enclosing_function_idx(node, &func_by_start) else { continue };
        let func = &analysis.ir.functions[func_idx];
        let expected_count = func.return_annotations.len();
        if expected_count == 0 { continue; }

        let expr_count = node.children()
            .find_map(ExpressionList::cast)
            .map(|el| el.expressions().len())
            .unwrap_or(0);
        let last_is_multi = node.children()
            .find_map(ExpressionList::cast)
            .map(|el| matches!(
                el.expressions().last(),
                Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
            ))
            .unwrap_or(false);

        let has_nil_overload = func.overloads.iter().any(|o| {
            o.is_return_only
                && (o.returns.is_empty()
                    || o.returns.iter().all(|t| t == &ValueType::Nil))
        });
        let effective_expected = if func.has_vararg_return && expected_count > 0 {
            expected_count - 1
        } else {
            expected_count
        };

        let r = node.text_range();
        let start = u32::from(r.start()) as usize;
        let end = crate::analysis::build_ir::trimmed_node_end(node) as usize;

        if expr_count < effective_expected && !last_is_multi && !has_nil_overload {
            let omitted_all_optional = func.return_annotations[expr_count..effective_expected]
                .iter().all(|t| t.contains_nil());
            let all_returns_nullable = expr_count == 0 && omitted_all_optional;
            if all_returns_nullable {
                diags.push(WowDiagnostic {
                    code: super::implicit_nil_return::CODE,
                    message: format!("bare return implicitly returns nil for {} optional return value(s)", effective_expected),
                    severity: DiagnosticSeverity::HINT,
                    start, end,
                });
            } else if !omitted_all_optional {
                diags.push(WowDiagnostic {
                    code: CODE,
                    message: format!("expected {} return value(s) but got {}", effective_expected, expr_count),
                    severity: DiagnosticSeverity::WARNING,
                    start, end,
                });
            }
        }

        if expr_count > expected_count && !func.has_vararg_return
            && let Some(extra) = node.children()
                .find_map(ExpressionList::cast)
                .and_then(|el| el.expressions().get(expected_count).map(|e| e.syntax().text_range()))
        {
            super::redundant_return_value::check(
                diags,
                expected_count, expr_count,
                u32::from(extra.start()) as usize, u32::from(extra.end()) as usize,
            );
        }
    }
}
