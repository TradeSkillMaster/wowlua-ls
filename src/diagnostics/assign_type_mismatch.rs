use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::analysis::build_ir::trimmed_node_end;
use crate::ast::{AstNode, Assign, LocalAssign, Expression};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::*;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "assign-type-mismatch";

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, excess_inject: &mut Vec<InjectFieldCheck>, diags: &mut Vec<WowDiagnostic>) {
    for (&sym_idx, expected) in &analysis.ir.symbol_type_annotations {
        let sym = analysis.sym(sym_idx);
        let var_name = match &sym.id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => continue,
        };

        for (ver_idx, ver) in sym.versions.iter().enumerate() {
            let Some(original_expr) = ver.original_type_source else { continue };
            let Some(node_id) = ver.def_node.node_id else { continue };

            let (start, end) = if ver_idx == 0 {
                // Initial assignment: only check table constructors against class types
                let Some(table_idx) = analysis.ir.find_table_index(original_expr) else { continue };
                if analysis.ir.table(table_idx).fields.is_empty() { continue; }
                let Some(&(s, e)) = analysis.ir.table_ranges.iter()
                    .find(|(_, idx)| **idx == table_idx)
                    .map(|(range, _)| range) else { continue };
                (s, e)
            } else {
                // Reassignment: always check
                let Some(range) = range_from_ast(tree, node_id, &var_name) else { continue };
                range
            };

            let Some(actual) = analysis.resolve_expr_type(original_expr) else { continue };
            if actual.is_assignable_to(expected) {
                continue;
            }
            if analysis.is_table_subtype(&actual, expected) {
                analysis.check_excess_structural_fields(excess_inject, &actual, expected, start as usize, end as usize);
                continue;
            }
            let expected_str = analysis.format_value_type_depth(expected, 1);
            let actual_str = analysis.format_value_type_depth(&actual, 1);
            diags.push(WowDiagnostic {
                code: CODE,
                message: format!("cannot assign '{}' to '{}' (expected '{}')", actual_str, var_name, expected_str),
                severity: DiagnosticSeverity::WARNING,
                start: start as usize,
                end: end as usize,
            });
        }
    }
}

fn range_from_ast(tree: &SyntaxTree, node_id: crate::syntax::tree::NodeId, var_name: &str) -> Option<(u32, u32)> {
    let node = SyntaxNode { tree, id: node_id };
    if let Some(assign) = Assign::cast(node) {
        let identifiers = assign.variable_list()?.identifiers();
        let expressions: Vec<Expression<'_>> = assign.expression_list()
            .map(|el| el.expressions())
            .unwrap_or_default();
        let index = identifiers.iter()
            .position(|id| {
                let names = id.names();
                names.len() == 1 && names[0] == var_name
            })
            .unwrap_or(0);
        let expr_node = expressions.get(index).or_else(|| expressions.last())?;
        let start = u32::from(expr_node.syntax().text_range().start());
        let end = trimmed_node_end(expr_node.syntax());
        Some((start, end))
    } else if let Some(local_assign) = LocalAssign::cast(node) {
        let expressions: Vec<Expression<'_>> = local_assign.expression_list()
            .map(|el| el.expressions())
            .unwrap_or_default();
        let expr_node = expressions.first()?;
        let start = u32::from(expr_node.syntax().text_range().start());
        let end = trimmed_node_end(expr_node.syntax());
        Some((start, end))
    } else {
        None
    }
}
