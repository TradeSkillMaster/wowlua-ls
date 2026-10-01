use crate::analysis::AnalysisResult;
use crate::ast::*;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::SyntaxNode;
use crate::types::*;
use super::unknown_local_type::unknown_type_message;
use super::{DiagnosticPass, WowDiagnostic};

pub struct UnknownReturnType;

impl DiagnosticPass for UnknownReturnType {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }
        for (_, func) in analysis.local_functions() {
            if func.explicit_void_return { continue; }
            // Functions materialized from `fun(...)` types have no source to report.
            let Some(func_node_id) = func.def_node.node_id else { continue };
            report_unknown_return_annotations(func, SyntaxNode { tree, id: func_node_id }, diags);
            for &ret_sym_idx in &func.rets {
                let sym = analysis.sym(ret_sym_idx);
                let SymbolIdentifier::FunctionRet(_, ret_index) = &sym.id else { continue };
                // Every `return` expression is checked, including under an `@return`
                // annotation: the annotation types the function, not the value returned.
                for ver in &sym.versions {
                    if ver.type_source.is_none() { continue; }
                    let Some(desc) = unknown_type_message(ver.resolved_type.as_ref(), false) else { continue };
                    report_return_expr(tree, ver, *ret_index, desc, diags);
                }
            }
        }
        // A file-level `return x ---@type T` is typed by its annotation.
        for (_, sym) in analysis.local_symbols() {
            if !matches!(sym.id, SymbolIdentifier::FileReturn) { continue; }
            let Some(ver) = sym.versions.first() else { continue };
            let Some(desc) = unknown_type_message(ver.resolved_type.as_ref(), true) else { continue };
            report_return_expr(tree, ver, 0, desc, diags);
        }
    }
}

/// Report the value a `return` statement (the version's def node) yields at `ret_index`.
fn report_return_expr(tree: &SyntaxTree, ver: &SymbolVersion, ret_index: usize, desc: &str, diags: &mut Vec<WowDiagnostic>) {
    let Some(node_id) = ver.def_node.node_id else { return };
    let Some(ret_stmt) = Return::cast(SyntaxNode { tree, id: node_id }) else { return };
    let Some(expr_list) = ret_stmt.expression_list() else { return };
    let expressions = expr_list.expressions();
    let Some(expr_node) = expressions.get(ret_index).or(expressions.last()) else { return };
    let start = u32::from(expr_node.syntax().text_range().start());
    let end = crate::analysis::build_ir::trimmed_node_end(expr_node.syntax());
    super::UNKNOWN_RETURN_TYPE.emit(diags, format!("return value {}", desc), start as usize, end as usize);
}

/// Report `@return` slots declared with an unknown type (`---@return any`), on the
/// slot's `---@return` line when there's one line per slot, else the first one (or
/// the `function` keyword, when the annotations precede an enclosing statement).
fn report_unknown_return_annotations(func: &Function, func_node: SyntaxNode<'_>, diags: &mut Vec<WowDiagnostic>) {
    let return_lines: Vec<(usize, usize)> = crate::analysis::Analysis::collect_preceding_annotation_ranges(func_node)
        .into_iter()
        .filter(|(text, _, _)| {
            let tag = text.strip_prefix("---@return").or_else(|| text.strip_prefix("--- @return"));
            tag.is_some_and(|rest| rest.starts_with([' ', '\t']))
        })
        .map(|(_, s, e)| (s, e))
        .collect();
    for (i, annotation) in func.return_annotations.iter().enumerate() {
        let Some(desc) = unknown_type_message(Some(annotation), true) else { continue };
        let line = if return_lines.len() == func.return_annotations.len() { return_lines.get(i) } else { return_lines.first() };
        let keyword = func_node.first_token().map(|t| {
            let r = t.text_range();
            (u32::from(r.start()) as usize, u32::from(r.end()) as usize)
        });
        let Some((start, end)) = line.copied().or(keyword) else { continue };
        let label = match func.return_labels.get(i).cloned().flatten() {
            Some(name) => format!("return value '{}'", name),
            None => format!("return value {}", i + 1),
        };
        super::UNKNOWN_RETURN_TYPE.emit(diags, format!("{} {}", label, desc), start, end);
    }
}
