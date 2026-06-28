use std::collections::HashSet;

use crate::analysis::AnalysisResult;
use crate::annotations::annotation_scanning::ExternalGlobalKind;
use crate::ast::{AstNode, FunctionDefinition, Return};
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use crate::types::{ScopeIndex, SymbolIdentifier};
use super::{DiagnosticPass, WowDiagnostic};

/// Flags functions that are reachable beyond the file they're defined in
/// (global functions and methods/fields on cross-file tables) and lack
/// `@param`/`@return` annotations. Unlike `incomplete-signature-doc` — which
/// fires only on a *partially* annotated signature regardless of where the
/// function lives — these two codes fire even when the function has *no*
/// annotations at all, and are deliberately scoped to non-file-local functions
/// so day-to-day helper closures and `local function`s aren't nagged.
///
/// Both codes are HINT severity and off by default.
pub struct MissingAnnotations;

impl DiagnosticPass for MissingAnnotations {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }

        let sentinel = crate::annotations::AnnotationType::Simple(String::new());
        let root = SyntaxNode::new_root(tree);
        // Byte offsets of method/field definitions that escape this file (built
        // lazily on the first dotted/colon candidate). See `escaping_methods`.
        let mut escaping: Option<HashSet<u32>> = None;

        for (_func_idx, func) in analysis.local_functions() {
            let Some(nid) = func.def_node.node_id else { continue };

            let func_node = SyntaxNode { tree, id: nid };
            let Some(func_def) = FunctionDefinition::cast(func_node) else { continue };

            // Scope: only functions visible outside their defining file.
            if func_def.is_local() { continue; }
            let Some(names) = def_name_chain(&func_def) else { continue };
            let file_local = match names.len() {
                0 => true,
                // Dotted/colon definition: a method or namespace field. File-local
                // unless the receiver table escapes the file (global, `@class`, or
                // a local table attached to the addon namespace) — determined by
                // whether the workspace global scan registers it.
                n if n >= 2 => {
                    let set = escaping.get_or_insert_with(|| escaping_methods(root));
                    !set.contains(&func.def_node.start)
                }
                // Bare `function foo()`: a Lua global unless `foo` rebinds a
                // forward-declared local.
                _ => bare_name_is_file_local(analysis, tree, func.scope, &names[0]),
            };
            if file_local { continue; }

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

            // A colon-method definition injects `self` at arg position 0, which
            // never needs a `@param`; shift the source params past it.
            let self_injected = func.args.len() == src_params.len() + 1
                && matches!(&analysis.ir.sym(func.args[0]).id,
                    SymbolIdentifier::Name(n) if n == "self");
            let arg_offset = if self_injected { 1 } else { 0 };

            for (i, (name, pstart, pend)) in src_params.iter().enumerate() {
                // `self` and `_` are conventionally never annotated.
                if name == "self" || name == "_" { continue; }
                let arg_i = i + arg_offset;
                if arg_i >= func.args.len() { break; }
                let annotated = func.param_annotations.get(arg_i)
                    .is_some_and(|a| a != &sentinel);
                if annotated { continue; }
                super::MISSING_PARAM_ANNOTATION.emit(
                    diags,
                    format!("parameter `{}` has no `@param` annotation", name),
                    *pstart as usize,
                    *pend as usize,
                );
            }
            if let Some((vstart, vend)) = vararg_range
                && func.vararg_annotation.is_none()
            {
                super::MISSING_PARAM_ANNOTATION.emit(
                    diags,
                    "vararg `...` has no `@param ...` annotation".to_string(),
                    vstart as usize,
                    vend as usize,
                );
            }

            // Return: only flag when the body actually yields a value and there's
            // no `@return` (nor an implicit `self`/`built` return).
            let has_return_ann = !func.return_annotations.is_empty()
                || func.returns_self
                || func.returns_built;
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
            super::MISSING_RETURN_ANNOTATION.emit(
                diags,
                "function returns a value but has no `@return` annotation".to_string(),
                u32::from(kw_range.start()) as usize,
                u32::from(kw_range.end()) as usize,
            );
        }
    }
}

/// Recover a function definition's name chain (`foo`, or `A.B`, `A:B`, …).
/// Returns `None` for an anonymous function literal.
fn def_name_chain(func_def: &FunctionDefinition<'_>) -> Option<Vec<String>> {
    if let Some(ident) = func_def.identifier() {
        Some(ident.names())
    } else {
        func_def.name().map(|name| vec![name])
    }
}

/// Byte offsets of every method/field definition in the file that escapes it,
/// reusing the per-file global-scan function (`scan_file_globals`) as the
/// source of truth: a method is registered as a cross-file symbol exactly when
/// its receiver is a global, a `@class`, the addon namespace, or a local table
/// assigned onto the addon namespace (`ns.Field = LocalTable`). Anything else
/// is purely file-private.
///
/// Note: this calls `scan_file_globals(root, None)` — `source_path` is `None`,
/// so `addon_root` boundaries are not resolved. In a multi-addon workspace with
/// `addon_root: true`, the per-file rescan may classify a method differently
/// than the aggregated workspace scanner (which knows each file's addon root).
/// This is acceptable because the diagnostic is off by default and the
/// disagreement only affects edge cases at addon-root boundaries.
fn escaping_methods(root: SyntaxNode<'_>) -> HashSet<u32> {
    crate::annotations::scan_file_globals(root, None)
        .into_iter()
        .filter(|g| matches!(g.kind, ExternalGlobalKind::Method(..)))
        .map(|g| g.def_start)
        .collect()
}

/// Returns `true` when a bare `function foo()` is local to its file — i.e. `foo`
/// rebinds a forward-declared `local foo`. A genuine global definition (or an
/// unresolved name) is not file-local.
///
/// `func_scope` is the function's body scope; its parent is the scope the name
/// is bound in.
fn bare_name_is_file_local(
    analysis: &AnalysisResult,
    tree: &SyntaxTree,
    func_scope: ScopeIndex,
    name: &str,
) -> bool {
    let enclosing = analysis.ir.try_scope(func_scope)
        .and_then(|s| s.parent)
        .unwrap_or(ScopeIndex(0));
    let Some(sym_idx) = analysis.ir
        .get_symbol(&SymbolIdentifier::Name(name.to_string()), enclosing)
    else {
        // Unresolved — assume a global rather than suppress.
        return false;
    };
    if sym_idx.is_external() { return false; }
    match analysis.ir.sym(sym_idx).versions.first() {
        Some(v0) => analysis.is_local_declaration_site(tree, v0.def_node.start),
        None => false,
    }
}
