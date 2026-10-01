use crate::analysis::{is_unknown_type, AnalysisResult};
use crate::ast::{AstNode, ForInLoop};
use crate::collections::HashSet;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxKind, SyntaxNode, TextRange};
use crate::types::{Expr, Symbol, SymbolIdentifier, SymbolIndex, SymbolVersion, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub struct UnknownLocalType;

/// Message tail for an unknown type (`"has an unknown type"`, `` "has type `any`" ``,
/// …), or `None` when `t` is a known type.
pub(super) fn unknown_type_message(t: Option<&ValueType>, annotated: bool) -> Option<&'static str> {
    if !is_unknown_type(t) { return None; }
    Some(match (t, annotated) {
        (None, _) => "has an unknown type",
        (Some(ValueType::Union(_)), true) => "is annotated `any?`",
        (Some(ValueType::Union(_)), false) => "has type `any?`",
        (Some(_), true) => "is annotated `any`",
        (Some(_), false) => "has type `any`",
    })
}

/// The declaration/assignment sites of `sym` whose value is unknown, as
/// `(version index, name range, message tail)`. Narrowing and branch/loop-merge
/// versions copy an earlier version's `def_node`, so only the first version at each
/// def site is a real write — except `for` variables, where `for _, a, _, b in`
/// binds `_` twice. `decl_annotated` marks version 0 as typed by an annotation.
pub(super) fn unknown_writes(
    analysis: &AnalysisResult,
    tree: &SyntaxTree,
    sym: &Symbol,
    name: &str,
    decl_annotated: bool,
) -> Vec<(usize, TextRange, &'static str)> {
    let mut seen: HashSet<u32> = HashSet::default();
    let mut out = Vec::new();
    for (i, ver) in sym.versions.iter().enumerate() {
        let for_var_index = ver.type_source.and_then(|ts| match analysis.expr(ts) {
            Expr::ForInVar { var_index, .. } => Some(*var_index),
            _ => None,
        });
        if !seen.insert(ver.def_node.start) && for_var_index.is_none() { continue; }
        let Some(desc) = unknown_type_message(ver.resolved_type.as_ref(), i == 0 && decl_annotated) else { continue };
        // A forward declaration (`local x`) is only unknown when nothing is ever
        // assigned to it; otherwise the assignments are the sites to check. A
        // narrowing of the declaration shares its def site, so it isn't an assignment.
        if ver.type_source.is_none()
            && sym.versions[i + 1..].iter().any(|v| v.def_node.start != ver.def_node.start)
        {
            continue;
        }
        let range = match for_var_index {
            Some(var_index) => for_var_range(tree, ver, var_index),
            None => analysis.def_name_token_range(tree, ver.def_node.start, ver.def_node.end, name),
        };
        let Some(range) = range else { continue };
        out.push((i, range, desc));
    }
    out
}

/// The `var_index`-th name of the `for … in` loop a `for` variable version belongs to.
fn for_var_range(tree: &SyntaxTree, ver: &SymbolVersion, var_index: usize) -> Option<TextRange> {
    let for_in = ForInLoop::cast(SyntaxNode { tree, id: ver.def_node.node_id? })?;
    Some(for_in.name_list()?.name_tokens().get(var_index)?.text_range())
}

impl DiagnosticPass for UnknownLocalType {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }
        let param_syms: HashSet<SymbolIndex> = analysis.local_functions()
            .flat_map(|(_, f)| f.args.iter().copied())
            .collect();
        for (sym_idx, sym) in analysis.local_symbols() {
            if param_syms.contains(&sym_idx) { continue; }
            let SymbolIdentifier::Name(name) = &sym.id else { continue };
            let Some(v0) = sym.versions.first() else { continue };
            // Every other variable is a local declaration, a `for` variable, or an
            // implicit global (an assignment with no local in scope).
            let is_for_var = v0.def_node.node_id.is_some_and(|id| {
                matches!(SyntaxNode { tree, id }.kind(), SyntaxKind::ForInLoop | SyntaxKind::ForCountLoop)
            });
            let label = if is_for_var || analysis.is_local_declaration_site(tree, v0.def_node.start) {
                "local"
            } else {
                "global"
            };
            let decl_annotated = analysis.ir.symbol_type_annotations.contains_key(&sym_idx);
            for (_, range, desc) in unknown_writes(analysis, tree, sym, name, decl_annotated) {
                super::UNKNOWN_LOCAL_TYPE.emit(
                    diags,
                    format!("{} '{}' {}", label, name, desc),
                    u32::from(range.start()) as usize,
                    u32::from(range.end()) as usize,
                );
            }
        }
    }
}
