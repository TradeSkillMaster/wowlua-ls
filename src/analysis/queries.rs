use std::collections::{BTreeMap, HashMap, HashSet};

use crate::types::*;
use super::{AnalysisResult, Ir};
use crate::syntax::SyntaxKind;
use crate::syntax::tree::{SyntaxTree, TokenId};
use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken, TextSize, TextRange, TokenAtOffset};
use crate::ast::{AstNode, Expression, ForInLoop, FunctionCall, FunctionDefinition, Identifier, LocalAssign, Operator};

/// JSON data key: byte offset where the completion's text_edit range starts.
pub const DATA_REPLACE_START: &str = "replace_start";
/// JSON data key: byte offset where the completion's text_edit range ends.
/// When absent, the LSP handler uses the cursor position as the range end.
pub const DATA_REPLACE_END: &str = "replace_end";

/// All Lua reserved keywords, used for keyword completions in scope context.
const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for",
    "function", "if", "in", "local", "nil", "not", "or", "repeat",
    "return", "then", "true", "until", "while",
];

enum AnnotationContext {
    Function,
    Class,
    Any,
}

fn enclose_range(outer: DefNode, inner: DefNode) -> DefNode {
    DefNode {
        start: outer.start.min(inner.start),
        end: outer.end.max(inner.end),
        node_id: outer.node_id,
    }
}

/// Extract the header text for a control flow block (e.g. "if x > 5", "while running").
/// Walks tokens from the start of the node until the stop keyword (ThenKeyword/DoKeyword).
fn extract_block_header(node: &SyntaxNode<'_>, stop_kind: SyntaxKind) -> String {
    let mut parts = Vec::new();
    for item in node.children_with_tokens() {
        match item {
            NodeOrToken::Token(tok) => {
                let k = tok.kind();
                if k == stop_kind || k == SyntaxKind::EndKeyword { break; }
                if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline { continue; }
                parts.push(tok.text().to_string());
            }
            NodeOrToken::Node(child) => {
                // Inline the text of child nodes (e.g. Condition, NameList, ExpressionList)
                for tok in child.descendants_with_tokens() {
                    if let NodeOrToken::Token(tok) = tok {
                        let k = tok.kind();
                        if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline { continue; }
                        parts.push(tok.text().to_string());
                    }
                }
            }
        }
    }
    let header = parts.join(" ");
    if header.len() > 80 {
        // Truncate at a char boundary to avoid panicking on multi-byte UTF-8
        let cut = header.floor_char_boundary(77);
        format!("{}...", &header[..cut])
    } else {
        header
    }
}

/// Create a DefNode covering just the first keyword token of a block node.
fn keyword_def_node(node: &SyntaxNode<'_>) -> DefNode {
    if let Some(tok) = node.first_token() {
        let r = tok.text_range();
        DefNode { start: u32::from(r.start()), end: u32::from(r.end()), node_id: None }
    } else {
        DefNode::from_node(*node)
    }
}

/// Check if a node spans multiple lines in the source.
fn is_multiline(node: &SyntaxNode<'_>, source: &str) -> bool {
    let range = node.text_range();
    let start = u32::from(range.start()) as usize;
    let end = u32::from(range.end()) as usize;
    source[start..end].contains('\n')
}

/// Create a Block document symbol entry from a node with the given name.
/// Finds the Block child and recursively collects nested symbols.
fn make_block_entry(
    analysis: &AnalysisResult,
    node: SyntaxNode<'_>,
    name: String,
    tree: &SyntaxTree,
    func_map: &HashMap<u32, FunctionIndex>,
) -> DocumentSymbolEntry {
    let def_node = DefNode::from_node(node);
    let sel = keyword_def_node(&node);
    let children = node.children()
        .find(|c| c.kind() == SyntaxKind::Block)
        .map(|body| analysis.collect_block_symbols(body, tree, func_map))
        .unwrap_or_default();
    DocumentSymbolEntry {
        name,
        detail: None,
        kind: DocumentSymbolKind::Block,
        range: def_node,
        selection_range: sel,
        children,
        deprecated: false,
    }
}

/// Recursively sort document symbol entries by file position.
fn sort_entries_recursive(entries: &mut [DocumentSymbolEntry]) {
    entries.sort_by_key(|s| s.range.start);
    for s in entries.iter_mut() {
        sort_entries_recursive(&mut s.children);
    }
}

/// Recursively extend each entry's range to encompass all children's ranges.
/// This is required for VS Code sticky scroll: the parent range must contain
/// children positions so the editor knows the cursor is "inside" the parent.
fn extend_ranges_to_children(entries: &mut [DocumentSymbolEntry]) {
    for entry in entries.iter_mut() {
        extend_ranges_to_children(&mut entry.children);
        for child in &entry.children {
            if child.range.end > entry.range.end {
                entry.range.end = child.range.end;
            }
            if child.range.start < entry.range.start {
                entry.range.start = child.range.start;
            }
        }
    }
}

fn collect_type_name_completions<'a>(
    names: impl Iterator<Item = &'a String>,
    prefix: &str,
    kind: lsp_types::CompletionItemKind,
    seen: &mut HashSet<String>,
    items: &mut Vec<lsp_types::CompletionItem>,
) {
    for name in names {
        if name.starts_with(prefix) && seen.insert(name.clone()) {
            items.push(lsp_types::CompletionItem {
                label: name.clone(),
                kind: Some(kind),
                ..lsp_types::CompletionItem::default()
            });
        }
    }
}

// ── Shared free functions (used by both Analysis and AnalysisResult) ─────────

/// Union the resolved types of every `FunctionRet` symbol in `rets` whose
/// slot index matches `slot`. Returns `None` if no matching symbol has a
/// resolved type yet (e.g. mid-fixpoint, or no returns exist for that slot).
///
/// Each `return` statement registers its own `FunctionRet` symbol at the
/// scope it lives in, so a function with branched returns has multiple
/// symbols sharing the same `(func_idx, slot)` id. The call-site resolver
/// in `resolve.rs` and `dedup_return_types` (below) both walk `func.rets`
/// to collect every contribution.
pub(super) fn return_type_at_slot(ir: &Ir, rets: &[SymbolIndex], slot: usize) -> Option<ValueType> {
    let mut acc: Option<ValueType> = None;
    for &sym_idx in rets {
        if let SymbolIdentifier::FunctionRet(_, idx) = &ir.sym(sym_idx).id {
            if *idx != slot { continue; }
            if let Some(vt) = ir.sym(sym_idx).versions.first()
                .and_then(|v| v.resolved_type.as_ref())
            {
                acc = Some(match acc.take() {
                    Some(prev) => ir.dedupe_union_tables(ValueType::make_union(vec![prev, vt.clone()])),
                    None => vt.clone(),
                });
            }
        }
    }
    acc
}

/// Deduplicate `func.rets` by return position and union the resolved types.
/// Multiple `return` statements in different scopes create separate symbols for
/// the same position in `func.rets`. This function groups them by index and
/// returns one type per position (the union of all matching symbols' types).
fn dedup_return_types(ir: &Ir, rets: &[SymbolIndex]) -> Vec<Option<ValueType>> {
    let mut by_index: BTreeMap<usize, Option<ValueType>> = BTreeMap::new();
    for &sym_idx in rets {
        if let SymbolIdentifier::FunctionRet(_, index) = &ir.sym(sym_idx).id {
            by_index.entry(*index).or_insert(None);
        }
    }
    for slot in by_index.keys().cloned().collect::<Vec<_>>() {
        let vt = return_type_at_slot(ir, rets, slot);
        by_index.insert(slot, vt);
    }
    by_index.into_values().collect()
}

/// Maximum recursion depth for read-only expression resolution.
const MAX_QUERY_RESOLVE_DEPTH: usize = 200;

/// Shared implementation for read-only expression type resolution.
/// Both `Analysis::resolve_expr_type` and `AnalysisResult::resolve_expr_type` delegate here.
pub(super) fn resolve_expr_type_impl(
    ir: &Ir,
    resolved_expr_cache: &[Option<ValueType>],
    expr_id: ExprId,
    visited: &mut HashSet<ExprId>,
    depth: usize,
) -> Option<ValueType> {
    // Check Phase 2 resolve cache first — builder chains (@builds-field / @built-name /
    // @return self) are resolved during the fixpoint loop and the result is cached here.
    // The read-only resolver can't replicate the mutable table-cloning logic, so we
    // rely on the cached result for these expressions.
    if let Some(cached) = resolved_expr_cache.get(expr_id.val()).and_then(|v| v.as_ref()) {
        return Some(cached.clone());
    }
    // Depth limit: prevent stack overflow on deeply nested chains
    if depth >= MAX_QUERY_RESOLVE_DEPTH {
        return None;
    }
    // External exprs (>= EXT_BASE) are immutable/shared and can legitimately appear
    // multiple times in method chains (e.g. repeated :AddField() calls on the same class).
    // Only track local exprs for cycle detection.
    if !expr_id.is_external() && !visited.insert(expr_id) {
        return None;
    }
    match ir.expr(expr_id) {
        Expr::Literal(vt) => Some(vt.clone()),
        Expr::SymbolRef(sym_idx, ver_idx) => {
            let sym = ir.sym(*sym_idx);
            sym.versions[*ver_idx].resolved_type.clone()
        }
        Expr::FunctionDef(func_idx) => {
            Some(ValueType::Function(Some(*func_idx)))
        }
        Expr::TableConstructor(table_idx) => {
            Some(ValueType::Table(Some(*table_idx)))
        }
        Expr::Grouped(inner) => resolve_expr_type_impl(ir, resolved_expr_cache, *inner, visited, depth + 1),
        Expr::BinaryOp { op, lhs, rhs } => {
            let (op, lhs, rhs) = (*op, *lhs, *rhs);
            let lhs_type = resolve_expr_type_impl(ir, resolved_expr_cache, lhs, visited, depth + 1);
            let rhs_type = resolve_expr_type_impl(ir, resolved_expr_cache, rhs, visited, depth + 1);
            match (lhs_type, rhs_type) {
                (Some(l), Some(r)) => super::resolve::resolve_binary_op_standalone(op, l, r),
                (Some(ValueType::Number), None) | (None, Some(ValueType::Number))
                    if op.is_arithmetic() => Some(ValueType::Number),
                (Some(ref t), None) | (None, Some(ref t))
                    if op == Operator::Concatenate && t.can_concat_to_string() => Some(ValueType::String(None)),
                _ if op.is_comparison() => Some(ValueType::Boolean(None)),
                _ => None,
            }
        }
        Expr::UnaryOp { op, operand } => {
            let (op, operand) = (*op, *operand);
            let operand_type = resolve_expr_type_impl(ir, resolved_expr_cache, operand, visited, depth + 1)?;
            match op {
                Operator::Not => Some(ValueType::Boolean(None)),
                Operator::Subtract => {
                    match &operand_type {
                        ValueType::Number => Some(ValueType::Number),
                        _ => None,
                    }
                }
                Operator::ArrayLength => Some(ValueType::Number),
                _ => None,
            }
        }
        Expr::FieldAccess { table, field, .. } => {
            let table = *table;
            let field = field.clone();
            let table_type = resolve_expr_type_impl(ir, resolved_expr_cache, table, visited, depth + 1)?;
            let table_type = table_type.into_strip_opaque();
            let table_indices: Vec<TableIndex> = match &table_type {
                ValueType::Table(Some(idx)) => vec![*idx],
                ValueType::Intersection(types) => types.iter().filter_map(|t| match t {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                }).collect(),
                ValueType::Union(types) => types.iter().flat_map(|t| match t {
                    ValueType::Table(Some(idx)) => vec![*idx],
                    ValueType::Intersection(itypes) => itypes.iter().filter_map(|it| match it {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }).collect(),
                    _ => vec![],
                }).collect(),
                _ => return None,
            };
            // Try each table in the union for the field, including parent classes
            let mut field_types: Vec<ValueType> = Vec::new();
            for &idx in &table_indices {
                if let Some(fi) = ir.get_field(idx, &field) {
                    let primary = fi.expr;
                    let extras: Vec<ExprId> = fi.extra_exprs.clone();
                    let annotation = fi.annotation.clone();
                    if let Some(ann) = annotation {
                        if !field_types.contains(&ann) {
                            field_types.push(ann);
                        }
                    } else {
                        // Skip nil primary when there are reassignments
                        let skip_primary = !extras.is_empty()
                            && matches!(resolve_expr_type_impl(ir, resolved_expr_cache, primary, visited, depth + 1), Some(ValueType::Nil));
                        let all_exprs: Vec<ExprId> = if skip_primary {
                            extras
                        } else {
                            std::iter::once(primary).chain(extras).collect()
                        };
                        let mut has_unresolvable = false;
                        for eid in all_exprs {
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, eid, visited, depth + 1) {
                                if !field_types.contains(&vt) {
                                    field_types.push(vt);
                                }
                            } else {
                                has_unresolvable = true;
                            }
                        }
                        // If the primary was a nil placeholder (skipped) and
                        // any reassignment couldn't be resolved, the field
                        // could hold any type — widen to Any.
                        if has_unresolvable && skip_primary
                            && !field_types.contains(&ValueType::Any)
                        {
                            field_types.push(ValueType::Any);
                        }
                    }
                    // If own field resolved only to Table(None) placeholders and the
                    // table is a class, fall through to parent class check for a better type.
                    // (Mirrors the same guard in resolve.rs FieldAccess and
                    // queries.rs resolve_field_or_g_env.)
                    if !field_types.is_empty()
                        && (!field_types.iter().all(|vt| matches!(vt, ValueType::Table(None)))
                            || ir.table(idx).class_name.is_none())
                    {
                        continue;
                    }
                }
                // Check parent classes
                for &parent_idx in &ir.table(idx).parent_classes {
                    if let Some(fi) = ir.get_field(parent_idx, &field) {
                        if let Some(ref ann) = fi.annotation {
                            if !matches!(ann, ValueType::Any | ValueType::Table(None))
                                && !field_types.contains(ann) {
                                field_types.push(ann.clone());
                            }
                        } else {
                            let expr = fi.expr;
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, expr, visited, depth + 1)
                                && !matches!(vt, ValueType::Any | ValueType::Table(None))
                                && !field_types.contains(&vt) {
                                    field_types.push(vt);
                                }
                        }
                        break;
                    }
                }
            }
            if field_types.is_empty() { return None; }
            Some(ValueType::make_union(field_types))
        }
        Expr::FunctionCall { func, ret_index, .. } => {
            let func = *func;
            let ret_index = *ret_index;
            let func_type = resolve_expr_type_impl(ir, resolved_expr_cache, func, visited, depth + 1)?;
            let func_type = func_type.into_strip_opaque();
            let func_idx = match func_type {
                ValueType::Function(Some(idx)) => idx,
                ValueType::Table(Some(table_idx)) => {
                    ir.table(table_idx).call_func?
                }
                _ => return None,
            };
            let func_info = ir.func(func_idx);
            // Handle @return self
            if func_info.returns_self && ret_index == 0
                && let Expr::FieldAccess { table: receiver_expr, .. } = ir.expr(func).clone()
                    && let Some(rt) = resolve_expr_type_impl(ir, resolved_expr_cache, receiver_expr, visited, depth + 1) {
                        return Some(rt);
                    }
            // Handle @return built: return the accumulated built_table from the receiver
            if func_info.returns_built && ret_index == 0
                && let Expr::FieldAccess { table: receiver_expr, .. } = ir.expr(func).clone()
                    && let Some(ValueType::Table(Some(recv_idx))) = resolve_expr_type_impl(ir, resolved_expr_cache, receiver_expr, visited, depth + 1) {
                        if let Some(built_idx) = ir.table(recv_idx).built_table {
                            return Some(ValueType::Table(Some(built_idx)));
                        }
                        return Some(ValueType::Table(None));
                    }
            let ret_id = SymbolIdentifier::FunctionRet(func_idx, ret_index);
            let ret_sym_idx = ir.get_symbol(&ret_id, func_info.scope)?;
            ir.sym(ret_sym_idx).versions.first()?.resolved_type.clone()
        }
        Expr::BracketIndex { table, .. } => {
            let table = *table;
            let table_type = resolve_expr_type_impl(ir, resolved_expr_cache, table, visited, depth + 1)?;
            let table_type = table_type.into_strip_opaque();
            match &table_type {
                ValueType::Table(Some(idx)) => ir.table(*idx).value_type.clone(),
                ValueType::Union(types) => {
                    if types.iter().any(|t| matches!(t, ValueType::Table(None))) {
                        return Some(ValueType::Any);
                    }
                    let mut vts: Vec<ValueType> = Vec::new();
                    for t in types {
                        if let ValueType::Table(Some(idx)) = t
                            && let Some(vt) = &ir.table(*idx).value_type
                                && !vts.contains(vt) { vts.push(vt.clone()); }
                    }
                    if vts.is_empty() { None } else { Some(ValueType::make_union(vts)) }
                }
                ValueType::Table(None) => Some(ValueType::Any),
                _ => None,
            }
        }
        Expr::VarArgs(ret_index, file_level) => {
            if *file_level {
                match ret_index {
                    0 => Some(ValueType::String(None)),
                    1 => {
                        ir.addon_table_idx().map(|idx| ValueType::Table(Some(idx)))
                    }
                    _ => Some(ValueType::Nil),
                }
            } else {
                None
            }
        }
        Expr::BranchMerge(exprs) => {
            let exprs = exprs.clone();
            let mut types: Vec<ValueType> = Vec::new();
            for eid in exprs {
                if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, eid, visited, depth + 1) {
                    types.push(vt);
                }
            }
            if types.is_empty() { None } else { Some(ValueType::make_union(types)) }
        }
        Expr::StripNil(inner) => {
            let inner = *inner;
            match resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1).map(|vt| vt.strip_nil()) {
                Some(ValueType::Union(ref members)) if members.is_empty() => None,
                other => other,
            }
        }
        Expr::StripFalsy(inner) => {
            let inner = *inner;
            match resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1).map(|vt| vt.strip_falsy()) {
                Some(ValueType::Union(ref members)) if members.is_empty() => None,
                other => other,
            }
        }
        Expr::CastAdd(inner, cast_type) => {
            let inner = *inner;
            let cast_type = cast_type.clone();
            resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1)
                .map(|vt| ValueType::union(vt, cast_type))
        }
        Expr::CastRemove(inner, cast_type) => {
            let inner = *inner;
            let cast_type = cast_type.clone();
            resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1)
                .map(|vt| vt.strip_type_with(&cast_type, &|idx| ir.table(idx).enum_kind))
        }
        _ => None,
    }
}

/// Format a single return annotation, prefixing `...` if it's the last entry and vararg.
fn format_vararg_return(formatted: String, index: usize, func: &Function) -> String {
    if index == func.return_annotations.len() - 1 && func.has_vararg_return {
        if formatted.starts_with("...") {
            formatted
        } else {
            format!("...{}", formatted)
        }
    } else if is_intersection_of_varargs_raw(func, index) {
        format!("& {}", formatted)
    } else {
        formatted
    }
}

/// Check whether a return annotation at `index` was written as `& ...M`
/// (intersection-of-varargs).  The raw annotation is `Intersection([VarArgs(_)])`
/// — a single-element intersection wrapping a VarArgs.
fn is_intersection_of_varargs_raw(func: &Function, index: usize) -> bool {
    func.return_annotations_raw.get(index).is_some_and(|raw| {
        matches!(raw, crate::annotations::AnnotationType::Intersection(parts) if parts.len() == 1 && matches!(&parts[0], crate::annotations::AnnotationType::VarArgs(_)))
    })
}

/// Format a vararg parameter for display.  When the type annotation already
/// starts with `...` (e.g. `...M` from a variadic generic), the name `...` is
/// redundant so we show just the type.  Otherwise show `...: type`.
fn format_vararg_param(ann: &crate::annotations::AnnotationType) -> String {
    let type_text = crate::annotations::format_annotation_type(ann);
    if type_text.starts_with("...") {
        type_text
    } else {
        format!("...: {}", type_text)
    }
}

// ── LSP Queries ──────────────────────────────────────────────────────────────

/// Cross-file-stable identity of the thing at a cursor position, produced by
/// `AnalysisResult::reference_target_at` and consumed by
/// `AnalysisResult::references_for_target` to drive workspace-wide find-references.
///
/// When the inner index is `>= EXT_BASE`, the target refers to a shared entity in
/// `PreResolvedGlobals` and is meaningful to any `AnalysisResult` built from the
/// same `PreResolvedGlobals`. When the index is `< EXT_BASE`, the target is
/// file-local (only meaningful to the `AnalysisResult` that produced it).
#[derive(Debug, Clone)]
pub enum ReferenceTarget {
    /// A symbol (local or global). `idx >= EXT_BASE` means the symbol is a
    /// workspace-wide global and references can be found in any file.
    Symbol { idx: SymbolIndex, name: String },
    /// A field on a table. `table_idx >= EXT_BASE` means the table is
    /// workspace-wide (stub, `@class`, or addon namespace) and references can
    /// be found in any file.
    Field { table_idx: TableIndex, field_name: String },
}

impl ReferenceTarget {
    /// Whether the target refers to something visible across files (a global
    /// symbol or a field on an `EXT_BASE+` table).
    pub fn is_cross_file(&self) -> bool {
        match self {
            ReferenceTarget::Symbol { idx, .. } => idx.is_external(),
            ReferenceTarget::Field { table_idx, .. } => table_idx.is_external(),
        }
    }

    /// The name token text for the target (symbol name or field name). Used to
    /// cheaply skip files whose text doesn't contain the name at all.
    pub fn name(&self) -> &str {
        match self {
            ReferenceTarget::Symbol { name, .. } => name.as_str(),
            ReferenceTarget::Field { field_name, .. } => field_name.as_str(),
        }
    }
}

/// Context for an expression string argument at a given offset.
struct ExpressionStringContext {
    /// Table indices whose fields are the expression's variables.
    table_idxs: Vec<TableIndex>,
    /// Byte offset in the file where the string content starts (after opening delimiter).
    content_start: u32,
    /// The raw expression string content (without delimiters).
    content: String,
}

// ─── Control-flow document highlight helpers ────────────────────────────────

/// Kind returned by [`AnalysisResult::document_highlights_at`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HighlightKind {
    /// Normal textual reference (read or unknown).
    Text,
    /// Control-flow write effect (`return` or `break`).
    Write,
}

/// Node kinds that introduce a new loop scope (used to stop `break` collection).
const LOOP_KINDS: &[SyntaxKind] = &[
    SyntaxKind::ForCountLoop, SyntaxKind::ForInLoop,
    SyntaxKind::WhileLoop, SyntaxKind::RepeatUntilLoop,
];

/// Stop kinds for `break` collection: nested loops AND nested functions.
const BREAK_STOP_KINDS: &[SyntaxKind] = &[
    SyntaxKind::ForCountLoop, SyntaxKind::ForInLoop,
    SyntaxKind::WhileLoop, SyntaxKind::RepeatUntilLoop,
    SyntaxKind::FunctionDefinition,
];

/// Collect all tokens of kind `target` that are descendants of `node`, without
/// recursing into child nodes whose kind is listed in `stop_kinds`.
/// Used to gather `return`/`break` tokens without crossing function or loop
/// boundaries.
fn collect_cf_tokens<'a>(
    node: SyntaxNode<'a>,
    target: SyntaxKind,
    stop_kinds: &[SyntaxKind],
    out: &mut Vec<SyntaxToken<'a>>,
) {
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == target => out.push(t),
            NodeOrToken::Token(_) => {}
            NodeOrToken::Node(n) if stop_kinds.contains(&n.kind()) => {}
            NodeOrToken::Node(n) => collect_cf_tokens(n, target, stop_kinds, out),
        }
    }
}

/// Return the first direct-child token of `node` with the given kind.
fn first_direct_token(node: SyntaxNode<'_>, kind: SyntaxKind) -> Option<SyntaxToken<'_>> {
    node.children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == kind)
}

/// Collect all direct-child tokens whose kind is in `kinds` as `Text` highlights.
fn hl_matching_keywords(node: SyntaxNode<'_>, kinds: &[SyntaxKind]) -> Vec<(TextRange, HighlightKind)> {
    let mut out = Vec::new();
    for child in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = child
            && kinds.contains(&t.kind())
        {
            out.push((t.text_range(), HighlightKind::Text));
        }
    }
    out
}

/// Highlight `function` keyword, closing `end`, and all `return` keywords
/// in `fn_node` (not in nested functions).
fn hl_function_returns(fn_node: SyntaxNode<'_>) -> Vec<(TextRange, HighlightKind)> {
    let mut out = Vec::new();
    if let Some(t) = first_direct_token(fn_node, SyntaxKind::FunctionKeyword) {
        out.push((t.text_range(), HighlightKind::Text));
    }
    if let Some(t) = first_direct_token(fn_node, SyntaxKind::EndKeyword) {
        out.push((t.text_range(), HighlightKind::Text));
    }
    let mut returns = Vec::new();
    collect_cf_tokens(fn_node, SyntaxKind::ReturnKeyword,
        &[SyntaxKind::FunctionDefinition], &mut returns);
    for r in returns {
        out.push((r.text_range(), HighlightKind::Write));
    }
    out
}

/// Highlight all keyword tokens in an `if`-chain (`if`, `then`, `elseif`, `else`, `end`).
fn hl_if_chain(chain: SyntaxNode<'_>) -> Vec<(TextRange, HighlightKind)> {
    let mut out = Vec::new();
    for child in chain.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::IfBranch => {
                for tok in n.children_with_tokens().filter_map(|c| c.into_token()) {
                    if matches!(tok.kind(),
                        SyntaxKind::IfKeyword | SyntaxKind::ElseIfKeyword
                        | SyntaxKind::ThenKeyword)
                    {
                        out.push((tok.text_range(), HighlightKind::Text));
                    }
                }
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::ElseBranch => {
                if let Some(kw) = first_direct_token(n, SyntaxKind::ElseKeyword) {
                    out.push((kw.text_range(), HighlightKind::Text));
                }
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::EndKeyword => {
                out.push((t.text_range(), HighlightKind::Text));
            }
            _ => {}
        }
    }
    out
}

/// Highlight all `break` keywords in `loop_node` (not in nested loops or
/// nested functions), plus the loop boundary keywords.
fn hl_break_in_loop(loop_node: SyntaxNode<'_>) -> Vec<(TextRange, HighlightKind)> {
    let mut out = if loop_node.kind() == SyntaxKind::RepeatUntilLoop {
        hl_matching_keywords(loop_node, &[SyntaxKind::RepeatKeyword, SyntaxKind::UntilKeyword])
    } else {
        hl_matching_keywords(loop_node, &[
            SyntaxKind::ForKeyword, SyntaxKind::WhileKeyword,
            SyntaxKind::DoKeyword, SyntaxKind::EndKeyword,
        ])
    };
    let mut breaks = Vec::new();
    collect_cf_tokens(loop_node, SyntaxKind::BreakKeyword, BREAK_STOP_KINDS, &mut breaks);
    for b in breaks {
        out.push((b.text_range(), HighlightKind::Write));
    }
    out
}

// ────────────────────────────────────────────────────────────────────────────

impl AnalysisResult {
    fn is_field_position(tree: &SyntaxTree, offset: u32) -> bool {
        let text_size = TextSize::from(offset);
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(_, right) => right,
            TokenAtOffset::None => return false,
        };
        if token.kind() != SyntaxKind::Name { return false; }
        if let Some(parent) = token.parent() {
            return parent.children_with_tokens()
                .take_while(|sib| sib.as_token().is_none_or(|t| t.text_range().start() < token.text_range().start()))
                .any(|sib| sib.as_token().is_some_and(|t| t.kind() == SyntaxKind::Dot || t.kind() == SyntaxKind::Colon));
        }
        false
    }

    /// Returns true when the token at `offset` is the field name in a `_G.X` DotAccess
    /// whose base resolves to the external `_G` global environment.  Also handles
    /// indirect references like `local g = _G; g.X` by checking whether the base
    /// symbol's resolved type is the global environment table.
    fn is_g_dot_field(&self, tree: &SyntaxTree, offset: u32) -> bool {
        let text_size = TextSize::from(offset);
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(_, right) => right,
            TokenAtOffset::None => return false,
        };
        if token.kind() != SyntaxKind::Name { return false; }
        let parent = match token.parent() {
            Some(p) if p.kind() == SyntaxKind::DotAccess => p,
            _ => return false,
        };
        // Find the base NameRef of this DotAccess
        let base_name_ref = parent.children().find(|c| c.kind() == SyntaxKind::NameRef);
        let base_name = base_name_ref.as_ref()
            .and_then(|nr| nr.children_with_tokens().find_map(|t| t.into_token()))
            .filter(|t| t.kind() == SyntaxKind::Name);
        let Some(base_name) = base_name else { return false; };
        let base_text = base_name.text().to_string();
        let Some(scope_idx) = self.scope_at_offset(text_size) else { return false; };
        // Check if base is literally "_G" and external
        if base_text == "_G" {
            return self.get_symbol(&SymbolIdentifier::Name(base_text), scope_idx)
                .is_some_and(|idx| idx.is_external());
        }
        // Check if base variable's resolved type is the _G table (indirect reference)
        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(base_text), scope_idx) {
            let sym = self.sym(sym_idx);
            if let Some(ValueType::Table(Some(table_idx))) = sym.versions.last().and_then(|v| v.resolved_type.as_ref()) {
                return self.ir.is_global_env(*table_idx);
            }
        }
        false
    }

    pub(crate) fn find_symbol_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(SymbolIndex, String, u32)> {
        let text_size = TextSize::from(offset);
        let is_name_or_param = |k: SyntaxKind| k == SyntaxKind::Name || k == SyntaxKind::Parameter;
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(left, right) => {
                if is_name_or_param(right.kind()) { right }
                else if is_name_or_param(left.kind()) { left }
                else { return None; }
            }
            TokenAtOffset::None => return None,
        };
        if !is_name_or_param(token.kind()) {
            return None;
        }
        let token_start = u32::from(token.text_range().start());
        let name = token.text().to_string();
        let scope_idx = self.scope_at_offset(text_size)?;
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx)?;

        // In `local x = x`, the RHS `x` should resolve to the outer/global
        // binding, not the freshly-defined local. During IR build, the RHS is
        // lowered before the symbol is inserted, but at query time we need to
        // replicate that ordering: if the token is inside the ExpressionList
        // (RHS) of the LocalAssignStatement that defines this symbol, skip it
        // and look in the parent scope.
        if !symbol_idx.is_external()
            && let Some(v) = self.sym(symbol_idx).versions.first()
            && Self::is_in_defining_local_assign_rhs(&token, &v.def_node)
            && let Some(outer) = self.get_symbol_excluding(
                &SymbolIdentifier::Name(name.clone()),
                scope_idx,
                symbol_idx,
            ) {
                return Some((outer, name, token_start));
        }

        Some((symbol_idx, name, token_start))
    }

    /// Returns `true` when `token` sits inside the `ExpressionList` (RHS) of
    /// the specific `LocalAssignStatement` whose byte range matches `def_node`.
    /// Stops the walk at function boundaries so that
    /// `local f = function() f() end` still resolves the recursive `f`.
    fn is_in_defining_local_assign_rhs(token: &SyntaxToken<'_>, def_node: &DefNode) -> bool {
        let mut in_expression_list = false;
        let mut node = token.parent();
        while let Some(n) = node {
            match n.kind() {
                SyntaxKind::ExpressionList => in_expression_list = true,
                SyntaxKind::LocalAssignStatement => {
                    // Only match if this is the SAME statement that defined the symbol
                    let r = n.text_range();
                    return in_expression_list
                        && u32::from(r.start()) == def_node.start
                        && u32::from(r.end()) == def_node.end;
                }
                // Stop at function boundaries: inside a function body
                // the local IS visible (recursive case).
                SyntaxKind::FunctionDefinition => return false,
                _ => {}
            }
            node = n.parent();
        }
        false
    }

    pub fn definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        // Try field access first so that a same-named global doesn't shadow the field.
        if let Some((table_idx, field_name, expr_id, _)) = self.resolve_field_chain_at(tree, offset) {
            if let Some(result) = self.definition_for_expr(expr_id) {
                return Some(result);
            }
            // Fall back to the field's definition range (e.g. table constructor field)
            if let Some(fi) = self.get_field(table_idx, &field_name)
                && let Some((start, end)) = fi.def_range {
                    let range = TextRange::new(
                        TextSize::from(start),
                        TextSize::from(end),
                    );
                    return Some(DefinitionResult::Local(range));
                }
            // Fall back to external field location (stubs / workspace @field annotations)
            if let Some(loc) = self.find_external_field_location(table_idx, &field_name) {
                return Some(DefinitionResult::External(loc.clone()));
            }
            // Last resort for fields materialized from annotations (e.g. TableLiteral):
            // find the parent table that has a field pointing to this sub-table, then
            // use the parent field's location so the user lands in the right file.
            // Only match fields whose annotation is a structured type (Table), not
            // FieldRef aliases that re-export the same table from a different file.
            if table_idx.is_external() {
                let fl = &self.ir.ext.field_locations;
                for (&candidate_idx, locs) in fl.iter() {
                    if !candidate_idx.is_external() { continue; }
                    let candidate_table = self.table(candidate_idx);
                    for (fname, fi) in &candidate_table.fields {
                        if matches!(&fi.annotation, Some(ValueType::Table(Some(idx))) if *idx == table_idx)
                            && let Some(loc) = locs.get(fname)
                        {
                            return Some(DefinitionResult::External(loc.clone()));
                        }
                    }
                }
            }
        }
        // Don't let a same-named global shadow a field-position token (preceded by dot/colon).
        // Mirrors the same guard in hover_at(); _G.X (including indirect references) is
        // exempted so global-environment field access still works.
        if Self::is_field_position(tree, offset) && !self.is_g_dot_field(tree, offset) {
            return None;
        }
        // Table constructor field: definition is itself. Check before find_symbol_at
        // so that a same-named global doesn't shadow the field key.
        if self.find_constructor_field_at(tree, offset).is_some() {
            let text_size = TextSize::from(offset);
            if let TokenAtOffset::Single(t) | TokenAtOffset::Between(t, _) = SyntaxNode::new_root(tree).token_at_offset(text_size) {
                return Some(DefinitionResult::Local(t.text_range()));
            }
        }
        if let Some((symbol_idx, _, token_start)) = self.find_symbol_at(tree, offset) {
            if symbol_idx.is_external() {
                if let Some(loc) = self.ir.ext.symbol_locations.get(&symbol_idx) {
                    return Some(DefinitionResult::External(loc.clone()));
                }
                return None;
            }
            let symbol = self.sym(symbol_idx);
            let version = self.version_at_def_site(symbol, token_start)
                .or_else(|| symbol.versions.first())?;
            return Some(DefinitionResult::Local(TextRange::new(
                TextSize::from(version.def_node.start),
                TextSize::from(version.def_node.end),
            )));
        }
        // Try expression string go-to-definition
        if let Some(result) = self.expression_definition_at(tree, offset) {
            return Some(result);
        }
        // Try event string go-to-definition
        if let Some(result) = self.event_string_definition_at(tree, offset) {
            return Some(result);
        }
        // Try annotation class/alias name go-to-definition
        if let Some(result) = self.annotation_name_definition_at(tree, offset) {
            return Some(result);
        }
        None
    }

    /// Navigate from a variable to its type's declaration (`textDocument/typeDefinition`).
    ///
    /// For a variable whose resolved type is a `@class`, jumps to the class declaration.
    /// For an `@alias (opaque)` type, jumps to the alias declaration.
    /// For union types, returns the first navigable class/alias member.
    /// Returns `None` for primitives and unresolvable types.
    pub fn type_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        // Try field access first so a same-named global doesn't shadow a field result.
        // Invariant: when resolve_field_chain_at returns Some, the token is always at a
        // field position, so the is_field_position guard below would also return None.
        // We return None explicitly here to make that intent clear and prevent symbol
        // lookup from returning the container variable's type for a non-navigable field.
        if let Some((table_idx, field_name, expr_id, _)) = self.resolve_field_chain_at(tree, offset) {
            let resolved_type = self.resolve_expr_type(expr_id).or_else(|| {
                self.get_field(table_idx, &field_name)
                    .and_then(|fi| fi.annotation.clone())
            });
            return if let Some(vt) = resolved_type {
                self.type_definition_for_value(&vt)
            } else {
                None
            };
        }
        if Self::is_field_position(tree, offset) && !self.is_g_dot_field(tree, offset) {
            return None;
        }
        if let Some((symbol_idx, _, token_start)) = self.find_symbol_at(tree, offset)
            && let Some(resolved) = self.symbol_resolved_type_at(symbol_idx, token_start)
        {
            return self.type_definition_for_value(resolved);
        }
        None
    }

    /// Map a resolved `ValueType` to the source location of its class or alias declaration.
    fn type_definition_for_value(&self, vt: &ValueType) -> Option<DefinitionResult> {
        match vt {
            ValueType::Table(Some(idx)) => {
                let class_name = self.table(*idx).class_name.as_deref()?;
                self.class_definition_by_name(class_name)
            }
            ValueType::OpaqueAlias(name, _) => self.alias_definition_by_name(name),
            ValueType::Union(types) => types.iter().find_map(|t| self.type_definition_for_value(t)),
            ValueType::Intersection(types) => types.iter().find_map(|t| self.type_definition_for_value(t)),
            _ => None,
        }
    }

    /// Look up a `@class` declaration by name, preferring local then external.
    fn class_definition_by_name(&self, name: &str) -> Option<DefinitionResult> {
        if let Some(&(start, end)) = self.ir.class_def_ranges.get(name) {
            return Some(DefinitionResult::Local(TextRange::new(
                TextSize::from(start),
                TextSize::from(end),
            )));
        }
        if let Some(loc) = self.ir.ext.class_locations.get(name) {
            return Some(DefinitionResult::External(loc.clone()));
        }
        None
    }

    /// Look up an `@alias` declaration by name, preferring local then external.
    fn alias_definition_by_name(&self, name: &str) -> Option<DefinitionResult> {
        if let Some(&(start, end)) = self.ir.alias_def_ranges.get(name) {
            return Some(DefinitionResult::Local(TextRange::new(
                TextSize::from(start),
                TextSize::from(end),
            )));
        }
        if let Some(loc) = self.ir.ext.alias_locations.get(name) {
            return Some(DefinitionResult::External(loc.clone()));
        }
        None
    }

    /// Resolve the type of a symbol at the given token offset, selecting the correct
    /// symbol version for redefined locals, params, and external symbols.
    ///
    /// This is the version-tracking logic shared by `type_definition_at` and `hover_at`.
    fn symbol_resolved_type_at(&self, symbol_idx: SymbolIndex, token_start: u32) -> Option<&ValueType> {
        let symbol = self.sym(symbol_idx);
        let is_param = self.is_param_symbol(symbol_idx);
        if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
            symbol.versions.get(ver_idx).and_then(|v| v.resolved_type.as_ref())
        } else if is_param {
            // Always use version 0 for params (the declaration type from @param),
            // not a later version from reassignment in the body.
            symbol.versions.first().and_then(|v| v.resolved_type.as_ref())
        } else if !symbol_idx.is_external() {
            // Declaration site fallback: find the version whose def_node contains this
            // token. For redefined locals (`local x = 1; local x = ""`), each
            // redefinition creates a new version with its own def_node, so we must
            // match the token offset to the correct version rather than always using v0.
            self.version_at_def_site(symbol, token_start)
                .or_else(|| symbol.versions.first())
                .and_then(|v| v.resolved_type.as_ref())
        } else {
            symbol.versions.iter().rev().find_map(|v| v.resolved_type.as_ref())
        }
    }

    fn definition_for_expr(&self, expr_id: ExprId) -> Option<DefinitionResult> {
        match self.expr(expr_id) {
            Expr::FunctionDef(func_idx) => {
                let func_idx = *func_idx;
                if func_idx.is_external() {
                    if let Some(loc) = self.ir.ext.function_locations.get(&func_idx) {
                        return Some(DefinitionResult::External(loc.clone()));
                    }
                    return None;
                }
                let func = self.func(func_idx);
                Some(DefinitionResult::Local(TextRange::new(
                    TextSize::from(func.def_node.start),
                    TextSize::from(func.def_node.end),
                )))
            }
            Expr::SymbolRef(sym_idx, _) => {
                let sym_idx = *sym_idx;
                if sym_idx.is_external() {
                    if let Some(loc) = self.ir.ext.symbol_locations.get(&sym_idx) {
                        return Some(DefinitionResult::External(loc.clone()));
                    }
                    return None;
                }
                let symbol = self.sym(sym_idx);
                let version = symbol.versions.first()?;
                Some(DefinitionResult::Local(TextRange::new(
                    TextSize::from(version.def_node.start),
                    TextSize::from(version.def_node.end),
                )))
            }
            _ => None,
        }
    }

    /// Search for an external field location across the table hierarchy
    /// (own fields → class_name redirect → addon namespace → parent classes → metatable chain).
    fn find_external_field_location(&self, table_idx: TableIndex, field_name: &str) -> Option<&ExternalLocation> {
        let fl = &self.ir.ext.field_locations;
        // Check direct table
        if let Some(loc) = fl.get(&table_idx).and_then(|m| m.get(field_name)) {
            return Some(loc);
        }
        // Try the corresponding external table via class_name.
        // Works for both local tables (cloned from external) and external tables
        // whose field_locations were recorded under a different table index.
        if let Some(ref class_name) = self.table(table_idx).class_name
            && let Some(&ext_idx) = self.ir.ext.classes.get(class_name)
                && ext_idx != table_idx
                    && let Some(loc) = fl.get(&ext_idx).and_then(|m| m.get(field_name)) {
                        return Some(loc);
                    }
        // Check addon namespace tables. Local tables created from select(2,...) clone the
        // addon table. In multi-addon workspaces, the field may belong to a different addon's
        // namespace (e.g. LibTSMData's field accessed from LibTSMApp). Check the current
        // file's addon table first, then all workspace addon tables as fallback.
        if self.table(table_idx).fields.contains_key(field_name) {
            if let Some(addon_idx) = self.ir.addon_table_idx()
                && addon_idx != table_idx
                    && let Some(loc) = fl.get(&addon_idx).and_then(|m| m.get(field_name)) {
                        return Some(loc);
                    }
            // Search all per-addon-root namespace tables (multi-addon workspace)
            for &other_addon_idx in self.ir.ext.addon_tables.values() {
                if other_addon_idx != table_idx
                    && let Some(loc) = fl.get(&other_addon_idx).and_then(|m| m.get(field_name)) {
                        return Some(loc);
                    }
            }
        }
        // Walk parent classes
        for &parent_idx in &self.table(table_idx).parent_classes {
            if let Some(loc) = fl.get(&parent_idx).and_then(|m| m.get(field_name)) {
                return Some(loc);
            }
        }
        // Walk metatable __index chain
        let mut visited = HashSet::new();
        let mut current = table_idx;
        while visited.insert(current) {
            if let Some(index_idx) = self.table(current).metatable_index {
                if let Some(loc) = fl.get(&index_idx).and_then(|m| m.get(field_name)) {
                    return Some(loc);
                }
                for &parent_idx in &self.table(index_idx).parent_classes {
                    if let Some(loc) = fl.get(&parent_idx).and_then(|m| m.get(field_name)) {
                        return Some(loc);
                    }
                }
                current = index_idx;
            } else {
                break;
            }
        }
        // NOTE: Previously had a "last resort" scan over all field_locations looking for
        // any external table with the same field name. Removed because it produced wrong
        // results for common field names (e.g. "type" → random WoW API file). The
        // legitimate cases (cross-addon sub-tables) are covered by the class_name redirect
        // and addon namespace checks above.
        None
    }

    pub fn hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        // Compute enclosing class for visibility filtering in hover tooltips
        let enclosing_class = {
            let text_size = TextSize::from(offset);
            let node = SyntaxNode::new_root(tree).token_at_offset(text_size)
                .right_biased()
                .and_then(|t| t.parent());
            node.and_then(|n| self.find_enclosing_class(&n))
        };
        // Try field access first (e.g. "GetText" in Inbox.GetText) so that
        // a same-named global doesn't shadow the field result.
        if let Some((table_idx, field_name, expr_id, access_kind)) = self.resolve_field_chain_at(tree, offset) {
            let is_g_env = self.ir.is_global_env(table_idx);
            // Try to resolve the field's type for function detection
            let resolved_type = self.resolve_expr_type(expr_id);
            let is_func = matches!(&resolved_type, Some(ValueType::Function(Some(_))));
            let table_name = self.table(table_idx).class_name.clone();
            let sep = match access_kind {
                FieldAccessKind::Colon => ":",
                FieldAccessKind::Dot => ".",
            };

            if is_func
                && let Some(ValueType::Function(Some(func_idx))) = &resolved_type {
                    let skip_self = access_kind == FieldAccessKind::Colon;
                    let qualified_name = if is_g_env {
                        field_name.clone()
                    } else {
                        match &table_name {
                            Some(tname) => format!("{}{}{}", tname, sep, field_name),
                            None => field_name.clone(),
                        }
                    };
                    let kind_label = if is_g_env { "global" } else if access_kind == FieldAccessKind::Colon { "method" } else { "field" };
                    // For a method/field on a parameterized-class receiver, look up
                    // the class type-param substitution (e.g. `T → string`) recorded
                    // at call resolution, keyed by this method-name token's range,
                    // so the displayed signature shows concrete bound types.
                    let subs = self.method_name_range_at(tree, offset)
                        .and_then(|r| self.method_decl_subs.get(&r));
                    let type_str = format!("({}) {}", kind_label, self.format_function_decl(*func_idx, &qualified_name, skip_self, subs));
                    let doc = self.format_function_doc(*func_idx);
                    return Some(HoverResult { type_str, doc });
                }

            if let Some(field_info) = self.get_field(table_idx, &field_name) {
                let (formatted, effective_type, has_annotation) = {
                    if let Some(ref text) = field_info.annotation_text {
                        let expansion = field_info.annotation_type_raw.as_ref()
                            .and_then(|raw| self.expand_alias_fun_signature(raw));
                        let s = match expansion {
                            Some(exp) => format!("{}\n  = {}", text, exp),
                            None => text.clone(),
                        };
                        (s, resolved_type.clone(), true)
                    } else if field_info.lateinit {
                        // Lateinit fields use compact format so "!" appears cleanly after the type name
                        (self.format_field_type(field_info, 0), resolved_type.clone(), true)
                    } else if let Some(ref ann) = field_info.annotation {
                        (self.format_type_accessible(ann, enclosing_class), Some(ann.clone()), true)
                    } else {
                        let has_extras = !field_info.extra_exprs.is_empty();
                        let skip_primary = has_extras
                            && matches!(self.resolve_expr_type(field_info.expr), Some(ValueType::Nil));
                        let mut types: Vec<ValueType> = Vec::new();
                        let exprs: Vec<ExprId> = if skip_primary {
                            field_info.extra_exprs.clone()
                        } else {
                            std::iter::once(field_info.expr).chain(field_info.extra_exprs.iter().copied()).collect()
                        };
                        let mut has_unresolvable = false;
                        for eid in exprs {
                            if let Some(vt) = self.resolve_expr_type(eid) {
                                if !types.contains(&vt) {
                                    types.push(vt);
                                }
                            } else {
                                has_unresolvable = true;
                            }
                        }
                        // If the primary was a nil placeholder (skipped) and
                        // any reassignment couldn't be resolved, the field
                        // could hold any type — widen to Any.
                        if has_unresolvable && skip_primary
                            && !types.contains(&ValueType::Any)
                        {
                            types.push(ValueType::Any);
                        }
                        if types.is_empty() {
                            ("?".to_string(), None, false)
                        } else {
                            let unified = ValueType::make_union(types);
                            let s = self.format_type_accessible(&unified, enclosing_class);
                            (s, Some(unified), false)
                        }
                    }
                };
                let formatted = if !has_annotation {
                    let mut type_args = self.get_type_args_for_expr(expr_id);
                    if type_args.is_empty() {
                        if let Some(args) = self.call_type_args.get(&field_info.expr) {
                            type_args = args.clone();
                        }
                        if type_args.is_empty() {
                            for &extra in &field_info.extra_exprs {
                                if let Some(args) = self.call_type_args.get(&extra) {
                                    type_args = args.clone();
                                    break;
                                }
                            }
                        }
                    }
                    if let Some(ref rt) = effective_type {
                        self.append_type_args_to_class(&formatted, rt, &type_args)
                    } else {
                        formatted
                    }
                } else {
                    formatted
                };
                let mut type_str = format!("(field) {}: {}", field_name, formatted);
                if self.table(table_idx).enum_kind.is_enum()
                    && let Some(val) = self.get_field_literal_value(field_info)
                {
                    type_str.push_str(&format!(" = {}", val));
                }
                let doc = effective_type.as_ref().and_then(|r| self.doc_for_type(r));
                let doc = if let Some(ValueType::Table(Some(table_idx))) = &effective_type {
                    self.append_call_hover(*table_idx, &mut type_str, doc)
                } else {
                    doc
                };
                return Some(HoverResult { type_str, doc });
            }
            if let Some(resolved) = resolved_type {
                let type_args = self.get_type_args_for_expr(expr_id);
                let formatted = self.format_type(&resolved);
                let formatted = self.append_type_args_to_class(&formatted, &resolved, &type_args);
                let label = if is_g_env { "global" } else { "field" };
                let mut type_str = format!("({}) {}: {}", label, field_name, formatted);
                let doc = self.doc_for_type(&resolved);
                let doc = if let ValueType::Table(Some(table_idx)) = &resolved {
                    self.append_call_hover(*table_idx, &mut type_str, doc)
                } else {
                    doc
                };
                return Some(HoverResult { type_str, doc });
            }
            return None;
        }
        // Check for @accessor token hover (e.g. __private in Widget.__private:Method)
        if let Some(result) = self.accessor_hover_at(tree, offset, enclosing_class) {
            return Some(result);
        }
        if Self::is_field_position(tree, offset) && !self.is_g_dot_field(tree, offset) {
            return None;
        }
        // Try varargs hover (... in expressions or parameter lists)
        if let Some(result) = self.varargs_hover_at(tree, offset) {
            return Some(result);
        }
        // Try table constructor field before symbol lookup, so that a same-named
        // global doesn't shadow the field result (e.g. `{ ARMOR = expr }` where
        // ARMOR is also a global string).
        if let Some((field_name, field_info)) = self.find_constructor_field_at(tree, offset) {
            if let Some(ref text) = field_info.annotation_text {
                let expansion = field_info.annotation_type_raw.as_ref()
                    .and_then(|raw| self.expand_alias_fun_signature(raw));
                let type_str = match expansion {
                    Some(exp) => format!("(field) {}: {}\n  = {}", field_name, text, exp),
                    None => format!("(field) {}: {}", field_name, text),
                };
                return Some(HoverResult { type_str, doc: None });
            }
            let type_str = format!("(field) {}: {}", field_name, self.format_field_type(&field_info, 0));
            return Some(HoverResult { type_str, doc: None });
        }
        if let Some((symbol_idx, name, token_start)) = self.find_symbol_at(tree, offset) {
            let symbol = self.sym(symbol_idx);
            let is_param = self.is_param_symbol(symbol_idx);
            // Resolve the type at this reference using shared version-selection logic.
            let resolved = self.symbol_resolved_type_at(symbol_idx, token_start);
            // Determine kind prefix
            let kind = if symbol_idx.is_external() {
                "global"
            } else if symbol.scope_idx == ScopeIndex(0) {
                let def_start = symbol.versions.first().map(|v| v.def_node.start).unwrap_or(0);
                if self.is_local_declaration_site(tree, def_start) { "local" } else { "global" }
            } else if is_param {
                "param"
            } else {
                "local"
            };
            if let Some(resolved) = resolved {
                // For params at declaration (ver_idx == 0 with no recorded reference),
                // skip narrow_type_for_display so scope-level type stripping from
                // early-exit guards doesn't override the declared annotation type.
                let ver_idx = self.symbol_version_at.get(&token_start).copied().unwrap_or(0);
                let is_param_decl = is_param && ver_idx == 0 && !self.symbol_version_at.contains_key(&token_start);
                let display_type = if is_param_decl {
                    None
                } else {
                    self.narrow_type_for_display(resolved, symbol_idx, offset)
                };
                let display_ref = display_type.as_ref().unwrap_or(resolved);
                let doc = self.doc_for_type(display_ref);
                // Declaration-style for functions
                if let ValueType::Function(Some(func_idx)) = display_ref {
                    let type_str = format!("({}) {}", kind, self.format_function_decl(*func_idx, &name, false, None));
                    return Some(HoverResult { type_str, doc });
                }
                // For params at declaration (not narrowed/reassigned), prefer annotation text
                if kind == "param" && ver_idx == 0 && display_type.is_none()
                    && let Some(ann_text) = self.find_param_annotation_text(symbol_idx) {
                        let optional = self.is_param_optional(symbol_idx) || display_ref.contains_nil();
                        let suffix = if optional { "?" } else { "" };
                        let value_suffix = self.get_string_value(symbol_idx, token_start)
                            .map(|s| format!(" = \"{}\"", s))
                            .or_else(|| self.get_number_value(symbol_idx, token_start)
                                .map(|n| format!(" = {}", n)))
                            .unwrap_or_default();
                        let expansion = self.find_param_annotation_raw(symbol_idx)
                            .and_then(|raw| self.expand_alias_fun_signature(raw));
                        let type_str = match expansion {
                            Some(exp) => format!("({}) {}: {}{}{}\n  = {}", kind, name, ann_text, suffix, value_suffix, exp),
                            None => format!("({}) {}: {}{}{}", kind, name, ann_text, suffix, value_suffix),
                        };
                        return Some(HoverResult { type_str, doc });
                    }
                // For locals assigned from event params (e.g. `local e = event`),
                // show the event type alias (e.g. "WowEvent") instead of "string".
                if !symbol_idx.is_external() && display_type.is_none()
                    && let Some(alias) = self.ir.event_type_display.get(&(symbol_idx, ver_idx))
                {
                    let type_str = format!("({}) {}: {}", kind, name, alias);
                    return Some(HoverResult { type_str, doc });
                }
                // For params that are optional or accept nil, strip nil and show ? suffix.
                // Only check contains_nil() — not is_param_optional() — so that
                // narrowed types (e.g. inside `x and abs(x)`) display without `?`.
                let (final_type, optional_suffix) = if kind == "param" && display_ref.contains_nil() {
                    let stripped = display_ref.strip_nil();
                    if matches!(stripped, ValueType::Nil)
                        || matches!(&stripped, ValueType::Union(v) if v.is_empty())
                    {
                        // Type was only nil (stripped result is nil or never) — don't strip, show as-is
                        (None, "")
                    } else {
                        (Some(stripped), "?")
                    }
                } else {
                    (None, "")
                };
                let type_to_format = final_type.as_ref().unwrap_or(display_ref);
                let value_suffix = self.get_string_value(symbol_idx, token_start)
                    .map(|s| format!(" = \"{}\"", s))
                    .or_else(|| self.get_number_value(symbol_idx, token_start)
                        .map(|n| format!(" = {}", n)))
                    .unwrap_or_default();
                // For tables mutated via bracket assignment, show the constructor's
                // initial element type rather than the post-mutation type.
                let formatted = self.initial_array_display(type_to_format)
                    .unwrap_or_else(|| {
                        let f = self.format_type_accessible(type_to_format, enclosing_class);
                        let type_args = self.get_symbol_type_args(symbol_idx, token_start);
                        self.append_type_args_to_class(&f, type_to_format, &type_args)
                    });
                let mut type_str = format!("({}) {}: {}{}{}", kind, name, formatted, optional_suffix, value_suffix);
                let doc = if let ValueType::Table(Some(table_idx)) = type_to_format {
                    self.append_call_hover(*table_idx, &mut type_str, doc)
                } else {
                    doc
                };
                return Some(HoverResult { type_str, doc });
            }
            return Some(HoverResult { type_str: format!("({}) {}: ?", kind, name), doc: None });
        }
        // Try expression string hover (e.g. hovering over "scanProgress" in Publisher([[scanProgress == 1]]))
        if let Some(result) = self.expression_hover_at(tree, offset) {
            return Some(result);
        }
        // Try event string hover (e.g. hovering over "ENCOUNTER_END" in RegisterEvent("ENCOUNTER_END"))
        if let Some(result) = self.event_string_hover_at(tree, offset) {
            return Some(result);
        }
        // Try annotation class/alias name hover (e.g. hovering over "osdateparam" in ---@type osdateparam)
        if let Some(result) = self.annotation_name_hover_at(tree, offset) {
            return Some(result);
        }
        None
    }

    /// Extract the identifier word at the given byte offset if it falls inside an annotation comment.
    /// Supports both `---` line comments and `--[[...]]` / `--[=[...]=]` block comments
    /// that contain `@`-prefixed annotation content (e.g. `@as`, `@cast`, `@type`).
    fn annotation_word_at(&self, tree: &SyntaxTree, offset: u32) -> Option<String> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;
        if token.kind() != SyntaxKind::Comment {
            return None;
        }
        let tok_text = token.text();
        if tok_text.starts_with("---") {
            // Skip @diagnostic lines — they contain diagnostic code names, not type references
            if tok_text.contains("@diagnostic") {
                return None;
            }
        } else {
            // Block comments: --[[...]], --[=[...]=], --[==[...]==], etc.
            let inner = super::block_comment_inner(tok_text)?;
            if !inner.trim_start().starts_with('@') || inner.contains("@diagnostic") {
                return None;
            }
        }
        let tok_start = u32::from(token.text_range().start());
        let cursor_in_tok = (offset - tok_start) as usize;
        if cursor_in_tok >= tok_text.len() {
            return None;
        }
        let bytes = tok_text.as_bytes();
        let is_word_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        if !is_word_byte(bytes[cursor_in_tok]) {
            return None;
        }
        // Scan left: consume word chars, and also '-'/'.' when sandwiched between word chars
        // (handles names like `LibQTip-2.0.Column`).
        let mut start = cursor_in_tok;
        while start > 0 {
            let prev = start - 1;
            if is_word_byte(bytes[prev])
                || ((bytes[prev] == b'-' || bytes[prev] == b'.') && prev > 0 && is_word_byte(bytes[prev - 1]))
            {
                start = prev;
            } else {
                break;
            }
        }
        // Scan right: same logic forward.
        let mut end = cursor_in_tok;
        while end < tok_text.len() {
            if is_word_byte(bytes[end])
                || ((bytes[end] == b'-' || bytes[end] == b'.') && end + 1 < tok_text.len() && is_word_byte(bytes[end + 1]))
            {
                end += 1;
            } else {
                break;
            }
        }
        let word = &tok_text[start..end];
        if word.is_empty() {
            return None;
        }
        Some(word.to_string())
    }

    /// Hover on a class or alias name inside an annotation comment.
    fn annotation_name_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        let word = self.annotation_word_at(tree, offset)?;
        // Check classes (local + external)
        if let Some(&table_idx) = self.ir.classes.get(&word) {
            let table = self.table(table_idx);
            let has_fields = !table.fields.is_empty() || !table.parent_classes.is_empty();
            let prefix = if table.is_key_enum { "(enum key)" } else if table.enum_kind.is_enum() { "(enum)" } else { "(class)" };
            let type_str = if has_fields {
                format!("{} {}", prefix, self.format_type_accessible(&ValueType::Table(Some(table_idx)), None))
            } else {
                format!("{} {}", prefix, word)
            };
            let doc = self.format_see_doc(&table.see);
            return Some(HoverResult { type_str, doc });
        }
        // Check aliases (local + external)
        if let Some(vt) = self.ir.aliases.get(&word).or_else(|| self.ir.ext.aliases.get(&word)) {
            // Prefer the raw `fun(...)` form from `alias_fun_types` over the resolved
            // `ValueType::Function(None)` which renders as the bare word "function".
            // `expand_alias_fun_signature` walks `alias A = B` chains for us.
            let body = self.ir.alias_fun_types.get(&word)
                .or_else(|| self.ir.ext.alias_fun_types.get(&word))
                .and_then(|raw| self.expand_alias_fun_signature(raw))
                .unwrap_or_else(|| self.format_type(vt));
            let type_str = format!("(alias) {} = {}", word, body);
            return Some(HoverResult { type_str, doc: None });
        }
        // Check parameterized aliases (local + external)
        if let Some((type_params, body)) = self.ir.parameterized_aliases.get(&word)
            .or_else(|| self.ir.ext.parameterized_aliases.get(&word))
        {
            let params_str = type_params.join(", ");
            let body_str = crate::annotations::format_annotation_type(body);
            let type_str = format!("(alias) {}<{}> = {}", word, params_str, body_str);
            return Some(HoverResult { type_str, doc: None });
        }
        None
    }

    /// Find the CallResolution and argument index for a token inside a function/method call.
    /// Returns (arg_index, param_index, call_resolution) where param_index accounts for
    /// the implicit `self` parameter in colon calls.
    fn call_resolution_for_arg<'a>(&'a self, token: &SyntaxToken) -> Option<(usize, usize, &'a crate::types::CallResolution)> {
        let call_node = token.ancestors()
            .find(|n| n.kind() == SyntaxKind::FunctionCall || n.kind() == SyntaxKind::MethodCall)?;

        let arg_list = call_node.children()
            .find(|n| n.kind() == SyntaxKind::ArgumentList)?;
        let tok_start = token.text_range().start();
        let mut arg_index = 0usize;
        for child in arg_list.children_with_tokens() {
            if child.text_range().start() >= tok_start {
                break;
            }
            if child.kind() == SyntaxKind::Comma {
                arg_index += 1;
            }
        }

        let call_range = (u32::from(call_node.text_range().start()), u32::from(call_node.text_range().end()));
        let call_res = self.ir.exprs.iter().enumerate()
            .find_map(|(idx, expr)| {
                if let Expr::FunctionCall { call_range: cr, .. } = expr
                    && *cr == call_range
                {
                    self.ir.call_resolutions.get(&ExprId(idx))
                } else {
                    None
                }
            })?;

        let is_colon = call_node.kind() == SyntaxKind::MethodCall
            || FunctionCall::cast(call_node)
                .is_some_and(|c| c.identifier().is_some_and(|id| id.is_call_to_self()));
        let param_index = if is_colon { arg_index + 1 } else { arg_index };

        Some((arg_index, param_index, call_res))
    }

    fn resolve_event_string_at<'a>(&'a self, tree: &'a SyntaxTree, offset: u32) -> Option<(&'a str, &'a str, &'a crate::pre_globals::EventPayload)> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;
        if token.kind() != SyntaxKind::String {
            return None;
        }
        let tok_text = token.text();
        let event_name = tok_text.trim_matches(|c| c == '"' || c == '\'');
        if event_name.is_empty() {
            return None;
        }

        let (_, param_idx, call_res) = self.call_resolution_for_arg(&token)?;
        let func = self.func(call_res.func_idx);
        let ann = func.param_annotations.get(param_idx)?;
        let mut event_type_name = match ann {
            crate::annotations::AnnotationType::Simple(s) => s.as_str(),
            _ => return None,
        };
        // If the param type is a generic type variable (e.g. `@param event E`
        // with `@generic E: FrameEvent`), resolve it to its constraint so the
        // event payload can be looked up under the event-type name.
        if let Some((_, Some(constraint))) = func.generic_constraints_raw.iter()
            .find(|(n, _)| n == event_type_name)
        {
            event_type_name = constraint.as_str();
        }

        let payload = self.ir.ext.event_types.get(event_type_name)?
            .get(event_name)?;
        Some((event_type_name, event_name, payload))
    }

    fn event_string_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        let (_, event_name, payload) = self.resolve_event_string_at(tree, offset)
            .or_else(|| self.resolve_event_string_in_comparison(tree, offset))?;
        let type_str = Self::format_event_payload(event_name, payload);
        Some(HoverResult { type_str, doc: payload.documentation.clone() })
    }

    fn event_string_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        let result = self.resolve_event_string_at(tree, offset)
            .or_else(|| self.resolve_event_string_in_comparison(tree, offset));
        let (event_type_name, event_name, _) = result?;
        let loc = self.ir.ext.event_locations.get(event_type_name)?.get(event_name)?;
        Some(DefinitionResult::External(loc.clone()))
    }

    /// Resolve an event string in an equality comparison like `event == "ADDON_LOADED"`.
    fn resolve_event_string_in_comparison<'a>(&'a self, tree: &'a SyntaxTree, offset: u32) -> Option<(&'a str, &'a str, &'a crate::pre_globals::EventPayload)> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;
        if token.kind() != SyntaxKind::String {
            return None;
        }
        let tok_text = token.text();
        let event_name = tok_text.trim_matches(|c| c == '"' || c == '\'');
        if event_name.is_empty() {
            return None;
        }

        // Walk up to find a BinaryExpression parent with == or ~=.
        // Stop at Block boundaries — the comparison must be a direct ancestor.
        let mut node = token.parent()?;
        let bin_expr = loop {
            match node.kind() {
                SyntaxKind::BinaryExpression => {
                    if let Some(be) = crate::ast::BinaryExpression::cast(node)
                        && matches!(be.kind(), Operator::Equals | Operator::NotEquals)
                    {
                        break be;
                    }
                    node = node.parent()?;
                }
                SyntaxKind::Block => return None,
                _ => node = node.parent()?,
            }
        };

        // Find the identifier on the other side
        let terms = bin_expr.get_terms();
        if terms.len() != 2 {
            return None;
        }
        let string_start = token.text_range().start();
        let string_end = token.text_range().end();
        let other_term = terms.iter().find(|t| {
            let r = t.syntax().text_range();
            !(r.start() <= string_start && string_end <= r.end())
        })?;
        let Expression::Identifier(ident) = other_term else { return None };
        let names = ident.names();
        if names.len() != 1 {
            return None;
        }

        let scope_idx = self.scope_at_offset(text_size)?;
        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;

        // Check if this symbol is an event parameter
        for func in &self.ir.functions {
            let Some((ref event_type_name, event_param_idx)) = func.event_params else { continue };
            if let Some(&arg_sym) = func.args.get(event_param_idx)
                && arg_sym == sym_idx
            {
                let payload = self.ir.ext.event_types.get(event_type_name.as_str())?
                    .get(event_name)?;
                return Some((event_type_name.as_str(), event_name, payload));
            }
        }
        None
    }

    fn format_event_payload(event_name: &str, payload: &crate::pre_globals::EventPayload) -> String {
        if payload.params.is_empty() {
            return format!("(event) {}", event_name);
        }
        let params: Vec<String> = payload.params.iter().map(|p| {
            let nilable = if p.nilable { "?" } else { "" };
            format!("{}{}: {}", p.name, nilable, p.type_name)
        }).collect();
        let single_line = format!("(event) {} \u{2192} {}", event_name, params.join(", "));
        if single_line.len() > 80 && params.len() > 1 {
            format!("(event) {} \u{2192}\n  {}", event_name, params.join(",\n  "))
        } else {
            single_line
        }
    }

    // ── Expression string analysis ─────────────────────────────────────────────
    //
    // For `expression<C, R>` parameters: parse string content as a Lua expression,
    // resolve identifiers against class C's fields, and provide hover/completions/def.

    /// Check whether the token at `offset` is a string literal passed to an
    /// `expression<C, R>` parameter, and return the context if so.
    fn resolve_expression_context_at(&self, tree: &SyntaxTree, offset: u32) -> Option<ExpressionStringContext> {
        use crate::diagnostics::expression_type::compute_content_start;

        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;
        if token.kind() != SyntaxKind::String {
            return None;
        }
        let tok_start = u32::from(token.text_range().start());
        let tok_end = u32::from(token.text_range().end());

        // Find the expression_arg whose stored range matches this string token
        let (&expr_id, arg_info) = self.ir.expression_args.iter()
            .find(|(_, info)| info.str_range.0 == tok_start && info.str_range.1 == tok_end)?;

        let raw_content = self.ir.string_literals.get(&expr_id)?;
        let content = raw_content.as_str();
        let content_start = compute_content_start(content.len(), tok_start, tok_end);

        Some(ExpressionStringContext {
            table_idxs: arg_info.table_idxs.clone(),
            content_start,
            content: content.to_string(),
        })
    }

    /// Extract the identifier word under the cursor within an expression string.
    /// Returns `(word, word_start_in_file, word_end_in_file)`.
    fn expression_word_at(&self, ctx: &ExpressionStringContext, offset: u32) -> Option<(String, u32, u32)> {
        let cursor_in_content = offset.checked_sub(ctx.content_start)? as usize;
        if cursor_in_content >= ctx.content.len() {
            return None;
        }
        let bytes = ctx.content.as_bytes();
        if !(bytes[cursor_in_content].is_ascii_alphanumeric() || bytes[cursor_in_content] == b'_') {
            return None;
        }
        // Find word boundaries
        let mut start = cursor_in_content;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }
        let mut end = cursor_in_content;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        let word = ctx.content[start..end].to_string();
        let word_start = ctx.content_start + start as u32;
        let word_end = ctx.content_start + end as u32;
        Some((word, word_start, word_end))
    }

    /// Hover on an identifier inside an expression string.
    fn expression_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        let ctx = self.resolve_expression_context_at(tree, offset)?;
        let (word, _, _) = self.expression_word_at(&ctx, offset)?;

        // Skip Lua keywords
        if matches!(word.as_str(), "and" | "or" | "not" | "nil" | "true" | "false") {
            return None;
        }

        // Look up the word in any of the context class fields (including parent classes)
        let field_info = ctx.table_idxs.iter()
            .find_map(|&idx| self.get_field(idx, &word))?;
        let type_str = format!("(field) {}: {}", word, self.format_field_type(field_info, 0));
        Some(HoverResult { type_str, doc: None })
    }

    /// Completions inside an expression string: offer all fields from the class.
    fn expression_completions_at(&self, tree: &SyntaxTree, offset: u32) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        let ctx = self.resolve_expression_context_at(tree, offset)?;

        // Don't trigger completions when cursor is on a Lua keyword
        if let Some((word, _, _)) = self.expression_word_at(&ctx, offset)
            && matches!(word.as_str(), "and" | "or" | "not" | "nil" | "true" | "false")
        {
            return None;
        }

        // Collect all fields from all context classes and their parents
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        for &idx in &ctx.table_idxs {
            self.collect_expression_fields(idx, &mut seen, &mut items);
        }

        if items.is_empty() {
            return None;
        }
        Some(items.into_iter().map(|(name, type_str)| {
            CompletionItem {
                label: name,
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(type_str),
                ..CompletionItem::default()
            }
        }).collect())
    }

    /// Recursively collect fields from a table and its parent classes.
    fn collect_expression_fields(&self, table_idx: TableIndex, seen: &mut HashSet<String>, out: &mut Vec<(String, String)>) {
        let table = self.table(table_idx);
        for (name, fi) in &table.fields {
            if seen.insert(name.clone()) {
                let type_str = self.format_field_type(fi, 0);
                out.push((name.clone(), type_str));
            }
        }
        let parents = table.parent_classes.clone();
        for parent_idx in parents {
            self.collect_expression_fields(parent_idx, seen, out);
        }
    }

    /// Go-to-definition on an identifier inside an expression string.
    fn expression_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        let ctx = self.resolve_expression_context_at(tree, offset)?;
        let (word, _, _) = self.expression_word_at(&ctx, offset)?;

        if matches!(word.as_str(), "and" | "or" | "not" | "nil" | "true" | "false") {
            return None;
        }

        // Check if the field has a local def_range in any context class
        for &idx in &ctx.table_idxs {
            if let Some(fi) = self.get_field(idx, &word)
                && let Some((start, end)) = fi.def_range
            {
                return Some(DefinitionResult::Local(TextRange::new(
                    TextSize::from(start),
                    TextSize::from(end),
                )));
            }
            // Try external field location
            if let Some(loc) = self.find_external_field_location(idx, &word) {
                return Some(DefinitionResult::External(loc.clone()));
            }
        }
        None
    }

    /// Go-to-definition on a class or alias name inside an annotation comment.
    fn annotation_name_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        let word = self.annotation_word_at(tree, offset)?;
        // Check local class def ranges
        if let Some(&(start, end)) = self.ir.class_def_ranges.get(&word) {
            let range = TextRange::new(
                TextSize::from(start),
                TextSize::from(end),
            );
            return Some(DefinitionResult::Local(range));
        }
        // Check local alias def ranges
        if let Some(&(start, end)) = self.ir.alias_def_ranges.get(&word) {
            let range = TextRange::new(
                TextSize::from(start),
                TextSize::from(end),
            );
            return Some(DefinitionResult::Local(range));
        }
        // Check external class locations
        if let Some(loc) = self.ir.ext.class_locations.get(&word) {
            return Some(DefinitionResult::External(loc.clone()));
        }
        // Check external alias locations
        if let Some(loc) = self.ir.ext.alias_locations.get(&word) {
            return Some(DefinitionResult::External(loc.clone()));
        }
        None
    }

    /// Get the string literal value for a symbol, checking both local and external sources.
    fn get_string_value(&self, symbol_idx: SymbolIndex, token_start: u32) -> Option<&str> {
        // External symbol: look up in PreResolvedGlobals string_values
        if symbol_idx.is_external() {
            return self.ir.ext.string_values.get(&symbol_idx).map(|s| s.as_str());
        }
        // Local symbol: find the version's type_source and check string_literals
        let symbol = self.sym(symbol_idx);
        let version = if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
            symbol.versions.get(ver_idx)
        } else {
            self.version_at_def_site(symbol, token_start)
                .or_else(|| symbol.versions.last())
        };
        version
            .and_then(|v| v.type_source)
            .and_then(|expr_id| self.ir.string_literals.get(&expr_id))
            .map(|s| s.as_str())
    }

    /// Get the number literal value for a symbol, checking both local and external sources.
    fn get_number_value(&self, symbol_idx: SymbolIndex, token_start: u32) -> Option<&str> {
        if symbol_idx.is_external() {
            return self.ir.ext.number_values.get(&symbol_idx).map(|s| s.as_str());
        }
        let symbol = self.sym(symbol_idx);
        let version = if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
            symbol.versions.get(ver_idx)
        } else {
            self.version_at_def_site(symbol, token_start)
                .or_else(|| symbol.versions.last())
        };
        version
            .and_then(|v| v.type_source)
            .and_then(|expr_id| self.ir.number_literals.get(&expr_id))
            .map(|s| s.as_str())
    }

    /// Get the literal display value for a field's expression (number or quoted string),
    /// checking both local and external sources.
    fn get_field_literal_value(&self, field_info: &FieldInfo) -> Option<String> {
        let (num_map, str_map) = if field_info.expr.is_external() {
            (&self.ir.ext.number_literals, &self.ir.ext.string_literals)
        } else {
            (&self.ir.number_literals, &self.ir.string_literals)
        };
        if let Some(val) = num_map.get(&field_info.expr) {
            return Some(val.clone());
        }
        if let Some(val) = str_map.get(&field_info.expr) {
            return Some(format!("\"{}\"", val));
        }
        None
    }

    /// Format a single field line for enum or regular table display.
    /// Enum fields show `name = value`, non-enum fields show `name: type`.
    fn format_enum_field_line(&self, indent: &str, name: &str, field_info: &FieldInfo, is_enum: bool, depth: usize) -> String {
        if is_enum
            && let Some(val) = self.get_field_literal_value(field_info)
        {
            return format!("{}{} = {}", indent, name, val);
        }
        let type_str = self.format_field_type(field_info, depth);
        format!("{}{}: {}", indent, name, type_str)
    }

    fn narrow_type_for_display(&self, resolved: &ValueType, symbol_idx: SymbolIndex, offset: u32) -> Option<ValueType> {
        let scope_idx = self.scope_at_offset(offset)?;
        // If the symbol was reassigned in this scope, narrowing no longer applies.
        let narrowing_active = !self.is_narrowing_overridden_at(symbol_idx, scope_idx, offset);
        // Start from a type-narrowed base if one exists (e.g. type(x) == "string")
        let base = if narrowing_active {
            if let Some(narrowed_vt) = self.get_type_narrowing(symbol_idx, scope_idx) {
                Some(narrowed_vt.clone())
            } else if let Some(guard_vt) = self.get_type_filtering(symbol_idx, scope_idx) {
                Some(resolved.filter_type_with(guard_vt, &|idx| self.table(idx).enum_kind))
            } else {
                self.get_type_stripping(symbol_idx, scope_idx).map(|stripped_vt| {
                    resolved.strip_type_with(stripped_vt, &|idx| self.table(idx).enum_kind)
                })
            }
        } else {
            None
        };
        // Apply falsy/nil narrowing on top (inner scope `if x then` further narrows)
        let strip_falsy = narrowing_active && self.is_symbol_falsy_narrowed(symbol_idx, scope_idx);
        let strip_nil = strip_falsy || (narrowing_active && self.is_symbol_narrowed(symbol_idx, scope_idx));
        if !strip_nil {
            return base;
        }
        let target = base.as_ref().unwrap_or(resolved);
        // Strip Nil (and optionally false) from union types
        if let ValueType::Union(types) = target {
            let filtered: Vec<_> = types.iter()
                .filter(|t| {
                    if **t == ValueType::Nil { return false; }
                    if strip_falsy && **t == ValueType::Boolean(Some(false)) { return false; }
                    true
                })
                .cloned()
                .collect();
            if filtered.len() == types.len() {
                // Nil stripping didn't change the union; return base if type-filtering
                // or type-narrowing was applied (otherwise None = no change).
                return base;
            }
            if filtered.len() == 1 {
                return Some(filtered.into_iter().next().unwrap());
            }
            if !filtered.is_empty() {
                return Some(ValueType::Union(filtered));
            }
        }
        // Non-union: nil stripping is a no-op. Return base if type-filtering
        // or type-narrowing was applied, otherwise None.
        base
    }

    fn extract_table_idx(resolved: &ValueType) -> Option<TableIndex> {
        match resolved {
            ValueType::Table(Some(idx)) => Some(*idx),
            // Unwrap opaque aliases — field chain resolution works on the inner type
            ValueType::OpaqueAlias(_, inner) => Self::extract_table_idx(inner),
            ValueType::Intersection(types) => types.iter().find_map(|t| match t {
                ValueType::Table(Some(idx)) => Some(*idx),
                _ => None,
            }),
            ValueType::Union(types) => {
                types.iter().find_map(|t| match t {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    ValueType::Intersection(itypes) => itypes.iter().find_map(|it| match it {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }),
                    _ => None,
                })
            }
            _ => None,
        }
    }

    /// Like `extract_table_idx` but returns ALL table indices from the type.
    /// For intersection types this includes every table member (not just the first).
    fn extract_all_table_indices(resolved: &ValueType) -> Vec<TableIndex> {
        match resolved {
            ValueType::Table(Some(idx)) => vec![*idx],
            ValueType::OpaqueAlias(_, inner) => Self::extract_all_table_indices(inner),
            ValueType::Intersection(types) => types.iter().flat_map(
                Self::extract_all_table_indices
            ).collect(),
            ValueType::Union(types) => {
                types.iter().flat_map(Self::extract_all_table_indices).collect()
            }
            _ => vec![],
        }
    }

    /// Find a field by name across multiple tables and their parent classes.
    /// Returns the owning table index and the field's expr id.
    fn find_field_in_tables(&self, table_indices: &[TableIndex], field_name: &str) -> Option<(TableIndex, ExprId)> {
        // First check direct fields on all tables
        for &idx in table_indices {
            if let Some(fi) = self.get_field(idx, field_name) {
                return Some((idx, fi.expr));
            }
        }
        // Then check parent classes of all tables
        for &idx in table_indices {
            for &parent_idx in &self.table(idx).parent_classes.clone() {
                if let Some(fi) = self.get_field(parent_idx, field_name) {
                    return Some((parent_idx, fi.expr));
                }
            }
        }
        None
    }

    /// Look up a global symbol by name in scope0 (local and external).
    /// Returns the symbol's resolved type. Used for `_G.field` redirect.
    fn resolve_global_symbol_type(&self, name: &str) -> Option<ValueType> {
        let sym_id = SymbolIdentifier::Name(name.to_string());
        let sym_idx = self.ir.scopes[0].symbols.get(&sym_id).copied()
            .or_else(|| self.ir.ext.scope0_symbols.get(&sym_id).copied());
        let si = sym_idx?;
        let sym = self.sym(si);
        sym.versions.last().and_then(|v| v.resolved_type.clone())
    }

    fn doc_for_type(&self, st: &ValueType) -> Option<String> {
        match st {
            ValueType::Function(Some(func_idx)) => {
                self.format_function_doc(*func_idx)
            }
            ValueType::Table(Some(table_idx)) => {
                self.format_see_doc(&self.table(*table_idx).see)
            }
            _ => None,
        }
    }

    /// Render `@see` targets as hover doc lines (one per entry).
    pub(crate) fn format_see_doc(&self, see: &[String]) -> Option<String> {
        if see.is_empty() {
            None
        } else {
            Some(see.iter().map(|t| format!("@*see* {}", t)).collect::<Vec<_>>().join("\n\n"))
        }
    }

    /// Build a rich doc string for a function, including its doc comment and @param descriptions.
    fn format_function_doc(&self, func_idx: FunctionIndex) -> Option<String> {
        let func = self.func(func_idx);
        let has_descriptions = func.param_descriptions.iter().any(|d| d.is_some());
        let flavors_mask = func.flavors;
        if func.doc.is_none() && !has_descriptions && func.see.is_empty() && flavors_mask == 0 {
            return None;
        }
        let mut parts = Vec::new();
        if let Some(ref doc) = func.doc {
            parts.push(doc.clone());
        }
        if has_descriptions {
            let mut param_lines = Vec::new();
            for (i, &sym_idx) in func.args.iter().enumerate() {
                if let Some(Some(desc)) = func.param_descriptions.get(i) {
                    let name = match &self.sym(sym_idx).id {
                        SymbolIdentifier::Name(n) => n.clone(),
                        _ => continue,
                    };
                    let optional = func.param_optional.get(i).copied().unwrap_or(false);
                    let ann_has_nil = func.param_annotations.get(i)
                        .is_some_and(crate::annotations::annotation_type_is_nullable);
                    let suffix = if optional && !ann_has_nil { "?" } else { "" };
                    param_lines.push(format!("@*param* `{}{}` — {}", name, suffix, desc));
                }
            }
            if !param_lines.is_empty() {
                parts.push(param_lines.join("\n\n"));
            }
        }
        if let Some(see_block) = self.format_see_doc(&func.see) {
            parts.push(see_block);
        }
        // Low-key flavor info for APIs with known availability data.
        if flavors_mask != 0 {
            parts.push(format!("Flavors: {}", crate::flavor::format_flavor_list(flavors_mask)));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }

    pub fn completions_at(&self, tree: &SyntaxTree, offset: u32, source: &str, snippets: bool) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        if offset == 0 {
            return None;
        }

        let prev_char = source.as_bytes().get((offset - 1) as usize).copied()?;

        // --- Expression string completion: inside a string passed to expression<C, R> ---
        if let Some(items) = self.expression_completions_at(tree, offset) {
            return Some(items);
        }

        // --- String literal completion: inside a string that's part of == or ~= ---
        if let Some(items) = self.string_literal_completions(tree, offset) {
            return Some(items);
        }

        // --- Annotation completion: detect if cursor is inside a ---@ comment ---
        {
            let text_size = TextSize::from(offset.saturating_sub(1));
            let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased();
            if let Some(tok) = token
                && tok.kind() == SyntaxKind::Comment {
                    let tok_text = tok.text();
                    if tok_text.starts_with("---") {
                        let tok_start = u32::from(tok.text_range().start());
                        let cursor_within = (offset - tok_start) as usize;
                        let cursor_within = cursor_within.min(tok_text.len());
                        let prefix = &tok_text[..cursor_within];

                        if let Some(result) = self.annotation_completions(prefix, &tok, snippets) {
                            return Some(result);
                        }
                    }
                }
        }

        // Suppress function-call snippets when a '(' already follows the cursor.
        // This handles swapping one function name for another in an existing call —
        // inserting parens+params would duplicate the existing ones.
        let snippets = snippets && source.get(offset as usize..)
            .is_none_or(|rest| rest.bytes()
                .find(|&b| b != b' ' && b != b'\t') != Some(b'('));

        // Determine effective offset for member-access completions.
        // When the user has typed characters after a '.' or ':', scan backwards
        // through the identifier to find the separator and use its position.
        let (member_offset, is_member_access) = if prev_char == b'.' || prev_char == b':' {
            (offset, true)
        } else if prev_char.is_ascii_alphanumeric() || prev_char == b'_' {
            let mut scan = (offset - 1) as usize;
            while scan > 0 && {
                let ch = source.as_bytes()[scan - 1];
                ch.is_ascii_alphanumeric() || ch == b'_'
            } {
                scan -= 1;
            }
            if scan > 0 && (source.as_bytes()[scan - 1] == b'.' || source.as_bytes()[scan - 1] == b':') {
                (scan as u32, true)
            } else {
                (offset, false)
            }
        } else {
            (offset, false)
        };

        // Extract the typed prefix after '.'/')' for member-access filtering.
        // e.g. in `frame:Regis|`, member_offset points right after ':' and
        // offset is at the cursor, so member_prefix = "Regis".
        let member_prefix = if is_member_access && member_offset < offset {
            source.get(member_offset as usize..offset as usize).unwrap_or("")
        } else {
            ""
        };
        let member_prefix_lower = member_prefix.to_ascii_lowercase();

        if is_member_access {
            // Dot/colon completion: resolve the prefix to a table, enumerate fields
            let offset = member_offset;
            if offset < 2 { return None; }
            let prev_char = source.as_bytes()[(offset - 1) as usize];
            let prefix_offset = offset - 2;
            let text_size = TextSize::from(prefix_offset);
            let mut token = SyntaxNode::new_root(tree).token_at_offset(text_size).right_biased()?;

            // Skip whitespace/newline tokens backwards for multi-line chains like:
            //   func(args)
            //       :method()
            while matches!(token.kind(), SyntaxKind::Whitespace | SyntaxKind::Newline) {
                token = token.prev_token()?;
            }

            // Handle function call return completions: func(). or func():
            // The token before the dot is ')' (RightBracket), so resolve the FunctionCall
            let table_idx = if token.kind() == SyntaxKind::RightBracket {
                if let Some(funcall_node) = token.parent().filter(|p| p.kind() == SyntaxKind::ArgumentList)
                    .and_then(|al| al.parent())
                    .filter(|p| p.kind() == SyntaxKind::FunctionCall || p.kind() == SyntaxKind::MethodCall)
                {
                    Some(self.resolve_funcall_node_to_table(&funcall_node, text_size)?)
                } else if let Some(grouped) = token.parent().filter(|p| p.kind() == SyntaxKind::GroupedExpression) {
                    // ("str"). or ("str"):  — grouped expression containing a string literal
                    let vt = Self::resolve_literal_receiver_type(&grouped)?;
                    let mut indices = Vec::new();
                    self.ir.collect_library_table_indices(&vt, &mut indices);
                    Some(*indices.first()?)
                } else {
                    return None;
                }
            } else if token.kind() == SyntaxKind::String {
                // "str". or "str":  — bare string literal
                let vt = ValueType::String(None);
                let mut indices = Vec::new();
                self.ir.collect_library_table_indices(&vt, &mut indices);
                Some(*indices.first()?)
            } else if token.kind() != SyntaxKind::Name {
                return None;
            } else if let Some(parent) = token.parent() {
                if parent.kind().is_identifier() {
                    Some(self.resolve_identifier_to_table(&parent, text_size)?)
                } else {
                    let name = token.text().to_string();
                    let scope_idx = self.scope_at_offset(text_size)?;
                    let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(name), scope_idx)?;
                    let ver = self.sym(symbol_idx).versions.last()?;
                    let resolved = ver.resolved_type.as_ref()?;
                    Some(Self::extract_table_idx(resolved)?)
                }
            } else {
                return None;
            };

            let table_idx = table_idx?;
            let table = self.table(table_idx);
            let is_colon = prev_char == b':';
            // Determine enclosing class for visibility filtering
            let enclosing_class = {
                let node = SyntaxNode::new_root(tree).token_at_offset(text_size)
                    .right_biased()
                    .and_then(|t| t.parent());
                node.and_then(|n| self.find_enclosing_class(&n))
            };
            // _G global-environment redirect: show all globals as completions
            if self.ir.is_global_env(table_idx) {
                let mut items: Vec<CompletionItem> = Vec::new();
                let mut seen = HashSet::new();
                // Collect from local scope0 and external scope0_symbols
                let scope0_iter = self.ir.scopes[0].symbols.iter()
                    .map(|(id, &idx)| (id.clone(), idx));
                let ext_iter = self.ir.ext.scope0_symbols.iter()
                    .map(|(id, &idx)| (id.clone(), idx));
                for (id, sym_idx) in scope0_iter.chain(ext_iter) {
                    if let SymbolIdentifier::Name(name) = &id {
                        if !seen.insert(name.clone()) { continue; }
                        if !member_prefix_lower.is_empty()
                            && !name.to_ascii_lowercase().starts_with(&member_prefix_lower)
                        {
                            continue;
                        }
                        let sym = self.sym(sym_idx);
                        let resolved = sym.versions.last().and_then(|v| v.resolved_type.as_ref());
                        let kind = match resolved {
                            Some(ValueType::Function(_)) => {
                                if is_colon { CompletionItemKind::METHOD } else { CompletionItemKind::FUNCTION }
                            }
                            _ => {
                                if is_colon { continue; }
                                CompletionItemKind::VARIABLE
                            }
                        };
                        let sort_text = if name.starts_with('_') {
                            format!("1{}", name)
                        } else {
                            format!("0{}", name)
                        };
                        let (insert_text, insert_text_format) = if snippets {
                            if let Some(ValueType::Function(Some(func_idx))) = resolved {
                                if let Some(snippet) = self.build_func_call_snippet(name, *func_idx, is_colon) {
                                    (Some(snippet), Some(lsp_types::InsertTextFormat::SNIPPET))
                                } else {
                                    (None, None)
                                }
                            } else {
                                (None, None)
                            }
                        } else {
                            (None, None)
                        };
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(kind),
                            sort_text: Some(sort_text),
                            insert_text,
                            insert_text_format,
                            data: Some(serde_json::json!({"member": true, "offset": offset, (DATA_REPLACE_START): member_offset})),
                            ..CompletionItem::default()
                        });
                    }
                }
                items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
                return Some(items);
            }

            // Collect all fields: base table + overlay + inherited from parent_classes
            let overlay = self.ir.overlay_fields.get(&table_idx);
            let mut seen_fields: HashSet<&String> = table.fields.keys().collect();
            let mut all_fields: Vec<(&String, &FieldInfo)> = table.fields.iter().collect();
            if let Some(ov) = overlay {
                for (name, fi) in ov.iter() {
                    if seen_fields.insert(name) {
                        all_fields.push((name, fi));
                    }
                }
            }
            // Add inherited fields from parent classes
            for &parent_idx in &table.parent_classes {
                let parent_table = self.table(parent_idx);
                for (name, fi) in &parent_table.fields {
                    if seen_fields.insert(name) {
                        all_fields.push((name, fi));
                    }
                }
            }
            let mut items: Vec<CompletionItem> = all_fields.iter()
                .filter_map(|(name, field_info)| {
                    // Filter out inaccessible private/protected fields
                    let vis = field_info.visibility;
                    if vis != crate::annotations::Visibility::Public {
                        let accessible = match vis {
                            crate::annotations::Visibility::Private => {
                                enclosing_class.is_some_and(|ec| self.same_class(ec, table_idx))
                            }
                            crate::annotations::Visibility::Protected => {
                                enclosing_class.is_some_and(|ec| self.is_subclass_of(ec, table_idx))
                            }
                            crate::annotations::Visibility::Public => true,
                        };
                        if !accessible { return None; }
                    }
                    // Filter by typed prefix (e.g. "Regis" in `frame:Regis`)
                    if !member_prefix_lower.is_empty()
                        && !name.to_ascii_lowercase().starts_with(&member_prefix_lower)
                    {
                        return None;
                    }
                    let resolved = self.resolve_expr_type(field_info.expr);
                    let kind = match &resolved {
                        Some(ValueType::Function(_)) => CompletionItemKind::METHOD,
                        Some(_) => {
                            if is_colon { return None; }
                            CompletionItemKind::FIELD
                        }
                        None => {
                            if is_colon { return None; }
                            CompletionItemKind::FIELD
                        }
                    };
                    let sort_text = if name.starts_with('_') {
                        format!("1{}", name)
                    } else {
                        format!("0{}", name)
                    };
                    let (insert_text, insert_text_format) = if snippets {
                        if let Some(ValueType::Function(Some(func_idx))) = &resolved {
                            if let Some(snippet) = self.build_func_call_snippet(name, *func_idx, is_colon) {
                                (Some(snippet), Some(lsp_types::InsertTextFormat::SNIPPET))
                            } else {
                                (None, None)
                            }
                        } else {
                            (None, None)
                        }
                    } else {
                        (None, None)
                    };
                    Some(CompletionItem {
                        label: name.to_string(),
                        kind: Some(kind),
                        sort_text: Some(sort_text),
                        insert_text,
                        insert_text_format,
                        data: Some(serde_json::json!({"member": true, "offset": offset, (DATA_REPLACE_START): member_offset})),
                        ..CompletionItem::default()
                    })
                })
                .collect();
            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            Some(items)
        } else {
            // Scope completion: enumerate all visible symbols
            let text_size = TextSize::from(offset);

            // Suppress completions when the cursor is on a keyword token (e.g. "then", "end", "do").
            // Without this, typing `if expr then` offers symbols matching "t*" and Enter replaces "then".
            let check_pos = TextSize::from(offset.saturating_sub(1));
            if let Some(tok) = SyntaxNode::new_root(tree).token_at_offset(check_pos).left_biased()
                && tok.kind().is_keyword()
            {
                return None;
            }

            // --- Table constructor field completion ---
            // When cursor is inside a table constructor whose expected type is a
            // known class, offer the class's field names as completions.
            if let Some(items) = self.constructor_field_completions(tree, offset, source) {
                return Some(items);
            }

            let scope_idx = self.scope_at_offset(text_size)?;

            // Extract the typed prefix (partial identifier before the cursor)
            // so we can filter symbols server-side. This keeps the response
            // small even with 60K+ external globals.
            // Note: scanning backwards through as_bytes() is safe because Lua
            // identifiers are ASCII-only; a multi-byte UTF-8 byte would fail
            // the is_ascii_alphanumeric() check, keeping slice boundaries valid.
            let prefix_start;
            let prefix = {
                let end = offset as usize;
                let mut start = end;
                while start > 0 {
                    let ch = source.as_bytes()[start - 1];
                    if ch.is_ascii_alphanumeric() || ch == b'_' {
                        start -= 1;
                    } else {
                        break;
                    }
                }
                prefix_start = start;
                if start < end {
                    &source[start..end]
                } else {
                    ""
                }
            };
            let prefix_lower = prefix.to_ascii_lowercase();
            let has_prefix = !prefix.is_empty();

            // When the grammar unambiguously requires a specific keyword at this position
            // (e.g. `then` after an `if` condition, `do` after `while`), return only that
            // keyword so the user doesn't see unrelated scope symbols.
            if let Some(required_kw) = Self::detect_keyword_only_position(tree, prefix_start) {
                if required_kw.starts_with(&prefix_lower) {
                    return Some(vec![CompletionItem {
                        label: required_kw.to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        sort_text: Some(format!("0{}", required_kw)),
                        data: Some(serde_json::json!({"scope": true, "offset": offset, (DATA_REPLACE_START): prefix_start})),
                        ..CompletionItem::default()
                    }]);
                }
                // Prefix doesn't match the required keyword — nothing useful to offer.
                return None;
            }

            let mut seen = HashSet::new();
            let mut items = Vec::new();
            let mut current_scope = Some(scope_idx);
            while let Some(si) = current_scope {
                let scope = &self.ir.scopes[si.val()];
                for (id, &sym_idx) in &scope.symbols {
                    if let SymbolIdentifier::Name(name) = id
                        && seen.insert(name.clone()) {
                            if has_prefix && !name.to_ascii_lowercase().starts_with(&prefix_lower) {
                                continue;
                            }
                            let resolved = self.sym(sym_idx).versions.iter().rev()
                                .find_map(|v| v.resolved_type.as_ref());
                            let kind = match resolved {
                                Some(ValueType::Function(_)) => CompletionItemKind::FUNCTION,
                                Some(ValueType::Table(Some(idx))) => {
                                    if self.table(*idx).class_name.is_some() {
                                        CompletionItemKind::CLASS
                                    } else {
                                        CompletionItemKind::VARIABLE
                                    }
                                }
                                _ => CompletionItemKind::VARIABLE,
                            };
                            let sort_text = if name.starts_with('_') {
                                format!("1{}", name)
                            } else {
                                format!("0{}", name)
                            };
                            let (insert_text, insert_text_format) = if snippets {
                                if let Some(ValueType::Function(Some(func_idx))) = resolved {
                                    if let Some(snippet) = self.build_func_call_snippet(name, *func_idx, false) {
                                        (Some(snippet), Some(lsp_types::InsertTextFormat::SNIPPET))
                                    } else {
                                        (None, None)
                                    }
                                } else {
                                    (None, None)
                                }
                            } else {
                                (None, None)
                            };
                            items.push(CompletionItem {
                                label: name.clone(),
                                kind: Some(kind),
                                sort_text: Some(sort_text),
                                insert_text,
                                insert_text_format,
                                data: Some(serde_json::json!({"scope": true, "offset": offset, (DATA_REPLACE_START): prefix_start})),
                                ..CompletionItem::default()
                            });
                        }
                }
                current_scope = scope.parent;
            }

            // Include external globals (WoW API functions, tables, etc.)
            let ext_maps: Vec<&HashMap<SymbolIdentifier, SymbolIndex>> = if self.ir.framexml_enabled {
                vec![&self.ir.ext.scope0_symbols, &self.ir.ext.framexml_scope0_symbols]
            } else {
                vec![&self.ir.ext.scope0_symbols]
            };
            for ext_map in ext_maps {
                for (id, &sym_idx) in ext_map {
                    if let SymbolIdentifier::Name(name) = id
                        && seen.insert(name.clone()) {
                            if has_prefix && !name.to_ascii_lowercase().starts_with(&prefix_lower) {
                                continue;
                            }
                            let resolved = self.sym(sym_idx).versions.iter().rev()
                                .find_map(|v| v.resolved_type.as_ref());
                            let kind = match resolved {
                                Some(ValueType::Function(_)) => CompletionItemKind::FUNCTION,
                                Some(ValueType::Table(Some(idx))) => {
                                    if self.table(*idx).class_name.is_some() {
                                        CompletionItemKind::CLASS
                                    } else {
                                        CompletionItemKind::MODULE
                                    }
                                }
                                _ => CompletionItemKind::VARIABLE,
                            };
                            let sort_text = if name.starts_with('_') {
                                format!("3{}", name)
                            } else {
                                format!("2{}", name)
                            };
                            let (insert_text, insert_text_format) = if snippets {
                                if let Some(ValueType::Function(Some(func_idx))) = resolved {
                                    if let Some(snippet) = self.build_func_call_snippet(name, *func_idx, false) {
                                        (Some(snippet), Some(lsp_types::InsertTextFormat::SNIPPET))
                                    } else {
                                        (None, None)
                                    }
                                } else {
                                    (None, None)
                                }
                            } else {
                                (None, None)
                            };
                            items.push(CompletionItem {
                                label: name.clone(),
                                kind: Some(kind),
                                sort_text: Some(sort_text),
                                insert_text,
                                insert_text_format,
                                data: Some(serde_json::json!({"scope": true, "offset": offset, (DATA_REPLACE_START): prefix_start})),
                                ..CompletionItem::default()
                            });
                        }
                }
            }

            // Add Lua keyword completions that match the prefix.
            // This ensures that e.g. `th<TAB>` offers `then` before any external globals
            // like `THE_ALLIANCE` that happen to match the same prefix.
            // Keywords can never appear in `seen` (Lua reserves them, so no local can have
            // a keyword name), so the deduplication guard is omitted here.
            if has_prefix {
                for &kw in LUA_KEYWORDS {
                    if kw.starts_with(&prefix_lower) {
                        items.push(CompletionItem {
                            label: kw.to_string(),
                            kind: Some(CompletionItemKind::KEYWORD),
                            sort_text: Some(format!("0{}", kw)),
                            data: Some(serde_json::json!({"scope": true, "offset": offset, (DATA_REPLACE_START): prefix_start})),
                            ..CompletionItem::default()
                        });
                    }
                }
            }

            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            if items.is_empty() { None } else { Some(items) }
        }
    }

    /// If the cursor is in a position where the grammar requires exactly one keyword
    /// (e.g. `then` after an `if`/`elseif` condition, `do` after a `while` condition
    /// or a `for…in` expression list), return that keyword. The caller can then
    /// suppress all other completions.
    ///
    /// Strategy: find the previous non-whitespace token before the typed prefix,
    /// then walk up its ancestor chain. If we find an `IfBranch`, `WhileLoop`, or
    /// `ForInLoop` node that is missing its required keyword child (`then`/`do`),
    /// the cursor must be in the keyword-only gap between the condition and the block.
    ///
    /// Guard: if the previous token IS the opening keyword (`if`, `elseif`, `while`,
    /// `for`, `in`) the user is still typing the condition/iterator expression —
    /// don't restrict to keyword-only.
    ///
    /// `ForInLoop` is included but only when the `in` keyword is already present
    /// (i.e. we're past the name-list and the iterable expression); this avoids a
    /// false positive for `for k d` where `d` might be another iteration variable.
    fn detect_keyword_only_position(tree: &SyntaxTree, prefix_start: usize) -> Option<&'static str> {
        if prefix_start == 0 { return None; }
        let check = TextSize::from((prefix_start - 1) as u32);
        let mut prev_tok = SyntaxNode::new_root(tree)
            .token_at_offset(check)
            .left_biased()?;

        while matches!(prev_tok.kind(), SyntaxKind::Whitespace | SyntaxKind::Newline) {
            prev_tok = prev_tok.prev_token()?;
        }

        // If the immediately preceding token is the control keyword itself, the user
        // is still typing the condition/iterator — don't restrict to keyword-only.
        if matches!(prev_tok.kind(),
            SyntaxKind::IfKeyword | SyntaxKind::ElseIfKeyword
            | SyntaxKind::WhileKeyword | SyntaxKind::ForKeyword | SyntaxKind::InKeyword
        ) {
            return None;
        }

        // Walk up ancestors looking for a statement node that is missing its keyword.
        let mut node_opt = prev_tok.parent();
        while let Some(node) = node_opt {
            match node.kind() {
                SyntaxKind::IfBranch => {
                    let has_then = node.children_with_tokens().any(|c| {
                        matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::ThenKeyword)
                    });
                    return if has_then { None } else { Some("then") };
                }
                SyntaxKind::WhileLoop => {
                    let has_do = node.children_with_tokens().any(|c| {
                        matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::DoKeyword)
                    });
                    return if has_do { None } else { Some("do") };
                }
                SyntaxKind::ForInLoop => {
                    // Only trigger when `in` is present — otherwise the cursor might be
                    // inside the name list (e.g. `for k d` where `d` is another var).
                    let has_in = node.children_with_tokens().any(|c| {
                        matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::InKeyword)
                    });
                    if !has_in { return None; }
                    let has_do = node.children_with_tokens().any(|c| {
                        matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::DoKeyword)
                    });
                    return if has_do { None } else { Some("do") };
                }
                // Stop at any block/root boundary — we've gone too far.
                SyntaxKind::Block => return None,
                _ => {}
            }
            node_opt = node.parent();
        }
        None
    }

    /// Offer field-name completions when the cursor is inside a table constructor
    /// whose expected type is a known class. Returns `None` if no constructor
    /// context or no expected class is found, letting the caller fall through
    /// to normal scope completions.
    fn constructor_field_completions(&self, tree: &SyntaxTree, offset: u32, source: &str) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        // Find enclosing TableConstructor by walking the AST upward from cursor.
        let check_pos = TextSize::from(offset.saturating_sub(1));
        let token = SyntaxNode::new_root(tree).token_at_offset(check_pos).left_biased()?;
        let parent = token.parent()?;
        let tc_node = parent.ancestors().find(|a| a.kind() == SyntaxKind::TableConstructor)?;

        // Look up the table index for this constructor
        let r = tc_node.text_range();
        let key = (u32::from(r.start()), u32::from(r.end()));
        let ctor_idx = *self.ir.table_ranges.get(&key)?;

        // Find the expected class(es) for this constructor
        let class_indices = self.ir.tc_expected_class.get(&ctor_idx)?;

        // Extract the typed prefix for filtering
        let prefix = {
            let end = offset as usize;
            let mut start = end;
            while start > 0 {
                let ch = source.as_bytes()[start - 1];
                if ch.is_ascii_alphanumeric() || ch == b'_' {
                    start -= 1;
                } else {
                    break;
                }
            }
            if start < end { &source[start..end] } else { "" }
        };
        let prefix_lower = prefix.to_ascii_lowercase();

        // Collect already-set field names from the constructor to exclude them
        let ctor_table = &self.ir.tables[ctor_idx.val()];
        let already_set: HashSet<&String> = ctor_table.fields.keys().collect();

        // Collect fields from all candidate classes and their parents
        let mut seen_fields: HashSet<&String> = HashSet::new();
        let mut all_fields: Vec<(&String, &FieldInfo)> = Vec::new();
        for &class_idx in class_indices {
            let class_table = self.table(class_idx);
            for (name, fi) in &class_table.fields {
                if seen_fields.insert(name) {
                    all_fields.push((name, fi));
                }
            }
            for &parent_idx in &class_table.parent_classes {
                let parent_table = self.table(parent_idx);
                for (name, fi) in &parent_table.fields {
                    if seen_fields.insert(name) {
                        all_fields.push((name, fi));
                    }
                }
            }
        }

        let mut items: Vec<CompletionItem> = all_fields.iter()
            .filter_map(|(name, field_info)| {
                // Skip fields already set in the constructor
                if already_set.contains(*name) { return None; }
                // Skip methods (functions with `self` as first param) — they
                // belong on the prototype, not in a constructor literal.
                // Callbacks like `---@field onClick fun()` are kept.
                let resolved = self.resolve_expr_type(field_info.expr);
                if let Some(ValueType::Function(Some(func_idx))) = &resolved {
                    let func = self.func(*func_idx);
                    let has_self = func.args.first().is_some_and(|&arg| {
                        matches!(&self.sym(arg).id, SymbolIdentifier::Name(n) if n == "self")
                    });
                    if has_self { return None; }
                }
                // Filter by typed prefix
                if !prefix_lower.is_empty()
                    && !name.to_ascii_lowercase().starts_with(&prefix_lower)
                {
                    return None;
                }
                let sort_text = if name.starts_with('_') {
                    format!("1{}", name)
                } else {
                    format!("0{}", name)
                };
                Some(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::FIELD),
                    sort_text: Some(sort_text),
                    ..CompletionItem::default()
                })
            })
            .collect();

        if items.is_empty() { return None; }
        items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
        Some(items)
    }

    /// Build a function-call snippet string for the given function index.
    /// `skip_self` should be true for colon-method calls where `self` is implicit.
    /// Returns `None` if the function has no params (caller should use plain text).
    fn build_func_call_snippet(&self, label: &str, func_idx: crate::types::FunctionIndex, skip_self: bool) -> Option<String> {
        let func = self.func(func_idx);
        let mut param_names: Vec<String> = func.args.iter()
            .filter_map(|&sym_idx| {
                if let crate::types::SymbolIdentifier::Name(n) = &self.sym(sym_idx).id {
                    Some(n.clone())
                } else {
                    None
                }
            })
            .collect();
        if skip_self && param_names.first().map(|n| n == "self").unwrap_or(false) {
            param_names.remove(0);
        }
        if param_names.is_empty() && !func.is_vararg {
            // No params: no snippet needed, return plain `label()`
            return None;
        }
        let mut tabstops: Vec<String> = param_names.iter().enumerate()
            .map(|(i, name)| format!("${{{}:{}}}", i + 1, name))
            .collect();
        if func.is_vararg {
            let next = tabstops.len() + 1;
            tabstops.push(format!("${{{}:...}}", next));
        }
        Some(format!("{}({})", label, tabstops.join(", ")))
    }

    /// Lazily resolve a completion item's `detail` field (called by completionItem/resolve).
    pub(crate) fn resolve_completion(&self, tree: &SyntaxTree, item: &mut lsp_types::CompletionItem) {
        let data = match item.data.as_ref() {
            Some(d) => d,
            None => return,
        };
        let offset = data.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let name = &item.label;

        if data.get("member").and_then(|v| v.as_bool()).unwrap_or(false) {
            // Member-access resolve: find the table, look up the field
            if let Some(detail) = self.resolve_member_detail(tree, offset, name) {
                item.detail = Some(detail);
            }
        } else if data.get("scope").and_then(|v| v.as_bool()).unwrap_or(false) {
            // Scope resolve: find the symbol by name in scope hierarchy + externals
            let scope_idx = self.scope_at_offset(offset);
            if let Some(scope_idx) = scope_idx
                && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx) {
                    let resolved = self.sym(sym_idx).versions.iter().rev()
                        .find_map(|v| v.resolved_type.as_ref());
                    if let Some(vt) = resolved {
                        item.detail = Some(self.format_type(vt));
                    }
                }
        }
    }

    /// Resolve the type detail for a member-access completion item.
    /// `offset` is the byte position of the trigger character (`.` or `:`).
    /// We scan backward from offset to find the preceding token (the receiver).
    fn resolve_member_detail(&self, tree: &SyntaxTree, offset: u32, field_name: &str) -> Option<String> {
        if offset < 1 { return None; }
        // Start just before the trigger character to land on the receiver token
        let prefix_offset = offset - 1;
        let text_size = TextSize::from(prefix_offset);
        let mut token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;

        while matches!(token.kind(), SyntaxKind::Whitespace | SyntaxKind::Newline) {
            token = token.prev_token()?;
        }

        let table_idx = if token.kind() == SyntaxKind::RightBracket {
            let funcall_node = token.parent().filter(|p| p.kind() == SyntaxKind::ArgumentList)
                .and_then(|al| al.parent())
                .filter(|p| p.kind() == SyntaxKind::FunctionCall || p.kind() == SyntaxKind::MethodCall)?;
            self.resolve_funcall_node_to_table(&funcall_node, text_size)?
        } else if token.kind() == SyntaxKind::Name {
            let name = token.text().to_string();
            let scope_idx = self.scope_at_offset(text_size)?;
            let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(name), scope_idx)?;
            let ver = self.sym(symbol_idx).versions.last()?;
            let resolved = ver.resolved_type.as_ref()?;
            Self::extract_table_idx(resolved)?
        } else {
            return None;
        };

        let fi = self.get_field(table_idx, field_name)?;
        let resolved = self.resolve_expr_type(fi.expr)?;
        Some(self.format_type(&resolved))
    }

    // ── String Literal Completions ──────────────────────────────────────────────

    fn string_literal_completions(
        &self,
        tree: &SyntaxTree,
        offset: u32,
    ) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

        if offset == 0 {
            return None;
        }

        // Find the string token at or before the cursor.
        // When the trigger fires on `"`, the cursor is right after the quote.
        let text_size = TextSize::from(offset.saturating_sub(1));
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;
        if token.kind() != SyntaxKind::String {
            return None;
        }

        // Try to resolve the expected type for this string position:
        // 1. Binary expression (== / ~=): resolve the other operand's type
        // 2. Function call argument: resolve the parameter's expected type
        let expected_type = self.string_context_type_from_binary(&token, tree)
            .or_else(|| self.string_context_type_from_call_arg(&token));

        let mut literals = Self::collect_string_literals(&expected_type?);
        if literals.is_empty() {
            return None;
        }

        let tok_text = token.text();
        let quote_char = tok_text.as_bytes().first().copied().unwrap_or(b'"');
        let closing = if quote_char == b'\'' { "'" } else { "\"" };

        // For large completion sets (e.g. event names), pre-filter by the prefix
        // the user has already typed. Without this, the LSP item cap truncates
        // alphabetically and the client never sees items past 'A'.
        // Small sets are left unfiltered so the client can do its own fuzzy matching.
        if literals.len() > crate::MAX_COMPLETIONS {
            let tok_start = u32::from(token.text_range().start());
            let content_end = if tok_text.ends_with('"') || tok_text.ends_with('\'') {
                tok_text.len() - 1
            } else {
                tok_text.len()
            };
            let max_prefix = content_end.saturating_sub(1);
            let prefix_len = (offset.saturating_sub(tok_start + 1) as usize).min(max_prefix);
            if prefix_len > 0
                && let Some(prefix) = tok_text.get(1..1 + prefix_len)
            {
                let prefix_upper = prefix.to_uppercase();
                literals.retain(|lit| lit.to_uppercase().starts_with(&prefix_upper));
                if literals.is_empty() {
                    return None;
                }
            }
        }

        // Replace from after the opening quote to the end of the string token
        // (including the closing quote, if any). The insert_text includes the
        // closing quote, so this avoids a double-quote when the string is
        // already closed (e.g. "" or "partial").
        let replace_start = u32::from(token.text_range().start()) + 1; // after opening quote
        let replace_end = u32::from(token.text_range().end()); // after closing quote (or end of unclosed string)

        let items: Vec<CompletionItem> = literals.iter().enumerate().map(|(i, lit)| {
            CompletionItem {
                label: lit.clone(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                sort_text: Some(format!("{:04}", i)),
                insert_text: Some(format!("{}{}", lit, closing)),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                filter_text: Some(format!("{}{}{}", closing, lit, closing)),
                data: Some(serde_json::json!({(DATA_REPLACE_START): replace_start, (DATA_REPLACE_END): replace_end})),
                ..CompletionItem::default()
            }
        }).collect();
        Some(items)
    }

    /// Resolve string literal type from a `== / ~=` binary expression context.
    fn string_context_type_from_binary(
        &self,
        token: &SyntaxToken,
        tree: &SyntaxTree,
    ) -> Option<ValueType> {
        let mut node = token.parent()?;
        let bin_expr = loop {
            if node.kind() == SyntaxKind::BinaryExpression
                && let Some(be) = crate::ast::BinaryExpression::cast(node)
                && matches!(be.kind(), Operator::Equals | Operator::NotEquals)
            {
                break be;
            }
            node = node.parent()?;
        };

        let terms = bin_expr.get_terms();
        if terms.len() != 2 {
            return None;
        }

        let string_start = token.text_range().start();
        let string_end = token.text_range().end();
        let other_term = terms.iter().find(|t| {
            let r = t.syntax().text_range();
            !(r.start() <= string_start && string_end <= r.end())
        })?;

        self.resolve_type_of_expression_node(tree, &other_term.syntax())
    }

    /// Resolve string literal type from a function/method call argument position.
    fn string_context_type_from_call_arg(
        &self,
        token: &SyntaxToken,
    ) -> Option<ValueType> {
        let (arg_index, param_index, call_res) = self.call_resolution_for_arg(token)?;

        // expected_args already excludes `self` for method calls, so use arg_index directly
        if let Some(resolved_arg) = call_res.expected_args.get(arg_index)
            && let Some(ref et) = resolved_arg.expected_type
        {
            let literals = Self::collect_string_literals(et);
            if !literals.is_empty() {
                return Some(et.clone());
            }
        }

        let func = self.func(call_res.func_idx);

        // Try parameter annotations (these include `self`, so offset for colon calls)
        if let Some(ann) = func.param_annotations.get(param_index) {
            if let Some(vt) = self.resolve_annotation_type_simple(ann) {
                let literals = Self::collect_string_literals(&vt);
                if !literals.is_empty() {
                    return Some(vt);
                }
            }
            // Check if the annotation is an event type name — build completions from event registry
            if let crate::annotations::AnnotationType::Simple(type_name) = ann
                && let Some(events) = self.ir.ext.event_types.get(type_name.as_str())
            {
                let mut names: Vec<&str> = events.keys().map(|s| s.as_str()).collect();
                names.sort_unstable();
                let types = names.into_iter().map(|s| ValueType::String(Some(s.to_owned()))).collect();
                return Some(ValueType::Union(types));
            }
        }

        // Collect string literals across all overload signatures for this param position
        let mut all_literals = Vec::new();
        for overload in &func.overloads {
            if overload.is_return_only {
                continue;
            }
            if let Some(param) = overload.params.get(param_index)
                && let Some(ref vt) = param.typ
            {
                Self::collect_string_literals_inner(vt, &mut all_literals);
            }
        }
        if !all_literals.is_empty() {
            all_literals.dedup();
            let types = all_literals.into_iter().map(|s| ValueType::String(Some(s))).collect();
            return Some(ValueType::Union(types));
        }

        // Check if param is a keyof-constrained generic — provide field name completions
        if let Some(ann) = func.param_annotations.get(param_index)
            && let crate::annotations::AnnotationType::Simple(gen_name) = ann {
                let keyof_target = func.generic_constraints_raw.iter()
                    .find(|(n, _)| n == gen_name)
                    .and_then(|(_, c)| c.as_ref())
                    .and_then(|c| crate::annotations::parse_keyof_constraint(c).map(|s| s.to_string()));
                if let Some(ref_name) = keyof_target {
                    // Find the referenced generic's table binding from the call resolution
                    let table_type = call_res.generic_subs.iter()
                        .find(|(n, _, _)| n == &ref_name)
                        .map(|(_, vt, _)| vt);
                    if let Some(ValueType::Table(Some(table_idx))) = table_type {
                        let fields = crate::analysis::collect_class_fields_impl(
                            &self.ir, &self.resolved_expr_cache, *table_idx,
                        );
                        let mut names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                        names.sort_unstable();
                        let types = names.into_iter()
                            .map(|s| ValueType::String(Some(s.to_owned()))).collect();
                        return Some(ValueType::Union(types));
                    }
                }
            }

        None
    }

    fn resolve_type_of_expression_node(
        &self,
        tree: &SyntaxTree,
        node: &SyntaxNode,
    ) -> Option<ValueType> {
        // For function/method calls, find the IR expr by matching call_range
        if node.kind() == SyntaxKind::FunctionCall || node.kind() == SyntaxKind::MethodCall {
            let range = node.text_range();
            let target = (u32::from(range.start()), u32::from(range.end()));
            for (idx, expr) in self.ir.exprs.iter().enumerate() {
                if let Expr::FunctionCall { call_range, .. } = expr
                    && *call_range == target
                {
                    return self.resolve_expr_type(ExprId(idx));
                }
            }
            return None;
        }

        // For identifiers (name, dot-access, etc.), find the last Name token and use
        // existing field-chain / symbol resolution
        let last_name = node.descendants_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .last()?;
        let name_offset = u32::from(last_name.text_range().start());

        // Try field chain first (e.g. reward.type)
        if let Some((_, _, expr_id, _)) = self.resolve_field_chain_at(tree, name_offset) {
            return self.resolve_expr_type(expr_id);
        }

        // Fall back to simple symbol
        if let Some((sym_idx, _, token_start)) = self.find_symbol_at(tree, name_offset) {
            let sym = self.sym(sym_idx);
            if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
                return sym.versions.get(ver_idx).and_then(|v| v.resolved_type.clone());
            }
            return sym.versions.last().and_then(|v| v.resolved_type.clone());
        }

        None
    }

    fn collect_string_literals(vt: &ValueType) -> Vec<String> {
        let mut result = Vec::new();
        Self::collect_string_literals_inner(vt, &mut result);
        result
    }

    fn collect_string_literals_inner(vt: &ValueType, out: &mut Vec<String>) {
        match vt {
            ValueType::String(Some(s)) => out.push(s.clone()),
            ValueType::Union(types) => {
                for t in types {
                    Self::collect_string_literals_inner(t, out);
                }
            }
            _ => {}
        }
    }

    // ── Annotation Completions ────────────────────────────────────────────────

    fn annotation_completions(
        &self,
        prefix: &str,
        token: &SyntaxToken,
        snippets: bool,
    ) -> Option<Vec<lsp_types::CompletionItem>> {
        let after_dashes = prefix.trim_start_matches('-');

        if !after_dashes.starts_with('@') {
            // Bare `---` with no `@` yet — offer "generate annotations" for the function below.
            // Return Some(empty) to suppress scope completions in comment context.
            return Some(self.try_generate_annotations_completion(token, snippets).unwrap_or_default());
        }

        let after_at = &after_dashes[1..];

        if let Some(mut items) = self.try_tag_completions(after_at, token, snippets) {
            // When no tag is typed yet (just `---@`), also offer "Annotate function"
            if after_at.is_empty() && let Some(gen_items) = self.try_generate_annotations_completion(token, snippets) {
                items.extend(gen_items);
            }
            return Some(items);
        }
        if let Some(items) = self.try_param_name_completions(after_at, token) {
            return Some(items);
        }
        if let Some(items) = self.try_type_completions(after_at) {
            return Some(items);
        }

        // Inside a ---@ annotation — never fall back to general scope completions.
        Some(Vec::new())
    }

    fn try_tag_completions(&self, after_at: &str, token: &SyntaxToken, snippets: bool) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

        if after_at.contains(' ') || after_at.contains('\t') {
            return None;
        }

        // Context flags for each tag
        const F: u8 = 1; // function context
        const C: u8 = 2; // class context
        const S: u8 = 4; // standalone / fresh context
        #[allow(clippy::identity_op)] // bare F/C/S without `|` triggers identity_op
        // (name, detail, context_flags, snippet_body)
        // snippet_body is the text inserted after `@`; None means no snippet for this tag.
        const TAGS: &[(&str, &str, u8, Option<&str>)] = &[
            ("param",          "Document a function parameter",               F,     Some("param ${1:name} ${2:type}")),
            ("return",         "Document return type(s)",                     F,     Some("return ${1:type}")),
            ("type",           "Declare variable type",                       S,     Some("type ${1:type}")),
            ("class",          "Define a class",                              S,     Some("class ${1:ClassName}")),
            ("field",          "Define a class field",                    C,         Some("field ${1:name} ${2:type}")),
            ("alias",          "Define a type alias",                         S,     Some("alias ${1:Name} ${2:type}")),
            ("enum",           "Define an enum",                              S,     Some("enum ${1:type}")),
            ("event",          "Declare an event with a typed payload",       S,     Some("event ${1:EventName}")),
            ("overload",       "Define an overload signature",            F|C,       None),
            ("defclass",       "Generic that auto-creates classes",       F,         None),
            ("generic",        "Declare generic type parameter(s)",       F,         Some("generic ${1:T}")),
            ("cast",           "Cast a variable's type",                      S,     Some("cast ${1:name} ${2:type}")),
            ("as",             "Inline type assertion",                       S,     None),
            ("builds-field",   "Builder method adds field to built type", F,         None),
            ("built-name",     "Set built table class name from param",   F,         None),
            ("built-extends",  "Built type inherits from receiver",       F,         None),
            ("constructor",    "Mark as constructor method",              F|C,       None),
            ("deprecated",     "Mark as deprecated",                      F|C|S,     None),
            ("nodiscard",      "Warn if return value is ignored",         F|C,       None),
            ("private",        "Mark as private visibility",              F|C|S,     None),
            ("protected",      "Mark as protected visibility",            F|C|S,     None),
            ("accessor",       "Define accessor with visibility",           C,       None),
            ("meta",           "Mark file as meta (declaration-only)",         S,   None),
            ("diagnostic",     "Control diagnostic suppression",          F|C|S,     Some("diagnostic ${1|enable,disable|}:${2:code}")),
            ("type-narrows",   "Type guard that narrows target param",    F,         None),
            ("flavor-narrows", "Flavor guard that narrows WoW API availability", F,  None),
            ("narrows-arg",    "In-place argument type narrowing",        F,         Some("narrows-arg ${1:N}")),
            ("requires",       "Restrict method by receiver type-param constraint", F,  Some("requires ${1:T}: ${2:Constraint}")),
            ("correlated",     "Declare fields that are always nil/non-nil together", C, None),
            ("see",            "Cross-reference link to related symbol or URL", F|C|S, None),
        ];

        let ctx = self.detect_annotation_context(token);
        let ctx_mask = match ctx {
            AnnotationContext::Function => F,
            AnnotationContext::Class => C,
            AnnotationContext::Any => F | C | S,
        };

        let partial = after_at;
        let items: Vec<CompletionItem> = TAGS.iter()
            .filter(|(name, _, flags, _)| name.starts_with(partial) && (flags & ctx_mask) != 0)
            .map(|(name, detail, _, snippet_body)| {
                let (insert_text, insert_text_format) = if snippets {
                    if let Some(body) = snippet_body {
                        (Some(body.to_string()), Some(InsertTextFormat::SNIPPET))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };
                CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some(detail.to_string()),
                    insert_text,
                    insert_text_format,
                    ..CompletionItem::default()
                }
            })
            .collect();

        if items.is_empty() {
            // No whitespace in after_at means we're in tag position — return empty
            // to prevent fallthrough to param-name / type-name completions.
            Some(Vec::new())
        } else {
            Some(items)
        }
    }

    fn detect_annotation_context(&self, token: &SyntaxToken) -> AnnotationContext {
        let mut has_function_tag = false;
        let mut has_class_tag = false;
        let mut prev_was_newline = false;

        // Walk backward through contiguous --- comments in the same block
        let mut tok = token.prev_token();
        while let Some(t) = tok {
            let kind = t.kind();
            if kind == SyntaxKind::Newline {
                if prev_was_newline {
                    break; // blank line = end of annotation block
                }
                prev_was_newline = true;
                tok = t.prev_token();
                continue;
            }
            prev_was_newline = false;
            if kind == SyntaxKind::Whitespace {
                tok = t.prev_token();
                continue;
            }
            if kind == SyntaxKind::Comment {
                let text = t.text();
                if text.starts_with("---") {
                    if let Some(after_at) = text.strip_prefix("---@")
                        .or_else(|| text.strip_prefix("---").and_then(|s| s.trim_start().strip_prefix('@')))
                    {
                        let tag = after_at.split(|c: char| c.is_whitespace()).next().unwrap_or("");
                        match tag {
                            "param" | "return" | "generic" | "builds-field" | "built-name"
                            | "built-extends" | "type-narrows" | "defclass" | "flavor-narrows"
                            | "narrows-arg" | "requires" => {
                                has_function_tag = true;
                            }
                            "class" | "enum" | "field" | "accessor" | "correlated" => {
                                has_class_tag = true;
                            }
                            _ => {} // ambiguous tags (deprecated, private, etc.) don't determine context
                        }
                    }
                    tok = t.prev_token();
                    continue;
                }
            }
            break; // non-doc-comment or non-comment token = end of block
        }

        if has_class_tag {
            AnnotationContext::Class
        } else if has_function_tag || self.is_annotation_block_above_function(token) {
            AnnotationContext::Function
        } else {
            AnnotationContext::Any
        }
    }

    /// Check if the annotation block containing `token` is directly above a function definition
    /// (no blank lines between the block and the function).
    fn is_annotation_block_above_function(&self, token: &SyntaxToken) -> bool {
        use crate::ast::FunctionDefinition;

        let mut prev_was_newline = false;
        let mut tok = token.next_token();
        while let Some(t) = tok {
            let kind = t.kind();
            match kind {
                SyntaxKind::Newline => {
                    if prev_was_newline {
                        return false; // blank line breaks association
                    }
                    prev_was_newline = true;
                }
                SyntaxKind::Whitespace => {}
                SyntaxKind::Comment => {
                    prev_was_newline = false;
                }
                _ => {
                    // First significant token — check if it starts a function.
                    // Only walk parents whose start matches the token (avoids
                    // matching an enclosing function when the annotation is
                    // inside a function body).
                    let tok_start = u32::from(t.text_range().start());
                    let mut node = t.parent();
                    while let Some(n) = node {
                        if u32::from(n.text_range().start()) != tok_start {
                            break;
                        }
                        match n.kind() {
                            SyntaxKind::FunctionDefinition => return true,
                            SyntaxKind::LocalAssignStatement | SyntaxKind::AssignStatement => {
                                // Check for `local f = function(...)` or `f = function(...)`
                                for child in n.children() {
                                    if child.kind() == SyntaxKind::ExpressionList {
                                        for expr in child.children() {
                                            if FunctionDefinition::cast(expr).is_some() {
                                                return true;
                                            }
                                        }
                                    }
                                }
                                return false;
                            }
                            _ => {}
                        }
                        node = n.parent();
                    }
                    return false;
                }
            }
            tok = t.next_token();
        }
        false
    }

    fn try_param_name_completions(
        &self,
        after_at: &str,
        token: &SyntaxToken,
    ) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        let rest = after_at.strip_prefix("param")?;
        if !rest.starts_with(' ') && !rest.starts_with('\t') {
            return None;
        }
        let after_param = rest.trim_start();

        // If there's already a space after the name, cursor is in type position
        if after_param.contains(' ') || after_param.contains('\t') {
            return None;
        }

        let partial_name = after_param;
        let param_names = self.find_function_params_below(token)?;

        let items: Vec<CompletionItem> = param_names.iter()
            .filter(|name| name.starts_with(partial_name))
            .map(|name| CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                ..CompletionItem::default()
            })
            .collect();

        if items.is_empty() { None } else { Some(items) }
    }

    fn find_function_params_below(
        &self,
        comment_token: &SyntaxToken,
    ) -> Option<Vec<String>> {
        use crate::ast::FunctionDefinition;

        let mut tok = comment_token.next_token();
        while let Some(t) = tok {
            let kind = t.kind();
            if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline || kind == SyntaxKind::Comment {
                tok = t.next_token();
                continue;
            }
            // First significant token — look for a FunctionDefinition in the parent chain
            let mut node = t.parent();
            while let Some(n) = node {
                if let Some(func_def) = FunctionDefinition::cast(n) {
                    return Some(func_def.params()?.parameters());
                }
                // Check children for inline function definitions (e.g. local x = function(...))
                for child in n.children() {
                    if let Some(func_def) = FunctionDefinition::cast(child) {
                        return Some(func_def.params()?.parameters());
                    }
                }
                node = n.parent();
            }
            return None;
        }
        None
    }

    /// Find the FunctionDefinition AST node directly below a comment token
    /// (no blank lines between) and return its start offset.
    fn find_function_def_start_below(&self, comment_token: &SyntaxToken) -> Option<u32> {
        let mut prev_was_newline = false;
        let mut tok = comment_token.next_token();
        while let Some(t) = tok {
            let kind = t.kind();
            match kind {
                SyntaxKind::Newline => {
                    if prev_was_newline { return None; } // blank line breaks association
                    prev_was_newline = true;
                    tok = t.next_token();
                    continue;
                }
                SyntaxKind::Whitespace => {
                    tok = t.next_token();
                    continue;
                }
                SyntaxKind::Comment => {
                    prev_was_newline = false;
                    tok = t.next_token();
                    continue;
                }
                _ => {}
            }
            let tok_start = u32::from(t.text_range().start());
            let mut node = t.parent();
            while let Some(n) = node {
                if u32::from(n.text_range().start()) != tok_start {
                    break;
                }
                match n.kind() {
                    SyntaxKind::FunctionDefinition => {
                        return Some(u32::from(n.text_range().start()));
                    }
                    SyntaxKind::LocalAssignStatement | SyntaxKind::AssignStatement => {
                        for child in n.children() {
                            if child.kind() == SyntaxKind::ExpressionList {
                                for expr in child.children() {
                                    if expr.kind() == SyntaxKind::FunctionDefinition {
                                        return Some(u32::from(expr.text_range().start()));
                                    }
                                }
                            }
                        }
                        return None;
                    }
                    _ => {}
                }
                node = n.parent();
            }
            return None;
        }
        None
    }

    /// Check if the annotation block already contains function-level tags (@param, @return, etc.)
    fn annotation_block_has_function_tags(&self, token: &SyntaxToken) -> bool {
        let mut prev_was_newline = false;
        let mut tok = token.prev_token();
        while let Some(t) = tok {
            let kind = t.kind();
            if kind == SyntaxKind::Newline {
                if prev_was_newline { break; }
                prev_was_newline = true;
                tok = t.prev_token();
                continue;
            }
            prev_was_newline = false;
            if kind == SyntaxKind::Whitespace {
                tok = t.prev_token();
                continue;
            }
            if kind == SyntaxKind::Comment {
                let text = t.text();
                if text.starts_with("---") {
                    if let Some(after_at) = text.strip_prefix("---@")
                        .or_else(|| text.strip_prefix("---").and_then(|s| s.trim_start().strip_prefix('@')))
                    {
                        let tag = after_at.split(|c: char| c.is_whitespace()).next().unwrap_or("");
                        match tag {
                            "param" | "return" | "generic" | "overload" => return true,
                            _ => {}
                        }
                    }
                    tok = t.prev_token();
                    continue;
                }
            }
            break;
        }
        false
    }

    /// Offer a "generate annotations" completion when typing `---` above a function.
    fn try_generate_annotations_completion(
        &self,
        token: &SyntaxToken,
        snippets: bool,
    ) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

        // Don't offer if existing annotation block already has @param/@return
        if self.annotation_block_has_function_tags(token) {
            return None;
        }

        let func_start = self.find_function_def_start_below(token)?;
        let func_idx = self.ir.functions.iter().enumerate()
            .find(|(_, f)| f.def_node.start == func_start)
            .map(|(i, _)| FunctionIndex(i))?;
        let func = self.func(func_idx);

        // Collect parameter info (skip self)
        let self_injected = !func.args.is_empty()
            && matches!(&self.sym(func.args[0]).id, SymbolIdentifier::Name(n) if n == "self");
        let arg_offset = if self_injected { 1 } else { 0 };

        struct ParamInfo {
            name: String,
            type_text: Option<String>,
        }
        let mut params: Vec<ParamInfo> = Vec::new();
        for i in arg_offset..func.args.len() {
            let sym_idx = func.args[i];
            let sym = self.sym(sym_idx);
            let name = match &sym.id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => continue,
            };
            // Get inferred type
            let type_text = sym.versions.first()
                .and_then(|v| v.resolved_type.as_ref())
                .and_then(|vt| {
                    if matches!(vt, ValueType::Any | ValueType::Nil) {
                        None
                    } else {
                        Some(self.format_type_depth(vt, 1))
                    }
                });
            params.push(ParamInfo { name, type_text });
        }

        // Collect return type info, filtering out unknown ("?") positions eagerly
        let returns: Vec<String> = if func.return_annotations.is_empty() && !func.returns_self && !func.explicit_void_return {
            self.format_inferred_returns(func, 1).into_iter()
                .filter(|r| r != "?")
                .collect()
        } else {
            vec![]
        };

        // Nothing to generate
        if params.is_empty() && returns.is_empty() {
            return None;
        }

        // Build the snippet/plain text
        let mut lines: Vec<String> = Vec::new();
        let mut tabstop = 1u32;

        // Summary line
        if snippets {
            lines.push(format!("---${{{}:TODO}}", tabstop));
            tabstop += 1;
        } else {
            lines.push("--- TODO".to_string());
        }

        for p in &params {
            if snippets {
                let type_placeholder = p.type_text.as_deref().unwrap_or("any");
                lines.push(format!("---@param {} ${{{}:{}}}", p.name, tabstop, type_placeholder));
                tabstop += 1;
            } else {
                let type_text = p.type_text.as_deref().unwrap_or("any");
                lines.push(format!("---@param {} {}", p.name, type_text));
            }
        }

        for r in &returns {
            if snippets {
                lines.push(format!("---@return ${{{}:{}}}", tabstop, r));
                tabstop += 1;
            } else {
                lines.push(format!("---@return {}", r));
            }
        }

        if lines.is_empty() {
            return None;
        }

        let insert_text = lines.join("\n");

        // Build a short detail preview
        let detail = if params.is_empty() {
            format!("@return {}", returns.join(", "))
        } else if returns.is_empty() {
            format!("{} @param(s)", params.len())
        } else {
            format!("{} @param(s), @return", params.len())
        };

        let tok_start = u32::from(token.text_range().start());

        let item = CompletionItem {
            label: "Annotate function".to_string(),
            // filter_text must cover the full token text so VS Code's client-side
            // fuzzy filter keeps this item when the typed prefix is `---` or `---@`.
            filter_text: Some("---@Annotate function".to_string()),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(detail),
            insert_text: Some(insert_text),
            insert_text_format: if snippets { Some(InsertTextFormat::SNIPPET) } else { None },
            sort_text: Some("0".to_string()),
            data: Some(serde_json::json!({(DATA_REPLACE_START): tok_start})),
            ..CompletionItem::default()
        };

        Some(vec![item])
    }

    fn try_type_completions(&self, after_at: &str) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        let type_prefix = self.extract_type_prefix_from_annotation(after_at)?;

        // Handle pipe-separated union types: take only the part after the last '|'
        let type_prefix = type_prefix.rsplit('|').next().unwrap_or(type_prefix).trim();

        let mut items = Vec::new();
        let mut seen = HashSet::new();

        const BUILTINS: &[&str] = &[
            "number", "string", "boolean", "nil", "table", "function", "any", "self", "void",
        ];
        for &name in BUILTINS {
            if name.starts_with(type_prefix) && seen.insert(name.to_string()) {
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..CompletionItem::default()
                });
            }
        }

        collect_type_name_completions(self.ir.classes.keys(), type_prefix, CompletionItemKind::CLASS, &mut seen, &mut items);
        collect_type_name_completions(self.ir.ext.classes.keys(), type_prefix, CompletionItemKind::CLASS, &mut seen, &mut items);
        collect_type_name_completions(self.ir.aliases.keys(), type_prefix, CompletionItemKind::INTERFACE, &mut seen, &mut items);
        collect_type_name_completions(self.ir.ext.aliases.keys(), type_prefix, CompletionItemKind::INTERFACE, &mut seen, &mut items);
        collect_type_name_completions(self.ir.parameterized_aliases.keys(), type_prefix, CompletionItemKind::INTERFACE, &mut seen, &mut items);
        collect_type_name_completions(self.ir.ext.parameterized_aliases.keys(), type_prefix, CompletionItemKind::INTERFACE, &mut seen, &mut items);

        items.sort_by(|a, b| a.label.cmp(&b.label));
        if items.is_empty() { None } else { Some(items) }
    }

    fn extract_type_prefix_from_annotation<'b>(&self, after_at: &'b str) -> Option<&'b str> {
        // @param name TYPE...
        if let Some(rest) = after_at.strip_prefix("param") {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let rest = rest.trim_start();
                if let Some(space_pos) = rest.find(char::is_whitespace) {
                    return Some(rest[space_pos..].trim_start());
                }
            }
            return None;
        }

        // @return TYPE...
        if let Some(rest) = after_at.strip_prefix("return") {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let after_return = rest.trim_start();
                // Handle multiple return types — take after last comma
                let after_last_comma = after_return.rsplit(',').next().unwrap_or(after_return).trim();
                return Some(after_last_comma);
            }
            return None;
        }

        // @type TYPE...
        if let Some(rest) = after_at.strip_prefix("type") {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                return Some(rest.trim_start());
            }
            return None;
        }

        // @field [visibility] name TYPE...
        if let Some(rest) = after_at.strip_prefix("field") {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let rest = rest.trim_start();
                let rest = if let Some(r) = rest.strip_prefix("private") {
                    if r.starts_with(char::is_whitespace) { r.trim_start() } else { rest }
                } else if let Some(r) = rest.strip_prefix("protected") {
                    if r.starts_with(char::is_whitespace) { r.trim_start() } else { rest }
                } else if let Some(r) = rest.strip_prefix("public") {
                    if r.starts_with(char::is_whitespace) { r.trim_start() } else { rest }
                } else {
                    rest
                };
                if let Some(space_pos) = rest.find(char::is_whitespace) {
                    return Some(rest[space_pos..].trim_start());
                }
            }
            return None;
        }

        // @alias name TYPE...
        if let Some(rest) = after_at.strip_prefix("alias") {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let rest = rest.trim_start();
                if let Some(space_pos) = rest.find(char::is_whitespace) {
                    return Some(rest[space_pos..].trim_start());
                }
            }
            return None;
        }

        // @generic name: CONSTRAINT_TYPE
        if let Some(rest) = after_at.strip_prefix("generic") {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let rest = rest.trim_start();
                if let Some(colon_pos) = rest.find(':') {
                    return Some(rest[colon_pos + 1..].trim_start());
                }
            }
            return None;
        }

        // @cast varname [+|-]TYPE
        if let Some(rest) = after_at.strip_prefix("cast") {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let rest = rest.trim_start();
                if let Some(space_pos) = rest.find(char::is_whitespace) {
                    let after_name = rest[space_pos..].trim_start();
                    let after_name = after_name.strip_prefix('+')
                        .or_else(|| after_name.strip_prefix('-'))
                        .unwrap_or(after_name);
                    return Some(after_name);
                }
            }
            return None;
        }

        None
    }

    /// Resolve a dot/colon chain at offset, returning (owning_table_idx, field_name, field_expr_id, access_kind).
    /// Byte range of the `Name` token at `offset`, matching the `field_range`
    /// stored on method/field `FieldAccess` exprs during lowering. Used to look
    /// up `method_decl_subs` for hover type-variable substitution.
    fn method_name_range_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(u32, u32)> {
        let text_size = TextSize::from(offset);
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(left, right) => {
                if right.kind() == SyntaxKind::Name { right }
                else if left.kind() == SyntaxKind::Name { left }
                else { return None; }
            }
            TokenAtOffset::None => return None,
        };
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        let r = token.text_range();
        Some((u32::from(r.start()), u32::from(r.end())))
    }

    pub(crate) fn resolve_field_chain_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(TableIndex, String, ExprId, FieldAccessKind)> {
        let text_size = TextSize::from(offset);
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(left, right) => {
                if right.kind() == SyntaxKind::Name { right }
                else if left.kind() == SyntaxKind::Name { left }
                else { return None; }
            }
            TokenAtOffset::None => return None,
        };
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        let parent = token.parent()?;

        // Handle method name in FunctionCall/MethodCall: expr:method(args)
        // The Name token is a direct child of FunctionCall/MethodCall, preceded by Colon
        if parent.kind() == SyntaxKind::FunctionCall || parent.kind() == SyntaxKind::MethodCall {
            let has_colon = parent.children_with_tokens().any(|t|
                t.as_token().is_some_and(|tok| tok.kind() == SyntaxKind::Colon));
            if has_colon {
                let method_name = token.text().to_string();
                // Resolve receiver to all table indices (intersection-aware).
                let table_indices = self.resolve_receiver_to_all_tables(&parent, text_size);
                if let Some((table_idx, expr_id)) = self.find_field_in_tables(&table_indices, &method_name) {
                    return Some((table_idx, method_name, expr_id, FieldAccessKind::Colon));
                }
            }
            return None;
        }

        if !parent.kind().is_identifier() {
            return None;
        }
        // Collect direct Name tokens in the Identifier
        let names: Vec<_> = parent.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();

        // Handle method/field after a child Identifier or FunctionCall (e.g. t[k]:method, chained calls)
        // The parent Identifier has a child node (the base) and one direct Name (the field/method).
        let is_call_kind = |k: SyntaxKind| k == SyntaxKind::FunctionCall || k == SyntaxKind::MethodCall;
        let has_child_ident = parent.children().any(|c| c.kind().is_identifier());
        let has_child_funcall = parent.children().any(|c| is_call_kind(c.kind()));
        if (has_child_ident || has_child_funcall) && names.len() == 1 {
            let has_colon = parent.children_with_tokens().any(|t|
                t.as_token().is_some_and(|tok| tok.kind() == SyntaxKind::Colon));
            let access = if has_colon { FieldAccessKind::Colon } else { FieldAccessKind::Dot };
            let field_name = names[0].text().to_string();
            // Resolve receiver to all table indices (intersection-aware)
            let table_indices = self.resolve_receiver_to_all_tables(&parent, text_size);
            if let Some((table_idx, expr_id)) = self.find_field_in_tables(&table_indices, &field_name) {
                return Some((table_idx, field_name, expr_id, access));
            }
            // Check _G.field redirect
            for &idx in &table_indices {
                if let Some(result) = self.resolve_g_env_field(idx, &field_name, access) {
                    return Some(result);
                }
            }
            return None;
        }

        if names.len() < 2 {
            // Check grandparent: for `func().field`, the parent Identifier wraps just "field",
            // but the grandparent Identifier has a FunctionCall sibling we can resolve through.
            if names.len() == 1
                && let Some(grandparent) = parent.parent()
                    && grandparent.kind() .is_identifier()
                        && let Some(funcall_node) = grandparent.children().find(|c| c.kind() == SyntaxKind::FunctionCall || c.kind() == SyntaxKind::MethodCall)
                            && let Some(table_idx) = self.resolve_funcall_node_to_table(&funcall_node, text_size) {
                                let field_name = names[0].text().to_string();
                                let access = Self::detect_access_before_token(&parent, &token);
                                if let Some(fi) = self.table(table_idx).fields.get(&field_name) {
                                    return Some((table_idx, field_name, fi.expr, access));
                                }
                                // Check parent classes
                                for &parent_idx in &self.table(table_idx).parent_classes.clone() {
                                    if let Some(fi) = self.table(parent_idx).fields.get(&field_name) {
                                        return Some((parent_idx, field_name, fi.expr, access));
                                    }
                                }
                            }
            return None;
        }
        let our_index = names.iter().position(|n| n.text_range() == token.text_range())?;
        if our_index == 0 {
            // Check if grandparent has a FunctionCall: for `func().field.sub`, cursor is on "field"
            // which is names[0] in the inner Identifier, but the root is the FunctionCall in grandparent
            if let Some(grandparent) = parent.parent()
                && grandparent.kind() .is_identifier()
                    && let Some(funcall_node) = grandparent.children().find(|c| c.kind() == SyntaxKind::FunctionCall || c.kind() == SyntaxKind::MethodCall)
                        && let Some(table_idx) = self.resolve_funcall_node_to_table(&funcall_node, text_size) {
                            let field_name = names[0].text().to_string();
                            let access = Self::detect_access_before_token(&parent, &token);
                            if let Some(fi) = self.table(table_idx).fields.get(&field_name) {
                                return Some((table_idx, field_name, fi.expr, access));
                            }
                            for &parent_idx in &self.table(table_idx).parent_classes.clone() {
                                if let Some(fi) = self.table(parent_idx).fields.get(&field_name) {
                                    return Some((parent_idx, field_name, fi.expr, access));
                                }
                            }
                        }
            return None; // Root name is a symbol, handled by find_symbol_at
        }

        // Resolve chain: root symbol → table → field
        let root_name = names[0].text().to_string();
        let scope_idx = self.scope_at_offset(text_size)?;
        // Check if grandparent has a FunctionCall: for `func().a.b`, cursor is on "b" and
        // names = ["a", "b"] in the inner Identifier, with "a" as root but not a symbol.
        let mut table_idx = if let Some(symbol_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
            let ver = self.sym(symbol_idx).versions.last()?;
            let resolved = ver.resolved_type.as_ref()?;
            Self::extract_table_idx(resolved)?
        } else if let Some(grandparent) = parent.parent() {
            // Root name is not a symbol; check if grandparent has a FunctionCall
            if grandparent.kind() .is_identifier() {
                if let Some(funcall_node) = grandparent.children().find(|c| c.kind() == SyntaxKind::FunctionCall || c.kind() == SyntaxKind::MethodCall) {
                    let base_table = self.resolve_funcall_node_to_table(&funcall_node, text_size)?;
                    let fi = self.table(base_table).fields.get(&root_name)
                        .or_else(|| self.table(base_table).parent_classes.clone().iter()
                            .find_map(|&p| self.table(p).fields.get(&root_name)))?;
                    let ft = self.resolve_field_type(fi)?;
                    Self::extract_table_idx(&ft)?
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            return None;
        };

        // Walk intermediate fields
        for name_token in &names[1..our_index] {
            let name = name_token.text().to_string();
            // Check for transparent @accessor — skip without changing table
            if self.ir.has_accessor(table_idx, &name) {
                continue;
            }
            table_idx = self.resolve_field_or_g_env(table_idx, &name)?;
        }

        // Look up the target field, checking parent classes if not found directly
        let field_name = names[our_index].text().to_string();
        let access = Self::detect_access_before_token(&parent, &token);
        if let Some(fi) = self.get_field(table_idx, &field_name) {
            return Some((table_idx, field_name, fi.expr, access));
        }
        for &parent_idx in &self.table(table_idx).parent_classes.clone() {
            if let Some(fi) = self.get_field(parent_idx, &field_name) {
                return Some((parent_idx, field_name, fi.expr, access));
            }
        }
        self.resolve_g_env_field(table_idx, &field_name, access)
    }

    /// When `table_idx` is the global environment (`_G`), look up `field_name` as a
    /// scope-0 symbol and return its `type_source` expression. Used as a fallback in
    /// `resolve_field_chain_at` after normal field/parent-class lookup fails.
    fn resolve_g_env_field(&self, table_idx: TableIndex, field_name: &str, access: FieldAccessKind) -> Option<(TableIndex, String, ExprId, FieldAccessKind)> {
        if !self.ir.is_global_env(table_idx) { return None; }
        let sym_id = SymbolIdentifier::Name(field_name.to_string());
        let sym_idx = self.ir.scopes[0].symbols.get(&sym_id).copied()
            .or_else(|| self.ir.ext.scope0_symbols.get(&sym_id).copied());
        if let Some(si) = sym_idx
            && let Some(source) = self.sym(si).versions.last().and_then(|v| v.type_source) {
                return Some((table_idx, field_name.to_string(), source, access));
            }
        None
    }

    /// Walk one step in a field chain, falling back to global-symbol resolution when
    /// the current table is the `_G` environment. Returns the next table index.
    fn resolve_field_or_g_env(&self, idx: TableIndex, name: &str) -> Option<TableIndex> {
        if let Some(fi) = self.get_field(idx, name) {
            if let Some(ft) = self.resolve_field_type(fi)
                && let Some(table_idx) = Self::extract_table_idx(&ft)
            {
                return Some(table_idx);
            }
            // Own field exists but couldn't resolve to a table. For class tables,
            // try parent classes for the same field with a resolvable type.
            // This handles self-referential patterns (X.field = X.field:Method())
            // where the own field's expression can't resolve due to the cycle.
            // (Mirrors the same guard in resolve.rs FieldAccess and
            // queries.rs resolve_expr_type_impl.)
            let tbl = self.table(idx);
            if tbl.class_name.is_some() {
                for &parent_idx in &tbl.parent_classes.clone() {
                    if let Some(pfi) = self.ir.get_field(parent_idx, name)
                        && let Some(pft) = self.resolve_field_type(pfi)
                        && !matches!(pft, ValueType::Table(None))
                        && let Some(table_idx) = Self::extract_table_idx(&pft)
                    {
                        return Some(table_idx);
                    }
                }
            }
            return None;
        }
        if self.ir.is_global_env(idx) {
            let global_type = self.resolve_global_symbol_type(name)?;
            return Self::extract_table_idx(&global_type);
        }
        None
    }

    /// Detect whether the separator before a Name token in an Identifier is a colon or dot.
    fn detect_access_before_token(parent: &SyntaxNode, token: &SyntaxToken) -> FieldAccessKind {
        let token_start = token.text_range().start();
        let mut last_sep = FieldAccessKind::Dot;
        for t in parent.children_with_tokens().filter_map(|it| it.into_token()) {
            if t.text_range().start() >= token_start {
                break;
            }
            match t.kind() {
                SyntaxKind::Colon => last_sep = FieldAccessKind::Colon,
                SyntaxKind::Dot => last_sep = FieldAccessKind::Dot,
                _ => {}
            }
        }
        last_sep
    }

    /// Resolve a method call's return type to a table index: look up the method on
    /// `receiver_table` (including parent classes), handle `@return self`, then delegate
    /// to `resolve_func_return_table` for backtick-generic / `@defclass` / annotation
    /// resolution. `call_node` is the MethodCall/FunctionCall syntax node — required so
    /// that `resolve_func_return_table` can extract string literal arguments.
    fn resolve_method_call_return_table(&self, receiver_table: TableIndex, method_name: &str, call_node: &SyntaxNode) -> Option<TableIndex> {
        let field_expr = self.get_field(receiver_table, method_name).map(|fi| fi.expr)
            .or_else(|| {
                self.table(receiver_table).parent_classes.clone().iter()
                    .find_map(|&p| self.get_field(p, method_name).map(|fi| fi.expr))
            })?;
        let func_type = self.resolve_expr_type(field_expr)?;
        let func_idx = match func_type {
            ValueType::Function(Some(idx)) => idx,
            _ => return None,
        };
        if self.func(func_idx).returns_self {
            return Some(receiver_table);
        }
        self.resolve_func_return_table(func_idx, call_node)
    }

    /// Resolve a function call's return type to a table index.
    /// `call_node` is the syntax node of the call — needed for backtick generic and
    /// `@defclass` resolution (both extract string literal arguments from the call site).
    fn resolve_func_return_table(&self, func_idx: FunctionIndex, call_node: &SyntaxNode) -> Option<TableIndex> {
        // For @defclass functions, resolve the class from the string literal argument
        let func_info = self.func(func_idx);
        if func_info.defclass.is_some()
            && let Some(arg_list) = call_node.children().find(|c| c.kind() == SyntaxKind::ArgumentList) {
                // Get first string literal argument
                for child in arg_list.descendants_with_tokens() {
                    if let NodeOrToken::Token(t) = child
                        && t.kind() == SyntaxKind::String {
                            let class_name = t.text().trim_matches(|c| c == '"' || c == '\'').to_string();
                            if let Some(&idx) = self.ir.classes.get(&class_name) {
                                return Some(idx);
                            }
                            // Check external classes
                            if let Some(&idx) = self.ir.ext.classes.get(&class_name) {
                                return Some(idx);
                            }
                        }
                }
            }
        // For backtick generic functions (e.g. `@generic T` + `@param name \`T\`` + `@return T`),
        // resolve the class from the string literal at the backtick parameter position.
        if !func_info.generics.is_empty()
            && let Some(result) = self.resolve_backtick_generic_return(func_idx, call_node) {
                return Some(result);
            }
        let ret_id = SymbolIdentifier::FunctionRet(func_idx, 0);
        let ret_sym_idx = self.get_symbol(&ret_id, func_info.scope)?;
        let ret_type = self.sym(ret_sym_idx).versions.first()?.resolved_type.as_ref()?;
        Self::extract_table_idx(ret_type)
    }

    /// For functions with backtick generic params (e.g. `@generic T` + `@param name \`T\`` + `@return T`),
    /// extract the string literal from the call node at the backtick parameter position
    /// and resolve it to a class table index.
    fn resolve_backtick_generic_return(&self, func_idx: FunctionIndex, call_node: &SyntaxNode) -> Option<TableIndex> {
        let func_info = self.func(func_idx).clone();
        let generic_names: Vec<&str> = func_info.generics.iter().map(|(n, _)| n.as_str()).collect();

        // Check if the return type references a generic name
        let return_generic = func_info.return_annotations.first().and_then(|ret| {
            match ret {
                ValueType::TypeVariable(name) if generic_names.contains(&name.as_str()) => Some(name.clone()),
                _ => None,
            }
        })?;

        // Find which param annotation has a backtick for this generic
        let self_offset = func_info.args.first().is_some_and(|&sym| {
            matches!(&self.sym(sym).id, SymbolIdentifier::Name(n) if n == "self")
        });
        let self_off = if self_offset { 1usize } else { 0 };
        let mut backtick_arg_index = None;
        for (ann_idx, ann) in func_info.param_annotations.iter().enumerate() {
            if let crate::annotations::AnnotationType::Backtick(inner) = ann
                && let crate::annotations::AnnotationType::Simple(name) = inner.as_ref()
                    && name == &return_generic {
                        backtick_arg_index = Some(ann_idx.saturating_sub(self_off));
                        break;
                    }
        }
        let target_idx = backtick_arg_index?;

        // Extract the string literal at that argument position from the call node
        let arg_list = call_node.children().find(|c| c.kind() == SyntaxKind::ArgumentList)?;
        let arg_exprs: Vec<_> = arg_list.children()
            .filter(|c| Expression::cast(*c).is_some())
            .collect();
        let target_expr = arg_exprs.get(target_idx)?;
        // Find the string token in this expression
        let string_token = target_expr.descendants_with_tokens()
            .find_map(|child| {
                if let NodeOrToken::Token(t) = child
                    && t.kind() == SyntaxKind::String { return Some(t); }
                None
            })?;
        let class_name = string_token.text().trim_matches(|c| c == '"' || c == '\'').to_string();
        // Skip primitive type names — they don't resolve to class tables
        if crate::annotations::resolve_primitive_type_name(&class_name).is_some() {
            return None;
        }
        self.ir.classes.get(&class_name).copied()
            .or_else(|| self.ir.ext.classes.get(&class_name).copied())
    }

    /// Check if a table has @constructor (own or inherited from parent classes).
    fn has_constructor(&self, table_idx: TableIndex) -> bool {
        if !self.table(table_idx).constructors.is_empty() {
            return true;
        }
        self.table(table_idx).parent_classes.clone().iter()
            .any(|&p| !self.table(p).constructors.is_empty())
    }

    /// Resolve a FunctionCall syntax node to the table its return type represents.
    /// Handles colon method calls, dot-calls, and chained combinations.
    fn resolve_funcall_node_to_table(&self, node: &SyntaxNode, scope_offset: TextSize) -> Option<TableIndex> {
        // Special-case: select(2, ...) → addon namespace table
        if let Some(expr) = Expression::cast(*node)
            && let Some(2) = crate::annotations::is_select_varargs(&expr)
        {
            return self.ir.addon_table_idx();
        }

        // Parser2 MethodCall: receiver:method(args) where receiver, Colon, Name, ArgList are direct children
        if node.kind() == SyntaxKind::MethodCall {
            let method_name = node.children_with_tokens()
                .filter_map(|it| it.into_token())
                .find(|t| t.kind() == SyntaxKind::Name)?
                .text().to_string();
            let is_call_node = |k: SyntaxKind| k == SyntaxKind::FunctionCall || k == SyntaxKind::MethodCall;
            let receiver_table = if let Some(funcall_node) = node.children().find(|c| is_call_node(c.kind())) {
                self.resolve_funcall_node_to_table(&funcall_node, scope_offset)?
            } else if let Some(ident_node) = node.children().find(|c| c.kind().is_identifier()) {
                self.resolve_identifier_to_table(&ident_node, scope_offset)?
            } else if let Some(vt) = Self::resolve_literal_receiver_type(node) {
                // String literal receiver: "str":method() or ("str"):method()
                let mut indices = Vec::new();
                self.ir.collect_library_table_indices(&vt, &mut indices);
                *indices.first()?
            } else {
                return None;
            };
            return self.resolve_method_call_return_table(receiver_table, &method_name, node);
        }

        if let Some(ident_node) = node.children().find(|c| c.kind() .is_identifier()) {
            let has_colon = ident_node.children_with_tokens().any(|t|
                t.as_token().is_some_and(|tok| tok.kind() == SyntaxKind::Colon));

            let names: Vec<_> = ident_node.children_with_tokens()
                .filter_map(|it| it.into_token())
                .filter(|t| t.kind() == SyntaxKind::Name)
                .collect();

            if has_colon {
                // Colon method call: receiver:method(args)
                let method_name = names.last()?.text().to_string();
                let is_call = |k: SyntaxKind| k == SyntaxKind::FunctionCall || k == SyntaxKind::MethodCall;
                let receiver_table = if let Some(child_funcall) = ident_node.children().find(|c| is_call(c.kind())) {
                    self.resolve_funcall_node_to_table(&child_funcall, scope_offset)?
                } else if let Some(child_ident) = ident_node.children().find(|c| c.kind().is_identifier()) {
                    self.resolve_identifier_to_table(&child_ident, scope_offset)?
                } else if names.len() >= 2 {
                    let root_name = names[0].text().to_string();
                    let scope_idx = self.scope_at_offset(scope_offset)?;
                    let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
                    let ver = self.sym(symbol_idx).versions.last()?;
                    let resolved = ver.resolved_type.as_ref()?;
                    let mut idx = Self::extract_table_idx(resolved)?;
                    for name_token in &names[1..names.len() - 1] {
                        let name = name_token.text().to_string();
                        let fi = self.get_field(idx, &name)?;
                        let ft = self.resolve_field_type(fi)?;
                        idx = Self::extract_table_idx(&ft)?;
                    }
                    idx
                } else {
                    return None;
                };
                return self.resolve_method_call_return_table(receiver_table, &method_name, node);
            } else {
                // Dot-call or simple call: func(args) or obj.func(args)
                // Resolve the identifier as a dot chain to find the function
                let func_name = names.last()?.text().to_string();
                // Check for nested child nodes (parser2 DotAccess has child NameRef + single Name)
                let is_call2 = |k: SyntaxKind| k == SyntaxKind::FunctionCall || k == SyntaxKind::MethodCall;
                let child_funcall_node = ident_node.children().find(|c| is_call2(c.kind()));
                let child_ident_node = if child_funcall_node.is_none() {
                    ident_node.children().find(|c| c.kind().is_identifier())
                } else {
                    None
                };
                let has_child = child_funcall_node.is_some() || child_ident_node.is_some();
                if names.len() >= 2 || has_child {
                    // Dot chain or parser2 DotAccess: resolve base → function field
                    let base_table = if let Some(cf) = child_funcall_node {
                        self.resolve_funcall_node_to_table(&cf, scope_offset)?
                    } else if let Some(ci) = child_ident_node {
                        self.resolve_identifier_to_table(&ci, scope_offset)?
                    } else {
                        // Simple dot chain with no nested nodes (old parser)
                        let root_name = names[0].text().to_string();
                        let scope_idx = self.scope_at_offset(scope_offset)?;
                        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
                        let ver = self.sym(symbol_idx).versions.last()?;
                        let resolved = ver.resolved_type.as_ref()?;
                        let mut idx = Self::extract_table_idx(resolved)?;
                        for name_token in &names[1..names.len() - 1] {
                            let name = name_token.text().to_string();
                            let fi = self.get_field(idx, &name)?;
                            let ft = self.resolve_field_type(fi)?;
                            idx = Self::extract_table_idx(&ft)?;
                        }
                        idx
                    };
                    let fi = self.get_field(base_table, &func_name)
                        .or_else(|| self.table(base_table).parent_classes.clone().iter()
                            .find_map(|&p| self.get_field(p, &func_name)))?;
                    let func_type = self.resolve_expr_type(fi.expr)?;
                    let func_idx = match func_type {
                        ValueType::Function(Some(idx)) => idx,
                        _ => return None,
                    };
                    return self.resolve_func_return_table(func_idx, node);
                } else {
                    // Simple function call: func(args)
                    let root_name = names[0].text().to_string();
                    let scope_idx = self.scope_at_offset(scope_offset)?;
                    let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
                    let ver = self.sym(symbol_idx).versions.last()?;
                    let resolved = ver.resolved_type.as_ref()?;
                    match resolved {
                        ValueType::Function(Some(func_idx)) => {
                            return self.resolve_func_return_table(*func_idx, node);
                        }
                        ValueType::Table(Some(table_idx)) => {
                            // Constructor call: class table called as function
                            if let Some(call_func_idx) = self.table(*table_idx).call_func {
                                return self.resolve_func_return_table(call_func_idx, node);
                            }
                            // @constructor: class table is callable, returns the class type
                            if self.has_constructor(*table_idx) {
                                return Some(*table_idx);
                            }
                            return None;
                        }
                        _ => return None,
                    }
                }
            }
        }

        // Pattern 2: FunctionCall with direct Colon child (outer chained call)
        let has_colon = node.children_with_tokens().any(|t|
            t.as_token().is_some_and(|tok| tok.kind() == SyntaxKind::Colon));
        if !has_colon {
            return None;
        }
        let method_name = node.children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::Name)?
            .text().to_string();
        let is_call3 = |k: SyntaxKind| k == SyntaxKind::FunctionCall || k == SyntaxKind::MethodCall;
        let receiver_table = if let Some(funcall_node) = node.children().find(|c| is_call3(c.kind())) {
            self.resolve_funcall_node_to_table(&funcall_node, scope_offset)?
        } else if let Some(ident_node) = node.children().find(|c| c.kind().is_identifier()) {
            self.resolve_identifier_to_table(&ident_node, scope_offset)?
        } else {
            return None;
        };
        self.resolve_method_call_return_table(receiver_table, &method_name, node)
    }

    /// Resolve an Identifier syntax node to the table it represents.
    /// Handles simple dot chains and bracket-indexed chains (e.g. `t.f[k]`).
    fn resolve_identifier_to_table(&self, node: &SyntaxNode, scope_offset: TextSize) -> Option<TableIndex> {
        let child_names: Vec<_> = node.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();

        // Check for nested Identifier (bracket indexing like private.tbl[k])
        // For parser2, MethodCall is also a call-like node that should be resolved through return type,
        // not as a pure identifier. So check for FunctionCall/MethodCall first.
        let is_call_node = |k: SyntaxKind| k == SyntaxKind::FunctionCall || k == SyntaxKind::MethodCall;
        let child_funcall = node.children().find(|c| is_call_node(c.kind()));
        let child_ident = if child_funcall.is_none() {
            node.children().find(|c| c.kind().is_identifier())
        } else {
            None
        };
        let has_bracket = node.children_with_tokens().any(|t|
            t.as_token().is_some_and(|tok| tok.kind() == SyntaxKind::LeftSquareBracket));

        let table_idx = if let Some(child) = child_ident {
            // Resolve child identifier first
            let inner_idx = self.resolve_identifier_to_table(&child, scope_offset)?;
            if has_bracket {
                // Bracket index: get value_type
                let value_type = self.table(inner_idx).value_type.as_ref()?;
                let bracket_idx = Self::extract_table_idx(value_type)?;
                // Chain any remaining direct Name tokens as field accesses
                let mut idx = bracket_idx;
                for name_tok in &child_names {
                    let name = name_tok.text().to_string();
                    let fi = self.get_field(idx, &name)?;
                    let ft = self.resolve_field_type(fi)?;
                    idx = Self::extract_table_idx(&ft)?;
                }
                idx
            } else if !child_names.is_empty() {
                // Chain direct Name tokens as field accesses (parser2 DotAccess has
                // child NameRef for the base and direct Name for the field)
                let mut idx = inner_idx;
                for name_tok in &child_names {
                    let name = name_tok.text().to_string();
                    idx = self.resolve_field_or_g_env(idx, &name)?;
                }
                idx
            } else {
                inner_idx
            }
        } else if let Some(funcall_node) = child_funcall {
            // FunctionCall child: resolve call return type to table, then chain fields
            let mut idx = self.resolve_funcall_node_to_table(&funcall_node, scope_offset)?;
            for name_tok in &child_names {
                let name = name_tok.text().to_string();
                let fi = self.table(idx).fields.get(&name)
                    .or_else(|| self.table(idx).parent_classes.clone().iter()
                        .find_map(|&p| self.table(p).fields.get(&name)))?;
                let ft = self.resolve_field_type(fi)?;
                idx = Self::extract_table_idx(&ft)?;
            }
            idx
        } else if let Some(first) = child_names.first() {
            // Simple dot chain
            let root_name = first.text().to_string();
            let scope_idx = self.scope_at_offset(scope_offset)?;
            let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
            let ver = self.sym(symbol_idx).versions.last()?;
            let resolved = ver.resolved_type.as_ref()?;
            // Apply type narrowing (e.g. from @type-narrows guards) so field lookups
            // use the narrowed type instead of the base type.
            let mut idx = self.get_type_narrowing(symbol_idx, scope_idx)
                .and_then(Self::extract_table_idx)
                .or_else(|| Self::extract_table_idx(resolved))?;
            for name_token in &child_names[1..] {
                let name = name_token.text().to_string();
                idx = self.resolve_field_or_g_env(idx, &name)?;
            }
            idx
        } else {
            return None;
        };
        Some(table_idx)
    }

    /// Resolve an identifier node to its full resolved type (intersection-aware).
    /// Handles both simple single-name identifiers (`foo`) and chained dot access
    /// (`self.Sidebar.ActionBtn`), walking field accesses iteratively while
    /// preserving the full ValueType (including intersections).
    fn resolve_identifier_to_type(&self, node: &SyntaxNode, scope_offset: TextSize) -> Option<ValueType> {
        // Only handles NameRef and DotAccess chains. BracketAccess involves index
        // resolution that this function doesn't support — bail out so the caller
        // can fall through to the table-based resolution path.
        if node.kind() == SyntaxKind::BracketAccess {
            return None;
        }
        // Collect all DotAccess/NameRef nodes bottom-up, then resolve from the
        // root outward. This avoids recursion (and potential stack overflow on
        // pathological inputs with deeply nested dot chains).
        let mut chain = vec![*node];
        loop {
            let current = *chain.last().unwrap();
            let has_child_call = current.children().any(|c|
                c.kind() == SyntaxKind::FunctionCall || c.kind() == SyntaxKind::MethodCall);
            if has_child_call {
                return None;
            }
            if let Some(child_ident) = current.children().find(|c|
                c.kind() == SyntaxKind::DotAccess || c.kind() == SyntaxKind::NameRef)
            {
                chain.push(child_ident);
            } else if current.children().any(|c| c.kind().is_identifier()) {
                // Child is an identifier kind we can't handle (e.g. BracketAccess)
                return None;
            } else {
                break;
            }
        }

        // The deepest node (last in chain) must be the root single-name identifier.
        let root_node = chain.last()?;
        let root_names: Vec<_> = root_node.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();
        if root_names.len() != 1 {
            return None;
        }
        let root_name = root_names[0].text().to_string();
        let scope_idx = self.scope_at_offset(scope_offset)?;
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
        let mut current_type = self.get_type_narrowing(symbol_idx, scope_idx)
            .cloned()
            .or_else(|| {
                let ver = self.sym(symbol_idx).versions.last()?;
                ver.resolved_type.clone()
            })?;

        // Walk from root outward through each intermediate node's Name tokens.
        for ancestor in chain.iter().rev().skip(1) {
            let field_names: Vec<_> = ancestor.children_with_tokens()
                .filter_map(|it| it.into_token())
                .filter(|t| t.kind() == SyntaxKind::Name)
                .collect();
            for name_tok in &field_names {
                let name = name_tok.text().to_string();
                let indices = Self::extract_all_table_indices(&current_type);
                let fi = indices.iter().find_map(|&idx| self.get_field(idx, &name))?;
                current_type = self.resolve_field_type(fi)?;
            }
        }

        Some(current_type)
    }

    /// Resolve a receiver (identifier, funcall, grouped expression, or string literal)
    /// to all table indices (intersection-aware).
    /// Returns all table members from the resolved type, not just the first.
    fn resolve_receiver_to_all_tables(&self, parent: &SyntaxNode, scope_offset: TextSize) -> Vec<TableIndex> {
        let is_call_node = |k: SyntaxKind| k == SyntaxKind::FunctionCall || k == SyntaxKind::MethodCall;
        // Try resolving the receiver's full type for intersection-aware lookup
        if let Some(ident_node) = parent.children().find(|c| c.kind().is_identifier())
            && let Some(resolved) = self.resolve_identifier_to_type(&ident_node, scope_offset) {
                let mut indices = Self::extract_all_table_indices(&resolved);
                // Primitive types with implicit metatables (e.g. string → string library).
                // Handles bare String and String inside unions (e.g. string | nil).
                self.ir.collect_library_table_indices(&resolved, &mut indices);
                if !indices.is_empty() {
                    return indices;
                }
            }
        // Handle string literal receivers: ("str"):method() or "str":method()
        if let Some(vt) = Self::resolve_literal_receiver_type(parent) {
            let mut indices = Vec::new();
            self.ir.collect_library_table_indices(&vt, &mut indices);
            if !indices.is_empty() {
                return indices;
            }
        }
        // Fallback: single table from existing resolution
        let table_idx = if let Some(funcall_node) = parent.children().find(|c| is_call_node(c.kind())) {
            self.resolve_funcall_node_to_table(&funcall_node, scope_offset)
        } else if let Some(ident_node) = parent.children().find(|c| c.kind().is_identifier()) {
            self.resolve_identifier_to_table(&ident_node, scope_offset)
        } else {
            None
        };
        table_idx.into_iter().collect()
    }

    /// Check if a node contains a string literal (directly or inside a GroupedExpression).
    /// Returns `Some(ValueType::String(None))` for string literal receivers.
    fn resolve_literal_receiver_type(node: &SyntaxNode) -> Option<ValueType> {
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Literal => {
                    if child.children_with_tokens().any(|t|
                        t.as_token().is_some_and(|tok| tok.kind() == SyntaxKind::String)) {
                        return Some(ValueType::String(None));
                    }
                }
                SyntaxKind::GroupedExpression => {
                    return Self::resolve_literal_receiver_type(&child);
                }
                _ => {}
            }
        }
        None
    }

    /// Resolve a field name inside a table constructor (e.g. `components` in `{ components = {} }`).
    /// Returns (field_name, field_info) if the token at offset is a named field key.
    pub(crate) fn find_constructor_field_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(String, FieldInfo)> {
        let text_size = TextSize::from(offset);
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(left, right) => {
                if right.kind() == SyntaxKind::Name { right }
                else if left.kind() == SyntaxKind::Name { left }
                else { return None; }
            }
            TokenAtOffset::None => return None,
        };
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        // Field names in constructors are wrapped: Field > Identifier > Name
        let parent = token.parent()?;
        let field_node = if parent.kind() .is_identifier() {
            let grandparent = parent.parent()?;
            if grandparent.kind() != SyntaxKind::Field { return None; }
            grandparent
        } else if parent.kind() == SyntaxKind::Field {
            parent
        } else {
            return None;
        };
        // Check this is a named field (has an = sign)
        let has_assign = field_node.children_with_tokens().any(|n| {
            matches!(n, NodeOrToken::Token(ref t) if t.kind() == SyntaxKind::Assign)
        });
        if !has_assign {
            return None;
        }
        let field_name = token.text().to_string();
        // Walk ancestors to find the TableConstructor
        let tc_node = field_node.ancestors().find(|n| n.kind() == SyntaxKind::TableConstructor)?;
        let r = tc_node.text_range();
        let key = (u32::from(r.start()), u32::from(r.end()));
        let table_idx = self.ir.table_ranges.get(&key)?;
        let field_info = self.get_field(*table_idx, &field_name)?.clone();
        Some((field_name, field_info))
    }

    /// Resolve the cross-file identity of the symbol or field at `offset`.
    /// Returns a `ReferenceTarget` whose index (symbol_idx / table_idx) is stable across
    /// any `AnalysisResult` built from the same `PreResolvedGlobals` when the index is
    /// `>= EXT_BASE`. Local-to-file identities (`idx < EXT_BASE`) are only meaningful
    /// to `self` and shouldn't be used for cross-file search.
    pub fn reference_target_at(&self, tree: &SyntaxTree, offset: u32) -> Option<ReferenceTarget> {
        if let Some((symbol_idx, name, _)) = self.find_symbol_at(tree, offset) {
            Some(ReferenceTarget::Symbol { idx: symbol_idx, name })
        } else if let Some((table_idx, field_name, _, _)) = self.resolve_field_chain_at(tree, offset) {
            Some(ReferenceTarget::Field { table_idx, field_name })
        } else if let Some((sym_idx, name, _)) = self.find_param_in_annotation_at(tree, offset) {
            Some(ReferenceTarget::Symbol { idx: sym_idx, name })
        } else {
            None
        }
    }

    /// If `target` is file-local but has a workspace-wide counterpart (a scope-0
    /// symbol shadowed by the file's own global-function definition, or a local
    /// `@class` table whose name is also registered in `PreResolvedGlobals`),
    /// return the promoted cross-file target. Returns `None` when no promotion
    /// applies (target is already cross-file, or genuinely file-local).
    ///
    /// Callers drive cross-file find-references with the promoted target so that
    /// a rename initiated at the definition site still reaches every consumer
    /// file.
    pub fn promote_to_cross_file(&self, target: &ReferenceTarget) -> Option<ReferenceTarget> {
        match target {
            ReferenceTarget::Symbol { idx, name } if !idx.is_external() => {
                // Only promote globals — symbols declared at scope 0.
                if self.sym(*idx).scope_idx != ScopeIndex(0) {
                    return None;
                }
                let ext_idx = self.ir.ext.scope0_symbols
                    .get(&SymbolIdentifier::Name(name.clone()))
                    .copied()?;
                Some(ReferenceTarget::Symbol { idx: ext_idx, name: name.clone() })
            }
            ReferenceTarget::Field { table_idx, field_name } if !table_idx.is_external() => {
                let class_name = self.table(*table_idx).class_name.clone()?;
                let ext_idx = self.ir.ext.classes.get(&class_name).copied()?;
                Some(ReferenceTarget::Field { table_idx: ext_idx, field_name: field_name.clone() })
            }
            _ => None,
        }
    }

    /// Walk tokens forward from `def_start` (inclusive) up to `def_end` and return the
    /// range of the first `Name`/`Parameter` token whose text equals `name`. This lets
    /// callers translate a statement-level `DefNode` (e.g. a whole `FunctionDefinition`
    /// or `LocalAssignStatement`) into the name-token range that actually appears in
    /// find-references results.
    pub(crate) fn def_name_token_range(&self, tree: &SyntaxTree, def_start: u32, def_end: u32, name: &str) -> Option<TextRange> {
        let start_token = SyntaxNode::new_root(tree)
            .token_at_offset(TextSize::from(def_start))
            .right_biased()?;
        let def_end = TextSize::from(def_end);
        let mut cursor = start_token;
        loop {
            if (cursor.kind() == SyntaxKind::Name || cursor.kind() == SyntaxKind::Parameter)
                && cursor.text() == name
            {
                return Some(cursor.text_range());
            }
            match cursor.next_token() {
                Some(next) if next.text_range().start() < def_end => cursor = next,
                _ => return None,
            }
        }
    }

    /// True when the enclosing statement of `def_start` is a `local`-prefixed declaration
    /// (`local x = ...`, `local function x()`, destructuring `local x, y = ...`, etc.).
    /// Used by the rename path's `strict_shadow` rule to reject truly-local bindings that
    /// happen to share a name with a workspace-wide global.
    pub(crate) fn is_local_declaration_site(&self, tree: &SyntaxTree, def_start: u32) -> bool {
        let Some(token) = SyntaxNode::new_root(tree)
            .token_at_offset(TextSize::from(def_start))
            .right_biased()
        else { return false };
        let mut node = token.parent();
        while let Some(n) = node {
            match n.kind() {
                SyntaxKind::LocalAssignStatement => return true,
                SyntaxKind::FunctionDefinition => {
                    // `local function X() end` — presence of LocalKeyword as a direct child.
                    return n.children_with_tokens()
                        .filter_map(|c| c.into_token())
                        .any(|t| t.kind() == SyntaxKind::LocalKeyword);
                }
                SyntaxKind::Block => return false,
                _ => node = n.parent(),
            }
        }
        false
    }

    /// True when `token` falls inside the initializer (RHS) of the target
    /// symbol's own `local` assignment. In `local x = x`, the RHS `x` is
    /// resolved to the outer/global `x` during build_ir (because non-function
    /// locals are registered after their initializers are lowered), but a
    /// post-hoc scope-based `get_symbol` lookup finds the newly-created local.
    ///
    /// Only applies to `LocalAssignStatement` (not `local function` or
    /// parameters, where the symbol is registered before the body is walked).
    /// Excludes the definition name token itself and tokens in nested scopes
    /// (closures correctly capture the local).
    fn is_in_own_local_init(&self, tree: &SyntaxTree, symbol_idx: SymbolIndex, token: &SyntaxToken<'_>, name: &str) -> bool {
        if symbol_idx.is_external() { return false; }
        let sym = self.sym(symbol_idx);
        let Some(v0) = sym.versions.first() else { return false; };
        let tok_offset = u32::from(token.text_range().start());
        if tok_offset < v0.def_node.start || tok_offset >= v0.def_node.end { return false; }
        // Only LocalAssignStatement — function defs and params register the
        // symbol before their bodies, so references in bodies are valid.
        if !self.is_local_assign_statement(tree, v0.def_node.start) { return false; }
        // Not the definition name token itself
        let Some(def_range) = self.def_name_token_range(tree, v0.def_node.start, v0.def_node.end, name)
        else { return false; };
        if token.text_range() == def_range { return false; }
        // Only if token is in the same scope as the declaration — nested
        // function bodies have their own scope and correctly capture the local.
        self.scope_at_offset(token.text_range().start()) == Some(sym.scope_idx)
    }

    /// True when the enclosing statement of `def_start` is specifically a
    /// `LocalAssignStatement` (i.e. `local x = ...`, NOT `local function`).
    fn is_local_assign_statement(&self, tree: &SyntaxTree, def_start: u32) -> bool {
        let Some(token) = SyntaxNode::new_root(tree)
            .token_at_offset(TextSize::from(def_start))
            .right_biased()
        else { return false };
        let mut node = token.parent();
        while let Some(n) = node {
            match n.kind() {
                SyntaxKind::LocalAssignStatement => return true,
                SyntaxKind::FunctionDefinition | SyntaxKind::Block => return false,
                _ => node = n.parent(),
            }
        }
        false
    }

    /// Find all references to the symbol or field at the given offset.
    /// Returns a list of TextRanges covering each Name token that references the target.
    pub fn references_at(&self, tree: &SyntaxTree, offset: u32, include_declaration: bool) -> Option<Vec<TextRange>> {
        let target = self.reference_target_at(tree, offset)?;
        let results = self.references_for_target(tree, &target, include_declaration, false);
        if results.is_empty() { None } else { Some(results) }
    }

    /// Compute document highlights at the given byte `offset`.
    ///
    /// When the cursor is on a control-flow keyword, returns all semantically
    /// related keywords (e.g. all `return` statements in a function plus the
    /// `function`/`end` pair; all branch keywords in an `if`-chain; loop
    /// boundary keywords plus every `break`).  Falls back to reference-based
    /// highlights for all other tokens.
    pub fn document_highlights_at(
        &self,
        tree: &SyntaxTree,
        offset: u32,
    ) -> Option<Vec<(TextRange, HighlightKind)>> {
        let root = SyntaxNode::new_root(tree);
        let token = root.token_at_offset(TextSize(offset)).right_biased()?;

        let cf = match token.kind() {
            SyntaxKind::ReturnKeyword | SyntaxKind::FunctionKeyword => {
                token.ancestors()
                    .find(|n| n.kind() == SyntaxKind::FunctionDefinition)
                    .map(hl_function_returns)
            }
            SyntaxKind::BreakKeyword => {
                token.ancestors()
                    .find(|n| LOOP_KINDS.contains(&n.kind()))
                    .map(hl_break_in_loop)
            }
            SyntaxKind::IfKeyword | SyntaxKind::ElseIfKeyword
            | SyntaxKind::ElseKeyword | SyntaxKind::ThenKeyword => {
                token.ancestors()
                    .find(|n| n.kind() == SyntaxKind::IfChain)
                    .map(hl_if_chain)
            }
            SyntaxKind::EndKeyword => {
                token.parent().and_then(|p| match p.kind() {
                    SyntaxKind::FunctionDefinition => Some(hl_function_returns(p)),
                    SyntaxKind::IfChain => Some(hl_if_chain(p)),
                    SyntaxKind::WhileLoop
                    | SyntaxKind::ForCountLoop
                    | SyntaxKind::ForInLoop
                    | SyntaxKind::RepeatUntilLoop => Some(hl_break_in_loop(p)),
                    SyntaxKind::DoBlock => Some(hl_matching_keywords(p,
                        &[SyntaxKind::DoKeyword, SyntaxKind::EndKeyword])),
                    _ => None,
                })
            }
            SyntaxKind::ForKeyword | SyntaxKind::WhileKeyword => {
                token.parent().map(hl_break_in_loop)
            }
            SyntaxKind::DoKeyword => {
                token.parent().and_then(|p| match p.kind() {
                    SyntaxKind::DoBlock => Some(hl_matching_keywords(p,
                        &[SyntaxKind::DoKeyword, SyntaxKind::EndKeyword])),
                    SyntaxKind::WhileLoop
                    | SyntaxKind::ForCountLoop
                    | SyntaxKind::ForInLoop => Some(hl_break_in_loop(p)),
                    _ => None,
                })
            }
            SyntaxKind::RepeatKeyword | SyntaxKind::UntilKeyword => {
                token.parent().map(hl_break_in_loop)
            }
            _ => None,
        };

        if let Some(highlights) = cf
            && !highlights.is_empty()
        {
            return Some(highlights);
        }

        // Fallback: symbol/field reference highlighting.
        let refs = self.references_at(tree, offset, true)?;
        Some(refs.into_iter().map(|r| (r, HighlightKind::Text)).collect())
    }

    /// Return all reference ranges for a file-local symbol at `offset`, including
    /// the declaration. Returns `None` for external symbols, fields, and scope-0
    /// globals that have cross-file counterparts (those should use full rename).
    pub fn linked_editing_ranges_at(&self, tree: &SyntaxTree, offset: u32) -> Option<Vec<TextRange>> {
        let (symbol_idx, name, _) = self.find_symbol_at(tree, offset)?;
        if symbol_idx.is_external() {
            return None;
        }
        if self.sym(symbol_idx).scope_idx == ScopeIndex(0)
            && !self.is_local_declaration_site(tree, self.sym(symbol_idx).versions[0].def_node.start)
        {
            return None;
        }
        let target = ReferenceTarget::Symbol { idx: symbol_idx, name };
        let results = self.references_for_target(tree, &target, true, true);
        if results.is_empty() { None } else { Some(results) }
    }

    /// Find references in `tree` that match `target`. Unlike `references_at`, this accepts
    /// an externally-resolved target so the same search can be run across multiple files'
    /// analyses (for cross-file find-references).
    ///
    /// `include_declaration`: when `false`, suppress definition-site tokens in the
    /// results. For an external target, that means dropping the first-version def-node
    /// of any shadow local accepted via the scope-0 shadow rule (the file that owns the
    /// global). For a local target, it drops the symbol's own first-version def-node.
    ///
    /// `strict_shadow`: when `true`, reject scope-0 shadow locals whose first version
    /// was declared with `local` / `local function`. Rename uses this to avoid rewriting
    /// a truly-local variable that happens to share a name with a workspace-wide global.
    /// Callers should only pass cross-file-stable targets (`target.is_cross_file()`)
    /// when searching files other than the file that produced the target.
    pub fn references_for_target(
        &self,
        tree: &SyntaxTree,
        target: &ReferenceTarget,
        include_declaration: bool,
        strict_shadow: bool,
    ) -> Vec<TextRange> {
        match target {
            ReferenceTarget::Symbol { idx: symbol_idx, name } => {
                let symbol_idx = *symbol_idx;
                let mut results = Vec::new();
                // Track shadow locals accepted via the scope-0 shadow rule so we can
                // drop their first-version def-nodes when include_declaration is false.
                let mut shadow_locals: HashSet<SymbolIndex> = HashSet::new();

                // Add definition-site Name tokens from all symbol versions.
                // This catches parameter defs that are outside the function body scope
                // and wouldn't be found by the token walk below. Only applicable to
                // local symbols — external (EXT_BASE+) symbols have no def_node in
                // this file's tree.
                if !symbol_idx.is_external() {
                    for ver in &self.sym(symbol_idx).versions {
                        if let Some(r) = self.def_name_token_range(tree, ver.def_node.start, ver.def_node.end, name) {
                            results.push(r);
                        }
                    }
                }

                for token in SyntaxNode::new_root(tree).descendants_with_tokens().filter_map(|it| it.into_token()) {
                    if token.kind() != SyntaxKind::Name || token.text() != name.as_str() {
                        continue;
                    }
                    // Skip tokens that are part of a field chain (not the root position)
                    if let Some(parent) = token.parent()
                        && parent.kind().is_identifier() {
                            let names: Vec<_> = parent.children_with_tokens()
                                .filter_map(|it| it.into_token())
                                .filter(|t| t.kind() == SyntaxKind::Name)
                                .collect();
                            if names.len() >= 2
                                && let Some(pos) = names.iter().position(|n| n.text_range() == token.text_range())
                                    && pos > 0 {
                                        continue; // This is a field, not a symbol reference
                                    }
                        }
                    let text_size = token.text_range().start();
                    if let Some(scope_idx) = self.scope_at_offset(text_size)
                        && let Some(resolved) = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx) {
                            let accept = if resolved == symbol_idx {
                                // Reject tokens in the initializer of the target
                                // symbol's own local declaration. In `local x = x`,
                                // the RHS `x` was resolved to the outer/global `x`
                                // during build_ir (locals are registered after RHS
                                // lowering), but scope-based get_symbol finds the
                                // local post-construction.
                                !self.is_in_own_local_init(tree, symbol_idx, &token, name)
                            } else if !resolved.is_external()
                                && !symbol_idx.is_external()
                                && self.is_in_own_local_init(tree, resolved, &token, name)
                                && self.get_symbol_excluding(
                                    &SymbolIdentifier::Name(name.clone()),
                                    scope_idx,
                                    resolved,
                                ) == Some(symbol_idx)
                            {
                                // The token is in the RHS of `local shadow = <token> ...`
                                // that shadows the target symbol. In Lua, `local x = x + 1`
                                // means the RHS `x` refers to the outer `x`. The standard
                                // get_symbol finds the shadow (resolved) rather than the
                                // outer symbol; get_symbol_excluding verifies the outer
                                // symbol is actually the target.
                                true
                            } else if symbol_idx.is_external() && !resolved.is_external() {
                                // Cross-file search against an external global: the file that
                                // defines the global (`function X() end` or `X = ...`) also
                                // creates a shadowing scope-0 local with the same name, which
                                // wins over the external in local lookups. Accept such shadows
                                // when a matching external entry exists in pre_globals so the
                                // definition-site token is reached from consumer call sites.
                                //
                                // `strict_shadow` (rename): additionally require the shadow's
                                // first version to come from a non-`local` declaration site
                                // (i.e. a global assignment or `function Name()`), so we don't
                                // rewrite a truly-local `local Name = ...` that happens to
                                // share a name with a workspace-wide global.
                                let sym = self.sym(resolved);
                                let has_ext = self.ir.ext.scope0_symbols
                                    .contains_key(&SymbolIdentifier::Name(name.clone()));
                                let passes_strict = !strict_shadow
                                    || sym.versions.first()
                                        .map(|v| !self.is_local_declaration_site(tree, v.def_node.start))
                                        .unwrap_or(false);
                                let matched = sym.scope_idx == ScopeIndex(0) && has_ext && passes_strict;
                                if matched { shadow_locals.insert(resolved); }
                                matched
                            } else {
                                false
                            };
                            if accept {
                                results.push(token.text_range());
                            }
                        }
                }

                // Deduplicate (def sites may overlap with walk results)
                results.sort_by_key(|r| (r.start(), r.end()));
                results.dedup();

                // Include @param annotation name ranges for parameter symbols.
                // Parameters always live in a function body scope (never scope 0),
                // so skip the O(F) scan for non-parameter symbols.
                if !symbol_idx.is_external()
                    && self.sym(symbol_idx).scope_idx != ScopeIndex(0) {
                    for (fi, func) in self.ir.functions.iter().enumerate() {
                        if func.args.contains(&symbol_idx) {
                            for (pname, range) in self.param_annotation_name_ranges(tree, FunctionIndex(fi)) {
                                if pname == *name {
                                    results.push(range);
                                }
                            }
                            break;
                        }
                    }
                    // Re-sort after adding annotation ranges
                    results.sort_by_key(|r| (r.start(), r.end()));
                    results.dedup();
                }

                // Filter out declaration if not requested. The "declaration" is the
                // name-token inside the first-version def-node (for local targets, the
                // symbol itself; for external targets, any shadow local we accepted).
                // Note: def_node ranges cover the whole statement (e.g. the entire
                // `function X() end`), so we translate to the name-token range before
                // filtering — matching against the full statement range would never hit.
                if !include_declaration {
                    let mut decl_ranges: Vec<TextRange> = Vec::new();
                    let mut collect_decl = |sym_idx: SymbolIndex| {
                        if let Some(v) = self.sym(sym_idx).versions.first()
                            && let Some(r) = self.def_name_token_range(tree, v.def_node.start, v.def_node.end, name) {
                                decl_ranges.push(r);
                            }
                    };
                    if !symbol_idx.is_external() {
                        collect_decl(symbol_idx);
                    }
                    for shadow_idx in &shadow_locals {
                        collect_decl(*shadow_idx);
                    }
                    results.retain(|r| !decl_ranges.contains(r));
                }

                results
            }
            ReferenceTarget::Field { table_idx, field_name } => {
                let table_idx = *table_idx;
                // Field reference: find all Name tokens in dot/colon chains that resolve to the same table+field
                let mut results = Vec::new();
                for token in SyntaxNode::new_root(tree).descendants_with_tokens().filter_map(|it| it.into_token()) {
                    if token.kind() != SyntaxKind::Name || token.text() != field_name.as_str() {
                        continue;
                    }
                    // Must be in a multi-part Identifier and not the root name.
                    // For parser2's DotAccess/MethodCall, the Name token is a direct child
                    // with the base expression as a child node (not a sibling Name token).
                    let parent = match token.parent() {
                        Some(p) if p.kind().is_identifier() => p,
                        _ => continue,
                    };
                    let parent_kind = parent.kind();
                    // For DotAccess/MethodCall: single direct Name is the field, base is a child node
                    let is_parser2_field = matches!(parent_kind, SyntaxKind::DotAccess | SyntaxKind::MethodCall)
                        && parent.children().any(|c| c.kind().is_identifier() || c.kind() == SyntaxKind::FunctionCall || c.kind() == SyntaxKind::MethodCall);
                    // For the old-style flat Identifier, collect the Name tokens once and
                    // reuse them both for locating the root name and walking intermediates.
                    let flat_names: Vec<SyntaxToken<'_>> = if is_parser2_field {
                        Vec::new()
                    } else {
                        let all: Vec<_> = parent.children_with_tokens()
                            .filter_map(|it| it.into_token())
                            .filter(|t| t.kind() == SyntaxKind::Name)
                            .collect();
                        if all.len() < 2 { continue; }
                        all
                    };
                    // Position of the caret-matched token in the flat chain (0 = root name).
                    let our_idx = if is_parser2_field {
                        0
                    } else {
                        match flat_names.iter().position(|n| n.text_range() == token.text_range()) {
                            Some(idx) if idx > 0 => idx,
                            _ => continue,
                        }
                    };
                    let root_name = if is_parser2_field {
                        // Parser2 DotAccess: walk nested identifiers to find root name
                        let Some(ident) = Identifier::cast(parent) else { continue };
                        let chain_names = ident.names();
                        if chain_names.is_empty() { continue; }
                        chain_names[0].clone()
                    } else {
                        flat_names[0].text().to_string()
                    };
                    let text_size = token.text_range().start();
                    let scope_idx = match self.scope_at_offset(text_size) {
                        Some(s) => s,
                        None => continue,
                    };
                    let sym_idx = match self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx) {
                        Some(s) => s,
                        None => continue,
                    };
                    let ver = match self.sym(sym_idx).versions.last() {
                        Some(v) => v,
                        None => continue,
                    };
                    let resolved = match ver.resolved_type.as_ref().and_then(Self::extract_table_idx) {
                        Some(idx) => idx,
                        _ => continue,
                    };
                    let mut cur_table = resolved;
                    let mut matched = true;
                    if !is_parser2_field {
                        // Old-style flat Identifier: walk intermediate names up to (but not including)
                        // our token's position.
                        for name_token in &flat_names[1..our_idx] {
                            let n = name_token.text().to_string();
                            match self.get_field(cur_table, &n) {
                                Some(field_info) => match self.resolve_expr_type(field_info.expr).as_ref().and_then(Self::extract_table_idx) {
                                    Some(next) => cur_table = next,
                                    _ => { matched = false; break; }
                                },
                                None => { matched = false; break; }
                            }
                        }
                    }
                    // For parser2 DotAccess: cur_table is already the direct parent table
                    let accept = if matched && cur_table == table_idx {
                        true
                    } else if matched && table_idx.is_external() && !cur_table.is_external() {
                        // Cross-file field search: the file that declares `@class X` keeps a
                        // local table for it with `class_name = "X"`; fields defined on that
                        // local (e.g. `function X:Method() end`) should be matched for an
                        // external `X` target too.
                        let ext_for_local = self.table(cur_table).class_name.as_ref()
                            .and_then(|n| self.ir.ext.classes.get(n).copied());
                        ext_for_local == Some(table_idx)
                    } else {
                        false
                    };
                    if accept {
                        results.push(token.text_range());
                    }
                }

                // Filter out declaration if not requested.
                if !include_declaration {
                    let mut decl_ranges: Vec<TextRange> = Vec::new();
                    let mut check_table = |tidx: TableIndex, this: &Self| {
                        if tidx.is_external() { return; }
                        if let Some(field) = this.get_field(tidx, field_name)
                            && let Some((ds, de)) = field.def_range
                            && let Some(r) = this.def_name_token_range(tree, ds, de, field_name)
                        {
                            decl_ranges.push(r);
                        }
                    };
                    check_table(table_idx, self);
                    // For external targets, also check local tables with matching class_name.
                    if table_idx.is_external() {
                        for &local_tidx in self.ir.classes.values() {
                            if local_tidx.is_external() { continue; }
                            if let Some(cn) = &self.table(local_tidx).class_name
                                && self.ir.ext.classes.get(cn).copied() == Some(table_idx)
                            {
                                check_table(local_tidx, self);
                            }
                        }
                    }
                    results.retain(|r| !decl_ranges.contains(r));
                }

                results
            }
        }
    }

    /// Parse a `---@param name ...` comment token and extract the param name with its
    /// byte range relative to the comment text start. Returns `(name, start, end)`.
    /// The `?` suffix on optional params is excluded. Skips `self` and `...` params.
    fn extract_param_from_comment(text: &str) -> Option<(&str, usize, usize)> {
        if !text.starts_with("---") {
            return None;
        }
        let stripped = text.trim_start_matches('-');
        let prefix_len = text.len() - stripped.len();
        let trimmed = stripped.trim_start();
        let ws_before = stripped.len() - trimmed.len();
        let rest = trimmed.strip_prefix("@param")?;
        let rest_trimmed = rest.trim_start();
        let ws_after_tag = rest.len() - rest_trimmed.len();
        let name_with_q = rest_trimmed.split(char::is_whitespace).next()?;
        let name = name_with_q.trim_end_matches('?');
        if name.is_empty() || name == "..." || name == "self" {
            return None;
        }
        let name_start = prefix_len + ws_before + "@param".len() + ws_after_tag;
        Some((name, name_start, name_start + name.len()))
    }

    /// For a given function, find the byte ranges of each `@param` name in the
    /// preceding annotation comments. Returns `(param_name, TextRange)` pairs.
    fn param_annotation_name_ranges(
        &self,
        tree: &SyntaxTree,
        func_idx: FunctionIndex,
    ) -> Vec<(String, TextRange)> {
        let func = self.func(func_idx);
        let def_start = func.def_node.start;
        let Some(start_token) = SyntaxNode::new_root(tree)
            .token_at_offset(TextSize::from(def_start))
            .right_biased()
        else {
            return Vec::new();
        };
        // Walk backward from the function's first token through preceding comments
        let mut results = Vec::new();
        let mut tok = start_token.prev_token();
        while let Some(token) = tok {
            let kind = token.kind();
            if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
                tok = token.prev_token();
                continue;
            }
            if kind == SyntaxKind::Comment {
                let text = token.text();
                if text.starts_with("---") {
                    if let Some((name, ns, ne)) = Self::extract_param_from_comment(text) {
                        let token_start = u32::from(token.text_range().start());
                        let range = TextRange::new(
                            TextSize::from(token_start + ns as u32),
                            TextSize::from(token_start + ne as u32),
                        );
                        results.push((name.to_string(), range));
                    }
                    tok = token.prev_token();
                    continue;
                }
            }
            break;
        }
        results
    }

    /// If the cursor offset falls inside a `@param` name within a comment preceding
    /// a function, return the parameter's symbol index, name, and the TextRange of the
    /// name in the comment. This enables rename-from-annotation.
    pub(crate) fn find_param_in_annotation_at(
        &self,
        tree: &SyntaxTree,
        offset: u32,
    ) -> Option<(SymbolIndex, String, TextRange)> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).right_biased()?;
        if token.kind() != SyntaxKind::Comment {
            return None;
        }
        let (name, ns, ne) = Self::extract_param_from_comment(token.text())?;
        let token_start = u32::from(token.text_range().start());
        let abs_start = token_start + ns as u32;
        let abs_end = token_start + ne as u32;
        // Check cursor is within the name range
        if offset < abs_start || offset >= abs_end {
            return None;
        }
        let name_range = TextRange::new(TextSize::from(abs_start), TextSize::from(abs_end));

        // Walk forward from this comment to find the function it annotates
        let mut forward = token.next_token();
        while let Some(t) = forward {
            let k = t.kind();
            if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline || k == SyntaxKind::Comment {
                forward = t.next_token();
                continue;
            }
            // Found a non-trivia token — walk up to find FunctionDefinition
            let mut node = t.parent();
            while let Some(n) = node {
                if n.kind() == SyntaxKind::FunctionDefinition {
                    let r = n.text_range();
                    let fn_start = u32::from(r.start());
                    // Find the function in ir.functions by def_node.start
                    for func in self.ir.functions.iter() {
                        if func.def_node.start == fn_start {
                            // Find matching param symbol
                            for &sym_idx in &func.args {
                                if let SymbolIdentifier::Name(ref sym_name) = self.sym(sym_idx).id
                                    && sym_name == name {
                                        return Some((sym_idx, name.to_string(), name_range));
                                }
                            }
                            return None;
                        }
                    }
                    return None;
                }
                node = n.parent();
            }
            return None;
        }
        None
    }

    /// Validate that the symbol at offset can be renamed. Returns (token_range, current_name).
    /// Rejects external symbols (WoW API stubs) and external table fields.
    pub(crate) fn prepare_rename_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(TextRange, String)> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).right_biased()?;

        if token.kind() == SyntaxKind::Name || token.kind() == SyntaxKind::Parameter {
            let name = token.text().to_string();
            // Try symbol first
            if let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, offset) {
                if symbol_idx.is_external() {
                    return None;
                }
                return Some((token.text_range(), name));
            }
            // Try field
            if let Some((table_idx, _, _, _)) = self.resolve_field_chain_at(tree, offset) {
                if table_idx.is_external() {
                    return None;
                }
                return Some((token.text_range(), name));
            }
        }

        // Try @param name in annotation comment
        if let Some((sym_idx, name, range)) = self.find_param_in_annotation_at(tree, offset)
            && !sym_idx.is_external() {
                return Some((range, name));
        }

        None
    }

    pub(crate) fn resolve_expr_type(&self, expr_id: ExprId) -> Option<ValueType> {
        let mut visited = HashSet::new();
        resolve_expr_type_impl(&self.ir, &self.resolved_expr_cache, expr_id, &mut visited, 0)
    }

    /// Resolve a field's type considering annotation, primary expr, and extra_exprs.
    /// Skips nil primary when extras exist (matches reassignment semantics).
    fn resolve_field_type(&self, fi: &FieldInfo) -> Option<ValueType> {
        if let Some(ref ann) = fi.annotation {
            return Some(ann.clone());
        }
        let mut types: Vec<ValueType> = Vec::new();
        let skip_primary = !fi.extra_exprs.is_empty()
            && matches!(self.resolve_expr_type(fi.expr), Some(ValueType::Nil));
        let exprs: Vec<ExprId> = if skip_primary {
            fi.extra_exprs.clone()
        } else {
            std::iter::once(fi.expr).chain(fi.extra_exprs.clone()).collect()
        };
        for eid in exprs {
            if let Some(vt) = self.resolve_expr_type(eid)
                && !types.contains(&vt) { types.push(vt); }
        }
        if types.is_empty() { None } else { Some(ValueType::make_union(types)) }
    }

    pub(crate) fn format_type(&self, vt: &ValueType) -> String {
        self.format_type_depth(vt, 0)
    }

    fn get_type_args_for_expr(&self, expr_id: ExprId) -> Vec<ValueType> {
        if let Some(args) = self.call_type_args.get(&expr_id) {
            return args.clone();
        }
        let expr = self.expr(expr_id).clone();
        match expr {
            Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::Grouped(inner) => {
                self.get_type_args_for_expr(inner)
            }
            Expr::SymbolRef(sym_idx, ver) => {
                let sym = self.sym(sym_idx);
                if let Some(version) = sym.versions.get(ver) {
                    if !version.type_args.is_empty() {
                        return version.type_args.clone();
                    }
                    if let Some(src_expr) = version.type_source
                        && let Some(args) = self.call_type_args.get(&src_expr) {
                            return args.clone();
                        }
                }
                Vec::new()
            }
            Expr::FieldAccess { table, field, .. } => {
                let table_idx = match self.resolve_expr_type(table) {
                    Some(ValueType::Table(Some(idx))) => idx,
                    _ => return Vec::new(),
                };
                if let Some(cached) = self.field_type_args_cache.get(&(table_idx, field.clone())) {
                    return cached.clone();
                }
                if let Some(fi) = self.table(table_idx).fields.get(&field) {
                    if let Some(args) = self.call_type_args.get(&fi.expr) {
                        return args.clone();
                    }
                    for &extra in &fi.extra_exprs {
                        if let Some(args) = self.call_type_args.get(&extra) {
                            return args.clone();
                        }
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn get_symbol_type_args(&self, sym_idx: SymbolIndex, token_start: u32) -> Vec<ValueType> {
        let ver_idx = self.symbol_version_at.get(&token_start).copied().unwrap_or(0);
        let sym = self.sym(sym_idx);
        if let Some(version) = sym.versions.get(ver_idx) {
            if !version.type_args.is_empty() {
                return version.type_args.clone();
            }
            if let Some(src_expr) = version.type_source
                && let Some(args) = self.call_type_args.get(&src_expr) {
                    return args.clone();
                }
        }
        Vec::new()
    }

    fn format_type_args(&self, type_args: &[ValueType]) -> String {
        type_args.iter()
            .map(|a| self.format_type_depth(a, 1))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn append_type_args_to_class(&self, formatted: &str, vt: &ValueType, type_args: &[ValueType]) -> String {
        if type_args.is_empty() {
            return formatted.to_string();
        }
        // Don't display unresolved generic type variables (e.g. "R" from @generic R).
        // Show just the class name without type args instead of "ClassName<R>".
        if type_args.iter().any(|a| matches!(a, ValueType::TypeVariable(_))) {
            return formatted.to_string();
        }
        if let ValueType::Table(Some(idx)) = vt
            && let Some(ref class_name) = self.table(*idx).class_name
            && formatted.starts_with(class_name.as_str())
        {
            let args_str = self.format_type_args(type_args);
            return format!("{}<{}>", class_name, args_str);
        }
        // Handle nullable parameterized class: Union([Table(class), Nil]) formatted as "ClassName?"
        if let ValueType::Union(members) = vt
            && members.len() == 2
            && members.iter().any(|t| matches!(t, ValueType::Nil))
            && let Some(ValueType::Table(Some(idx))) = members.iter().find(|t| !matches!(t, ValueType::Nil))
            && let Some(ref class_name) = self.table(*idx).class_name
            && formatted.starts_with(class_name.as_str())
        {
            let args_str = self.format_type_args(type_args);
            return format!("{}<{}>?", class_name, args_str);
        }
        formatted.to_string()
    }

    /// Collect accessible fields from one or more tables, deduplicating by name.
    /// Returns sorted, formatted field lines (e.g. `"  name: type"`).
    fn collect_accessible_fields(
        &self,
        table_indices: &[TableIndex],
        enclosing_class: Option<TableIndex>,
    ) -> Vec<String> {
        let indent = "  ";
        let mut seen: HashSet<&str> = HashSet::new();
        let mut fields: Vec<String> = Vec::new();
        for &table_idx in table_indices {
            let table = self.table(table_idx);
            let overlay = self.ir.overlay_fields.get(&table_idx);
            let is_enum = table.enum_kind.is_enum();
            let is_accessible = |fi: &FieldInfo| -> bool {
                match fi.visibility {
                    crate::annotations::Visibility::Public => true,
                    crate::annotations::Visibility::Private => {
                        enclosing_class.is_some_and(|ec| self.same_class(ec, table_idx))
                    }
                    crate::annotations::Visibility::Protected => {
                        enclosing_class.is_some_and(|ec| self.is_subclass_of(ec, table_idx))
                    }
                }
            };
            for (name, field_info) in &table.fields {
                if seen.insert(name.as_str()) && is_accessible(field_info) {
                    fields.push(self.format_enum_field_line(indent, name, field_info, is_enum, 0));
                }
            }
            if let Some(ov) = overlay {
                for (name, field_info) in ov.iter() {
                    if seen.insert(name.as_str()) && is_accessible(field_info) {
                        fields.push(self.format_enum_field_line(indent, name, field_info, is_enum, 0));
                    }
                }
            }
            for &parent_idx in &table.parent_classes {
                let parent_table = self.table(parent_idx);
                for (name, field_info) in &parent_table.fields {
                    if seen.insert(name.as_str()) && is_accessible(field_info) {
                        fields.push(self.format_enum_field_line(indent, name, field_info, is_enum, 0));
                    }
                }
            }
        }
        fields.sort();
        fields
    }

    /// Format a type for hover display, filtering out inaccessible private/protected fields.
    fn format_type_accessible(&self, vt: &ValueType, enclosing_class: Option<TableIndex>) -> String {
        if let ValueType::Table(Some(table_idx)) = vt {
            let table = self.table(*table_idx);
            let overlay = self.ir.overlay_fields.get(table_idx);
            let has_fields = !table.fields.is_empty() || overlay.is_some_and(|o| !o.is_empty());
            let has_parents = !table.parent_classes.is_empty();
            if let Some(ref class_name) = table.class_name {
                if !has_fields && !has_parents {
                    return class_name.clone();
                }
                let fields = self.collect_accessible_fields(&[*table_idx], enclosing_class);
                if fields.is_empty() {
                    return class_name.clone();
                }
                return format!("{} {{\n{}\n}}", class_name, fields.join(",\n"));
            }
        }
        if let ValueType::Intersection(types) = vt {
            // Flatten nested intersections.
            let flat = Self::flatten_intersection(types);

            // Collect table indices from members.
            let table_indices: Vec<TableIndex> = flat.iter()
                .filter_map(|t| if let ValueType::Table(Some(idx)) = t { Some(*idx) } else { None })
                .collect();

            if !table_indices.is_empty() {
                // Dedup: remove members that are ancestors of another member.
                // e.g. if MixinClass : Frame, then Frame & MixinClass → MixinClass
                let deduped: Vec<&ValueType> = flat.iter().copied().filter(|t| {
                    if let ValueType::Table(Some(idx)) = t {
                        // Drop this member if some OTHER table member is a subclass of it
                        !table_indices.iter().any(|&other| other != *idx && self.is_subclass_of(other, *idx))
                    } else {
                        true
                    }
                }).collect();

                // Build header line with deduped member names (depth 1 → class names only).
                // Skip anonymous tables with fields — they'd expand inline in the header
                // but their fields are already shown in the vertical block below.
                let header_parts: Vec<String> = deduped.iter()
                    .filter(|t| {
                        if let ValueType::Table(Some(idx)) = t {
                            let tbl = self.table(*idx);
                            // Keep: named classes, array/map tables. Skip: anonymous field tables.
                            tbl.class_name.is_some() || tbl.value_type.is_some() || tbl.fields.is_empty()
                        } else {
                            true
                        }
                    })
                    .map(|t| self.format_value_type_depth(t, 1))
                    .collect();
                let header = header_parts.join(" & ");

                // If all members were filtered from the header, fall back to compact format.
                if header.is_empty() {
                    return self.format_type(vt);
                }

                let fields = self.collect_accessible_fields(&table_indices, enclosing_class);
                if fields.is_empty() {
                    return header;
                }
                return format!("{} {{\n{}\n}}", header, fields.join(",\n"));
            }
        }
        self.format_type(vt)
    }

    /// Flatten nested `Intersection` types into a single flat list.
    fn flatten_intersection(types: &[ValueType]) -> Vec<&ValueType> {
        let mut flat = Vec::new();
        for t in types {
            if let ValueType::Intersection(inner) = t {
                flat.extend(Self::flatten_intersection(inner));
            } else {
                flat.push(t);
            }
        }
        flat
    }

    pub(crate) fn format_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        self.format_value_type_depth(vt, depth)
    }

    /// Format a type for inlay hints: anonymous shape tables (no class name,
    /// no key/value type) collapse to `table` instead of listing fields inline.
    fn format_type_for_hint(&self, vt: &ValueType) -> String {
        if self.is_anon_shape_table(vt) {
            return "table".to_string();
        }
        if let ValueType::Union(members) = vt
            && members.iter().any(|m| self.is_anon_shape_table(m))
        {
            // Re-format with anonymous tables collapsed
            let collapsed: Vec<String> = members.iter().map(|m| {
                if self.is_anon_shape_table(m) {
                    "table".to_string()
                } else {
                    self.format_type_depth(m, 1)
                }
            }).collect();
            // Apply T? shorthand for two-member unions with nil
            if collapsed.len() == 2 && members.iter().any(|t| matches!(t, ValueType::Nil)) {
                let other = collapsed.iter().find(|s| s.as_str() != "nil").unwrap();
                return format!("{}?", other);
            }
            return collapsed.join(" | ");
        }
        self.format_type_depth(vt, 1)
    }

    fn is_anon_shape_table(&self, vt: &ValueType) -> bool {
        if let ValueType::Table(Some(table_idx)) = vt {
            let table = self.table(*table_idx);
            table.class_name.is_none() && table.value_type.is_none() && table.key_type.is_none()
                && !table.fields.is_empty()
        } else {
            false
        }
    }

    /// For tables whose constructor had array elements that were later mutated
    /// via bracket assignment (e.g. `{strsplit(","  , s)}` then `tbl[i] = tonumber(tbl[i])`),
    /// return the initial element type for display purposes.
    fn initial_array_display(&self, vt: &ValueType) -> Option<String> {
        let ValueType::Table(Some(table_idx)) = vt else { return None };
        let table = self.table(*table_idx);
        let ivt = table.initial_value_type.as_ref()?;
        // Only use initial type when it actually differs from the resolved value_type
        if table.value_type.as_ref() == Some(ivt) { return None; }
        let val_str = self.format_value_type_depth(ivt, 1);
        Some(if matches!(ivt, ValueType::Union(_) | ValueType::Intersection(_)) {
            format!("({})[]", val_str)
        } else {
            format!("{}[]", val_str)
        })
    }

    fn format_field_type(&self, field_info: &FieldInfo, depth: usize) -> String {
        if let Some(ref text) = field_info.annotation_text {
            // annotation_text from format_annotation_type already includes ! for NonNil
            return text.clone();
        }
        if let Some(ref ann) = field_info.annotation {
            let base = self.format_type_depth(ann, depth + 1);
            return if field_info.lateinit { format!("{}!", base) } else { base };
        }
        // Union original expr with any reassignment exprs.
        // If there are reassignments and the initial value is nil,
        // skip the nil — it's just a placeholder initializer.
        let skip_primary = !field_info.extra_exprs.is_empty()
            && matches!(self.resolve_expr_type(field_info.expr), Some(ValueType::Nil));
        let mut types: Vec<ValueType> = Vec::new();
        let exprs: Vec<ExprId> = if skip_primary {
            field_info.extra_exprs.clone()
        } else {
            std::iter::once(field_info.expr).chain(field_info.extra_exprs.iter().copied()).collect()
        };
        for expr_id in exprs {
            if let Some(vt) = self.resolve_expr_type(expr_id)
                && !types.contains(&vt) {
                    types.push(vt);
                }
        }
        if types.is_empty() {
            return "?".to_string();
        }
        let unified = ValueType::make_union(types);
        self.format_type_depth(&unified, depth + 1)
    }

    /// Format the `__call` metamethod signature for a callable table.
    /// Returns `None` if the table has no metamethod-based `call_func`.
    fn format_call_signature(&self, table_idx: TableIndex) -> Option<String> {
        let table = self.table(table_idx);
        let func_idx = table.call_func?;
        if !table.call_func_is_metamethod { return None; }
        let func = self.func(func_idx);
        // The first parameter of a __call metamethod always receives the table
        // being called (the implicit receiver), regardless of its name — skip it
        // so the hover shows only the user-facing parameters.
        let skip = if !func.args.is_empty() { 1 } else { 0 };
        let args: Vec<String> = func.args.iter().enumerate().skip(skip).map(|(i, &sym_idx)| {
            let name = match &self.sym(sym_idx).id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => "?".to_string(),
            };
            let optional = func.param_optional.get(i).copied().unwrap_or(false);
            let ann_has_nil = func.param_annotations.get(i)
                .is_some_and(crate::annotations::annotation_type_is_nullable);
            let suffix = if optional && !ann_has_nil { "?" } else { "" };
            let type_str = self.param_annotation_text(func, i)
                .or_else(|| {
                    self.sym(sym_idx).versions.first()
                        .and_then(|v| v.resolved_type.as_ref())
                        .map(|rt| {
                            let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                            self.format_type_depth(&display_type, 1)
                        })
                });
            match type_str {
                Some(t) => format!("{}{}: {}", name, suffix, t),
                None => format!("{}{}", name, suffix),
            }
        }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let no_subs = HashMap::new();
        let rets: Vec<String> = if func.returns_self {
            vec![self.self_return_text(func, &no_subs)]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                let formatted = self.format_value_type_depth(vt, 1);
                format_vararg_return(formatted, i, func)
            }).collect()
        } else {
            self.format_inferred_returns(func, 1)
        };
        if rets.is_empty() {
            Some(format!("__call({})", all_args.join(", ")))
        } else {
            Some(format!("__call({}): {}", all_args.join(", "), rets.join(", ")))
        }
    }

    /// Append `__call` signature to `type_str` and merge `__call` doc with existing doc.
    fn append_call_hover(&self, table_idx: TableIndex, type_str: &mut String, base_doc: Option<String>) -> Option<String> {
        if let Some(call_sig) = self.format_call_signature(table_idx) {
            *type_str = format!("{}\n\n{}", type_str, call_sig);
        }
        let table = self.table(table_idx);
        if let Some(func_idx) = table.call_func.filter(|_| table.call_func_is_metamethod) {
            let call_doc = self.format_function_doc(func_idx);
            match (base_doc, call_doc) {
                (Some(td), Some(cd)) => Some(format!("{}\n\n{}", td, cd)),
                (Some(d), None) | (None, Some(d)) => Some(d),
                (None, None) => None,
            }
        } else {
            base_doc
        }
    }

    pub(crate) fn format_value_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        // Safety net: prevent stack overflow from recursive types (e.g. table
        // whose value_type contains the same table via a recursive function).
        if depth > 8 { return "?".to_string(); }
        match vt {
            ValueType::Any => "any".to_string(),
            ValueType::Nil => "nil".to_string(),
            ValueType::Boolean(Some(true)) => "true".to_string(),
            ValueType::Boolean(Some(false)) => "false".to_string(),
            ValueType::Boolean(None) => "boolean".to_string(),
            ValueType::Number => "number".to_string(),
            ValueType::NumberLiteral(val) => val.clone(),
            ValueType::String(Some(val)) => format!("\"{}\"", val),
            ValueType::String(None) => "string".to_string(),
            ValueType::Function(Some(func_idx)) => {
                let primary = self.format_function_value(*func_idx, depth, None);
                let func = self.func(*func_idx);
                if func.overloads.is_empty() || depth > 0 {
                    primary
                } else {
                    let mut lines = vec![primary];
                    for overload in &func.overloads {
                        lines.push(self.format_overload(overload));
                    }
                    lines.join("\n")
                }
            }
            ValueType::Function(None) => "function".to_string(),
            ValueType::Table(Some(table_idx)) => {
                let table = self.table(*table_idx);
                let overlay = self.ir.overlay_fields.get(table_idx);
                let has_fields = !table.fields.is_empty() || overlay.is_some_and(|o| !o.is_empty());
                // Array/map types: table has value_type and no class_name
                if table.class_name.is_none()
                    && let Some(ref val_vt) = table.value_type {
                        // Tighter limit than the outer depth > 8 guard: recursive
                        // functions (e.g. deep-copy) commonly produce tables whose
                        // value_type contains the same table, so cap early to avoid
                        // deep expansion before the general safety net kicks in.
                        if depth > 4 { return "table".to_string(); }
                        let val_str = self.format_value_type_depth(val_vt, depth + 1);
                        return match &table.key_type {
                            Some(ValueType::Number) | None if !table.is_explicit_map => {
                                if matches!(val_vt, ValueType::Union(_) | ValueType::Intersection(_)) {
                                    format!("({})[]", val_str)
                                } else {
                                    format!("{}[]", val_str)
                                }
                            }
                            Some(key_vt) => {
                                let key_str = self.format_value_type_depth(key_vt, depth + 1);
                                format!("table<{}, {}>", key_str, val_str)
                            }
                            // Defensive: explicit-map tables always have Some(key_type)
                            None => format!("{}[]", val_str),
                        };
                    }
                if let Some(ref class_name) = table.class_name {
                    let has_parents = !table.parent_classes.is_empty();
                    if (!has_fields && !has_parents) || depth > 0 {
                        return class_name.clone();
                    }
                    let indent = "  ".repeat(depth + 1);
                    let is_enum = table.enum_kind.is_enum();
                    let mut seen: HashSet<&str> = HashSet::new();
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        seen.insert(name.as_str());
                        self.format_enum_field_line(&indent, name, field_info, is_enum, depth)
                    }).collect();
                    if let Some(ov) = overlay {
                        for (name, field_info) in ov.iter() {
                            if seen.insert(name.as_str()) {
                                fields.push(self.format_enum_field_line(&indent, name, field_info, is_enum, depth));
                            }
                        }
                    }
                    // Include inherited fields from parent classes
                    for &parent_idx in &table.parent_classes {
                        let parent_table = self.table(parent_idx);
                        for (name, field_info) in &parent_table.fields {
                            if seen.insert(name.as_str()) {
                                fields.push(self.format_enum_field_line(&indent, name, field_info, is_enum, depth));
                            }
                        }
                    }
                    fields.sort();
                    if fields.is_empty() {
                        return class_name.clone();
                    }
                    return format!("{} {{\n{}\n}}", class_name, fields.join(",\n"));
                }
                if !has_fields || depth > 4 {
                    "table".to_string()
                } else if depth > 0 {
                    // Collapse sub-tables that contain methods or have many fields
                    // to keep hover readable (e.g. Auctionator.AH with 25 methods).
                    let has_methods = table.fields.values().any(|fi| {
                        matches!(self.expr(fi.expr), Expr::FunctionDef(_))
                    });
                    if (has_methods && table.fields.len() > 2) || table.fields.len() > 4 {
                        return format!("{{... {} fields}}", table.fields.len());
                    }
                    // Compact inline format for small nested anonymous tables
                    // (e.g. value_type in arrays: `{id: number, name: string}[]`)
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        let type_str = self.format_field_type(field_info, depth);
                        format!("{}: {}", name, type_str)
                    }).collect();
                    fields.sort();
                    format!("{{{}}}", fields.join(", "))
                } else {
                    let indent = "  ".repeat(depth + 1);
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        let type_str = self.format_field_type(field_info, depth);
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
                    if let Some(ov) = overlay {
                        for (name, field_info) in ov.iter() {
                            if !table.fields.contains_key(name) {
                                let type_str = self.format_field_type(field_info, depth);
                                fields.push(format!("{}{}: {}", indent, name, type_str));
                            }
                        }
                    }
                    fields.sort();
                    format!("{{\n{}\n}}", fields.join(",\n"))
                }
            }
            ValueType::Table(None) => "table".to_string(),
            ValueType::Union(types) if types.is_empty() => "never".to_string(),
            ValueType::Union(types) if types.len() == 2
                && types.iter().any(|t| matches!(t, ValueType::Nil))
                && types.iter().any(|t| !matches!(t, ValueType::Nil)) =>
            {
                let other = types.iter().find(|t| !matches!(t, ValueType::Nil)).unwrap();
                let formatted = self.format_value_type_depth(other, depth + 1);
                if matches!(other, ValueType::Function(Some(_))) {
                    format!("({})?", formatted)
                } else {
                    format!("{}?", formatted)
                }
            }
            ValueType::Union(types) => {
                const MAX_STRING_LITERALS: usize = 3;
                let string_literal_count = types.iter().filter(|t| matches!(t, ValueType::String(Some(_)))).count();
                if string_literal_count > MAX_STRING_LITERALS {
                    let mut parts: Vec<String> = Vec::new();
                    let mut shown_strings = 0;
                    for t in types {
                        if matches!(t, ValueType::String(Some(_))) {
                            if shown_strings < MAX_STRING_LITERALS {
                                parts.push(self.format_value_type_depth(t, depth + 1));
                                shown_strings += 1;
                            }
                        } else {
                            parts.push(self.format_value_type_depth(t, depth + 1));
                        }
                    }
                    let remaining = string_literal_count - MAX_STRING_LITERALS;
                    parts.push(format!("({} more)", remaining));
                    parts.join(" | ")
                } else {
                    let parts: Vec<String> = types.iter().map(|t| self.format_value_type_depth(t, depth + 1)).collect();
                    parts.join(" | ")
                }
            }
            ValueType::Intersection(types) => {
                let parts: Vec<String> = types.iter().map(|t| self.format_value_type_depth(t, depth + 1)).collect();
                parts.join(" & ")
            }
            ValueType::TypeVariable(name) => name.clone(),
            ValueType::Userdata => "userdata".to_string(),
            ValueType::Thread => "thread".to_string(),
            ValueType::OpaqueAlias(name, _) => name.clone(),
        }
    }

    /// Like `format_value_type_depth`, but substitutes class-level type
    /// variables (e.g. `T → string`) using `subs` before formatting. Used by
    /// hover on a method of a parameterized-class receiver so the displayed
    /// signature shows the concrete bound types instead of bare `T`. Falls back
    /// to the plain formatter whenever there's nothing to substitute, so it
    /// stays byte-for-byte identical to existing output in the common case.
    fn format_type_subst(
        &self,
        vt: &ValueType,
        depth: usize,
        subs: &HashMap<String, ValueType>,
    ) -> String {
        if depth > 8 {
            return "?".to_string();
        }
        if subs.is_empty() || !self.type_contains_type_variable_deep(vt) {
            return self.format_value_type_depth(vt, depth);
        }
        match vt {
            ValueType::TypeVariable(name) => match subs.get(name) {
                Some(t) => self.format_value_type_depth(t, depth),
                None => name.clone(),
            },
            ValueType::Function(Some(func_idx)) => {
                self.format_function_value(*func_idx, depth, Some(subs))
            }
            ValueType::Table(Some(table_idx)) => {
                let table = self.table(*table_idx);
                // Array/map types (no class name, has value_type): recurse into
                // the element/key types so nested `T` is substituted.
                if table.class_name.is_none()
                    && let Some(ref val_vt) = table.value_type
                {
                    if depth > 4 {
                        return "table".to_string();
                    }
                    let val_str = self.format_type_subst(val_vt, depth + 1, subs);
                    return match &table.key_type {
                        Some(ValueType::Number) | None if !table.is_explicit_map => {
                            if matches!(val_vt, ValueType::Union(_) | ValueType::Intersection(_)) {
                                format!("({})[]", val_str)
                            } else {
                                format!("{}[]", val_str)
                            }
                        }
                        Some(key_vt) => {
                            let key_str = self.format_type_subst(key_vt, depth + 1, subs);
                            format!("table<{}, {}>", key_str, val_str)
                        }
                        None => format!("{}[]", val_str),
                    };
                }
                // Named class tables collapse to their name at depth > 0, so the
                // plain formatter is sufficient for nested class references.
                self.format_value_type_depth(vt, depth)
            }
            ValueType::Union(types)
                if types.len() == 2
                    && types.iter().any(|t| matches!(t, ValueType::Nil))
                    && types.iter().any(|t| !matches!(t, ValueType::Nil)) =>
            {
                let other = types.iter().find(|t| !matches!(t, ValueType::Nil)).unwrap();
                let formatted = self.format_type_subst(other, depth + 1, subs);
                if matches!(other, ValueType::Function(Some(_))) {
                    format!("({})?", formatted)
                } else {
                    format!("{}?", formatted)
                }
            }
            ValueType::Union(types) => {
                let parts: Vec<String> = types.iter()
                    .map(|t| self.format_type_subst(t, depth + 1, subs))
                    .collect();
                parts.join(" | ")
            }
            ValueType::Intersection(types) => {
                let parts: Vec<String> = types.iter()
                    .map(|t| self.format_type_subst(t, depth + 1, subs))
                    .collect();
                parts.join(" & ")
            }
            // OpaqueAlias always displays its alias name; the inner type is
            // not shown, so no substitution is needed. Explicit arm avoids
            // the `type_contains_type_variable_deep` guard triggering for an
            // inner TypeVariable and then falling through without effect.
            ValueType::OpaqueAlias(name, _) => name.clone(),
            _ => self.format_value_type_depth(vt, depth),
        }
    }

    /// Format a `fun(...)` value, optionally substituting class type variables
    /// via `subs`. Shared implementation used by both `format_value_type_depth`
    /// (no subs) and `format_type_subst` (with subs).
    fn format_function_value(
        &self,
        func_idx: FunctionIndex,
        depth: usize,
        subs: Option<&HashMap<String, ValueType>>,
    ) -> String {
        let func = self.func(func_idx);
        let args: Vec<String> = func.args.iter().enumerate().map(|(i, &sym_idx)| {
            let name = match &self.sym(sym_idx).id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => "?".to_string(),
            };
            let optional = func.param_optional.get(i).copied().unwrap_or(false);
            let ann_has_nil = func.param_annotations.get(i)
                .is_some_and(crate::annotations::annotation_type_is_nullable);
            let suffix = if optional && !ann_has_nil { "?" } else { "" };
            let type_str = if let Some(s) = subs {
                self.param_annotation_text_subst(func, i, s)
            } else {
                self.param_annotation_text(func, i)
            }.or_else(|| {
                self.sym(sym_idx).versions.first()
                    .and_then(|v| v.resolved_type.as_ref())
                    .map(|rt| {
                        let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                        let effective_depth = if name == "self" && depth > 0 {
                            depth.max(5)
                        } else {
                            depth + 1
                        };
                        if let Some(s) = subs {
                            self.format_type_subst(&display_type, effective_depth, s)
                        } else {
                            self.format_type_depth(&display_type, effective_depth)
                        }
                    })
            });
            match type_str {
                Some(t) => format!("{}{}: {}", name, suffix, t),
                None => format!("{}{}", name, suffix),
            }
        }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let no_subs = HashMap::new();
        let effective_subs = subs.unwrap_or(&no_subs);
        let rets: Vec<String> = if func.returns_self {
            vec![self.self_return_text(func, effective_subs)]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                let formatted = if let Some(s) = subs {
                    self.format_type_subst(vt, depth + 1, s)
                } else {
                    self.format_value_type_depth(vt, depth + 1)
                };
                format_vararg_return(formatted, i, func)
            }).collect()
        } else {
            self.format_inferred_returns(func, depth + 1)
        };
        if rets.is_empty() {
            format!("fun({})", all_args.join(", "))
        } else {
            format!("fun({}): {}", all_args.join(", "), rets.join(", "))
        }
    }

    pub fn signature_help_at(&self, tree: &SyntaxTree, offset: u32) -> Option<SignatureHelpResult> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;

        // Walk up to find the enclosing FunctionCall/MethodCall node
        let call_node = token.ancestors()
            .find(|n| n.kind() == SyntaxKind::FunctionCall || n.kind() == SyntaxKind::MethodCall)?;
        let call = FunctionCall::cast(call_node)?;

        // Only trigger if cursor is within the argument list (at or after the open paren)
        let arg_list = call_node.children()
            .find(|n| n.kind() == SyntaxKind::ArgumentList)?;
        if text_size < arg_list.text_range().start() {
            return None;
        }
        let (active_parameter, total_commas) = {
            let mut commas_before = 0u32;
            let mut total = 0u32;
            for child in arg_list.children_with_tokens() {
                if child.kind() == SyntaxKind::Comma {
                    total += 1;
                    if child.text_range().start() < text_size {
                        commas_before += 1;
                    }
                }
            }
            (commas_before, total)
        };

        // Resolve the function being called
        let ident = call.identifier()?;
        let names = ident.names();
        if names.is_empty() {
            return None;
        }

        let scope_idx = self.scope_at_offset(text_size)?;

        // String literal method call: "str":method() or ("str"):method()
        // names will be just ["method"] with no preceding identifier to look up.
        let string_literal_method = if names.len() == 1
            && call_node.kind() == SyntaxKind::MethodCall
            && Self::resolve_literal_receiver_type(&call_node).is_some()
        {
            let method_name = &names[0];
            let vt = ValueType::String(None);
            let mut indices = Vec::new();
            self.ir.collect_library_table_indices(&vt, &mut indices);
            indices.first().and_then(|&table_idx| {
                let field_expr = self.get_field(table_idx, method_name)?.expr;
                let ft = self.resolve_expr_type(field_expr)?;
                match ft {
                    ValueType::Function(Some(idx)) => Some(idx),
                    _ => None,
                }
            })
        } else {
            None
        };

        let func_idx = if let Some(idx) = string_literal_method {
            idx
        } else if names.len() == 1 {
            // Simple function call: foo()
            let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
            let ver = self.sym(symbol_idx).versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            match ver {
                ValueType::Function(Some(idx)) => *idx,
                _ => return None,
            }
        } else {
            // Method/field call: obj.method() or obj:method()
            let root_sym = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
            let ver = self.sym(root_sym).versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            let mut table_idx = Self::extract_table_idx(ver)?;
            // Walk intermediate names
            for name in &names[1..names.len()-1] {
                let field_expr = self.get_field(table_idx, name)?.expr;
                let ft = self.resolve_expr_type(field_expr)?;
                table_idx = Self::extract_table_idx(&ft)?;
            }
            let method_name = &names[names.len() - 1];
            let field_expr = self.get_field(table_idx, method_name)?.expr;
            let ft = self.resolve_expr_type(field_expr)?;
            match ft {
                ValueType::Function(Some(idx)) => idx,
                _ => return None,
            }
        };

        let func = self.func(func_idx);
        let is_colon = ident.is_call_to_self();

        // Build signatures: primary + overloads
        let mut signatures = Vec::new();
        let mut param_counts: Vec<(usize, bool)> = Vec::new(); // (param_count, is_vararg)

        // Primary signature
        let primary = self.build_signature_info(func, is_colon);
        let primary_param_count = primary.params.len();
        let primary_is_vararg = func.is_vararg;
        signatures.push(primary);
        param_counts.push((primary_param_count, primary_is_vararg));

        // Overload signatures (skip return-only overloads)
        for overload in &func.overloads {
            if overload.is_return_only { continue; }
            let sig = self.build_overload_signature_info(overload);
            let param_count = sig.params.len();
            let is_vararg = overload.is_vararg;
            signatures.push(sig);
            param_counts.push((param_count, is_vararg));
        }

        // Select best-matching signature based on total argument count at the call site.
        // Use total commas (not cursor position) so we match the full call's arity.
        // .children() yields only expression nodes (not paren/comma tokens), so
        // this checks whether any argument expressions exist.
        let has_args = arg_list.children().next().is_some();
        let arg_count = if has_args { (total_commas + 1) as usize } else { 0 };
        // When no args typed yet (empty parens), default to showing the primary signature
        let active_signature = if arg_count == 0 {
            Some(0)
        } else {
            Some(Self::best_matching_signature(&param_counts, arg_count) as u32)
        };

        Some(SignatureHelpResult {
            signatures,
            active_signature,
            active_parameter,
        })
    }

    fn build_signature_info(&self, func: &Function, skip_self: bool) -> SignatureInfo {
        let args: Vec<(String, Option<String>, Option<String>)> = func.args.iter()
            .enumerate()
            .filter(|&(_, &sym_idx)| {
                if skip_self
                    && let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id {
                        return n != "self";
                    }
                true
            })
            .map(|(i, &sym_idx)| {
                let name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let optional = func.param_optional.get(i).copied().unwrap_or(false);
                let ann_has_nil = func.param_annotations.get(i)
                    .is_some_and(crate::annotations::annotation_type_is_nullable);
                let suffix = if optional && !ann_has_nil { "?" } else { "" };
                let display_name = format!("{}{}", name, suffix);
                // Prefer raw annotation text (preserves alias names) over resolved type
                let type_str = self.param_annotation_text(func, i)
                    .or_else(|| {
                        // Use version 0 only (declaration type from @param), not a
                        // later version from type-guard narrowing in the body.
                        self.sym(sym_idx).versions.first()
                            .and_then(|v| v.resolved_type.as_ref())
                            .map(|rt| {
                                let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                                self.format_type_depth(&display_type, 1)
                            })
                    });
                let desc = func.param_descriptions.get(i).cloned().flatten();
                (display_name, type_str, desc)
            })
            .collect();

        let no_subs = HashMap::new();
        let rets: Vec<String> = if func.returns_self {
            vec![self.self_return_text(func, &no_subs)]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                let formatted = self.format_value_type_depth(vt, 1);
                format_vararg_return(formatted, i, func)
            }).collect()
        } else {
            self.format_inferred_returns(func, 1)
        };

        let mut params: Vec<String> = args.iter().map(|(name, type_str, _)| {
            match type_str {
                Some(t) => format!("{}: {}", name, t),
                None => name.clone(),
            }
        }).collect();
        let mut param_docs: Vec<Option<String>> = args.iter().map(|(_, _, desc)| desc.clone()).collect();
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            params.push(vararg_str);
            param_docs.push(func.vararg_description.clone());
        }

        let label = if rets.is_empty() {
            format!("fun({})", params.join(", "))
        } else {
            format!("fun({}): {}", params.join(", "), rets.join(", "))
        };

        SignatureInfo { label, params, param_docs, doc: func.doc.clone() }
    }

    fn build_overload_signature_info(&self, overload: &ResolvedOverload) -> SignatureInfo {
        let params: Vec<String> = overload.params.iter().map(|p| {
            match &p.typ {
                Some(vt) => format!("{}: {}", p.name, self.format_value_type_depth(vt, 1)),
                None => p.name.clone(),
            }
        }).collect();

        let rets: Vec<String> = overload.returns.iter()
            .map(|vt| self.format_value_type_depth(vt, 1))
            .collect();

        let label = if rets.is_empty() {
            format!("fun({})", params.join(", "))
        } else {
            format!("fun({}): {}", params.join(", "), rets.join(", "))
        };

        let param_docs = vec![None; params.len()];
        SignatureInfo { label, params, param_docs, doc: None }
    }

    /// Pick the signature index whose parameter count best matches the number of
    /// arguments being typed. Prefers exact non-vararg match, then vararg match,
    /// then smallest count >= arg_count, then falls back to the largest count.
    fn best_matching_signature(param_counts: &[(usize, bool)], arg_count: usize) -> usize {
        if param_counts.len() <= 1 {
            return 0;
        }
        let mut best = 0usize;
        let mut best_score = u32::MAX;
        for (i, &(count, is_vararg)) in param_counts.iter().enumerate() {
            let score = if is_vararg && arg_count >= count {
                // Vararg can accept extra args, but prefer exact non-vararg matches
                1
            } else if count == arg_count {
                0 // exact match
            } else if count > arg_count {
                // Can accept all args — prefer closer counts
                (count - arg_count) as u32
            } else {
                // Too few params — heavily penalize
                1000 + (arg_count - count) as u32
            };
            if score < best_score {
                best_score = score;
                best = i;
            }
        }
        best
    }

    /// Find the version whose `def_node` range contains `token_start`.
    /// Used for redefined locals where multiple versions share the same SymbolIndex
    /// but each has a distinct `def_node` from its own `local` statement.
    /// Returns the first matching version because narrowing/merge versions copy the
    /// same `def_node` — we want the original declaration version, not a narrowed one.
    fn version_at_def_site<'a>(&self, symbol: &'a Symbol, token_start: u32) -> Option<&'a SymbolVersion> {
        symbol.versions.iter().find(|v| {
            v.def_node.start <= token_start && token_start < v.def_node.end
        })
    }

    /// Check if a symbol is a function parameter.
    pub(crate) fn is_param_symbol(&self, symbol_idx: SymbolIndex) -> bool {
        if symbol_idx.is_external() {
            return false;
        }
        self.ir.functions.iter().any(|f| f.args.contains(&symbol_idx))
    }

    /// Whether an EXT-space symbol came from the precomputed WoW API stubs
    /// (vs. being discovered by the workspace scan of user code).
    pub(crate) fn is_stub_symbol(&self, symbol_idx: SymbolIndex) -> bool {
        symbol_idx.is_external() && (symbol_idx.ext_offset()) < self.ir.ext.stub_symbols_end
    }

    fn is_param_optional(&self, symbol_idx: SymbolIndex) -> bool {
        if symbol_idx.is_external() {
            return false;
        }
        for f in &self.ir.functions {
            if let Some(pos) = f.args.iter().position(|&s| s == symbol_idx) {
                return f.param_optional.get(pos).copied().unwrap_or(false);
            }
        }
        false
    }

    /// Find the raw `AnnotationType` for a param symbol by locating its function.
    fn find_param_annotation_raw(&self, symbol_idx: SymbolIndex) -> Option<&crate::annotations::AnnotationType> {
        if symbol_idx.is_external() {
            return None;
        }
        for func in &self.ir.functions {
            if let Some(pos) = func.args.iter().position(|&s| s == symbol_idx) {
                return func.param_annotations.get(pos);
            }
        }
        None
    }

    /// If `ann` reduces to a single reference to a function-typed alias (optionally
    /// wrapped in `NonNil` or `Union(T, nil)`, and possibly chained through other
    /// aliases like `@alias A = B` where `B = fun(...)`), return the expanded
    /// `fun(...)` signature. Returns `None` for non-alias annotations, non-function
    /// aliases, or composite types like unions/intersections with multiple members.
    fn expand_alias_fun_signature(&self, ann: &crate::annotations::AnnotationType) -> Option<String> {
        let (fun_ann, _) = crate::annotations::reduce_to_fun_alias(
            ann, &self.ir.alias_fun_types, &self.ir.ext.alias_fun_types,
        )?;
        Some(crate::annotations::format_annotation_type(fun_ann))
    }

    /// Find the annotation text for a param symbol by locating its function.
    /// Returns the formatted annotation with nil members stripped (since the
    /// caller adds `?` for optional/nil-containing types).
    fn find_param_annotation_text(&self, symbol_idx: SymbolIndex) -> Option<String> {
        if symbol_idx.is_external() {
            return None;
        }
        for func in &self.ir.functions {
            if let Some(pos) = func.args.iter().position(|&s| s == symbol_idx) {
                let ann = func.param_annotations.get(pos)?;
                if matches!(ann, crate::annotations::AnnotationType::Simple(s) if s.is_empty()) {
                    return None;
                }
                if self.annotation_has_unresolvable(ann, &func.generics) {
                    return None;
                }
                // Strip nil from union annotations (added by `?` suffix syntax)
                return Some(Self::format_annotation_stripping_nil(ann));
            }
        }
        None
    }

    /// Format an annotation type, removing nil from union members.
    fn format_annotation_stripping_nil(ann: &crate::annotations::AnnotationType) -> String {
        use crate::annotations::AnnotationType;
        if let AnnotationType::Union(parts) = ann {
            let non_nil: Vec<_> = parts.iter()
                .filter(|p| !matches!(p, AnnotationType::Simple(s) if s == "nil"))
                .collect();
            if non_nil.len() < parts.len() {
                // Had nil — format without it
                return non_nil.iter()
                    .map(|p| crate::annotations::format_annotation_type(p))
                    .collect::<Vec<_>>()
                    .join(" | ");
            }
        }
        crate::annotations::format_annotation_type(ann)
    }

    /// Get the formatted annotation text for a function parameter, if it has
    /// a non-empty annotation. This preserves alias names like `ThemeColorKey`
    /// instead of expanding them to their underlying union.
    /// Skips annotations containing unresolvable names (e.g. generic type
    /// variables from a parent scope like `T`), so the resolved concrete type
    /// is shown instead.
    fn param_annotation_text(&self, func: &Function, param_idx: usize) -> Option<String> {
        let ann = func.param_annotations.get(param_idx)?;
        match ann {
            crate::annotations::AnnotationType::Simple(s) if s.is_empty() => None,
            _ => {
                if self.annotation_has_unresolvable(ann, &func.generics) {
                    return None;
                }
                // For named params, VarArgs(...) doesn't make sense — unwrap to
                // just the inner type (e.g. `...any?` → `any?`).
                let effective = match ann {
                    crate::annotations::AnnotationType::VarArgs(inner) => inner.as_ref(),
                    other => other,
                };
                let formatted = crate::annotations::format_annotation_type(effective);
                // If the formatted result is empty or just "?" (from VarArgs
                // wrapping an empty base type in old precomputed stubs), normalize
                // to "any?" since the annotation intent is "optional any value".
                if formatted.is_empty() {
                    return None;
                }
                if formatted == "?" {
                    return Some("any?".to_string());
                }
                Some(formatted)
            }
        }
    }

    /// Like `param_annotation_text` but substitutes class type variables (e.g.
    /// `T → string`) from `subs` into the raw annotation before formatting. This
    /// is what lets hover show concrete bound types for a method called on a
    /// parameterized-class receiver — the raw `@param func fun(value: T)` would
    /// otherwise short-circuit before any resolved-type substitution applied.
    fn param_annotation_text_subst(
        &self,
        func: &Function,
        param_idx: usize,
        subs: &HashMap<String, ValueType>,
    ) -> Option<String> {
        if subs.is_empty() {
            return self.param_annotation_text(func, param_idx);
        }
        let ann = func.param_annotations.get(param_idx)?;
        let ann = self.substitute_annotation_type_vars(ann, subs);
        match &ann {
            crate::annotations::AnnotationType::Simple(s) if s.is_empty() => None,
            _ => {
                if self.annotation_has_unresolvable(&ann, &func.generics) {
                    return None;
                }
                let effective = match &ann {
                    crate::annotations::AnnotationType::VarArgs(inner) => inner.as_ref(),
                    other => other,
                };
                let formatted = crate::annotations::format_annotation_type(effective);
                if formatted.is_empty() {
                    return None;
                }
                if formatted == "?" {
                    return Some("any?".to_string());
                }
                Some(formatted)
            }
        }
    }

    /// Format the `@return self` text for a method, expanding to `self<X>` when
    /// the method has `@return self<X>` re-parameterization args. When `subs`
    /// contains bindings for type variables in the args (e.g. `T → string?`),
    /// the concrete types are shown instead of the raw annotation variables.
    fn self_return_text(&self, func: &Function, subs: &HashMap<String, ValueType>) -> String {
        match &func.returns_self_type_args {
            Some(args) if !args.is_empty() => {
                let inner = args.iter()
                    .map(|arg| self.format_self_return_arg(arg, subs))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("self<{inner}>")
            }
            _ => "self".to_string(),
        }
    }

    /// Format a single `@return self<X>` type argument, substituting type
    /// variables from `subs`. Handles `Simple("T")` (bare type variables),
    /// `NonNil(Simple("T"))` (stripping nil from the substituted type), and
    /// `Union(members)` (recursively formatting each member with dedup).
    /// More complex annotation types containing type variables
    /// (e.g. `Parameterized("Foo", [Simple("T")])`) fall through to raw
    /// formatting without substitution.
    fn format_self_return_arg(&self, arg: &crate::annotations::AnnotationType, subs: &HashMap<String, ValueType>) -> String {
        use crate::annotations::AnnotationType;
        match arg {
            AnnotationType::Simple(name) => {
                if let Some(vt) = subs.get(name.as_str()) {
                    self.format_type_depth(vt, 1)
                } else {
                    crate::annotations::format_annotation_type(arg)
                }
            }
            AnnotationType::NonNil(inner) => {
                if let AnnotationType::Simple(name) = inner.as_ref()
                    && let Some(vt) = subs.get(name.as_str())
                {
                    return self.format_type_depth(&vt.strip_nil(), 1);
                }
                crate::annotations::format_annotation_type(arg)
            }
            AnnotationType::Union(members) => {
                let mut parts: Vec<String> = Vec::new();
                for member in members {
                    let formatted = self.format_self_return_arg(member, subs);
                    if !parts.contains(&formatted) {
                        parts.push(formatted);
                    }
                }
                parts.join(" | ")
            }
            _ => crate::annotations::format_annotation_type(arg),
        }
    }

    /// Format the raw `@return` annotation at `index` for a parameterized class
    /// type (e.g. `Schema<T>` / `Schema<string?>`), applying any class type-var
    /// substitution from the receiver. The resolved `return_annotations` drop
    /// class type args (they're tracked out-of-band), so a return like
    /// `Schema<string?>` would otherwise display as the bare `Schema`; the raw
    /// annotation is the only place the `<...>` survives. Returns None when
    /// there is no raw annotation, when it is not a parameterized class type, or
    /// when it references an unresolvable type — the caller then falls back to
    /// the resolved return type formatting (preserving all other shapes such as
    /// aliases, `fun()`, plain classes, and primitives).
    fn return_annotation_text_subst(
        &self,
        func: &Function,
        index: usize,
        subs: &HashMap<String, ValueType>,
    ) -> Option<String> {
        let raw = func.return_annotations_raw.get(index)?;
        let effective = match raw {
            crate::annotations::AnnotationType::VarArgs(inner) => inner.as_ref(),
            other => other,
        };
        // Only override the resolved formatting for parameterized class types,
        // whose type args the resolved return type discards. All other shapes
        // keep their existing resolved formatting.
        if !matches!(effective, crate::annotations::AnnotationType::Parameterized(..)) {
            return None;
        }
        // Gate on the *raw* annotation's type references, treating the
        // substitution keys (the class type vars being bound) as resolvable.
        // We can't gate on the post-substitution annotation because that
        // formats concrete types back into `Simple` leaves (e.g. `string?`),
        // which aren't valid type names and would be wrongly rejected.
        let mut gen_ctx: Vec<(String, Option<ValueType>)> = func.generics.clone();
        gen_ctx.extend(subs.keys().map(|k| (k.clone(), None)));
        if self.annotation_has_unresolvable(effective, &gen_ctx) {
            return None;
        }
        let ann = self.substitute_annotation_type_vars(effective, subs);
        let formatted = crate::annotations::format_annotation_type(&ann);
        if formatted.is_empty() || formatted == "?" {
            return None;
        }
        Some(formatted)
    }

    /// Recursively replace `Simple(name)` annotation leaves whose name is a key in
    /// `subs` with the formatted concrete type. Used by `param_annotation_text_subst`
    /// to render bound class type variables.
    fn substitute_annotation_type_vars(
        &self,
        ann: &crate::annotations::AnnotationType,
        subs: &HashMap<String, ValueType>,
    ) -> crate::annotations::AnnotationType {
        use crate::annotations::{AnnotationType as AT, ParamInfo, TuplePosition};
        match ann {
            AT::Simple(s) => match subs.get(s) {
                Some(vt) => AT::Simple(self.format_value_type_depth(vt, 1)),
                None => ann.clone(),
            },
            AT::Union(parts) => AT::Union(parts.iter().map(|p| self.substitute_annotation_type_vars(p, subs)).collect()),
            AT::Intersection(parts) => AT::Intersection(parts.iter().map(|p| self.substitute_annotation_type_vars(p, subs)).collect()),
            AT::Array(inner) => AT::Array(Box::new(self.substitute_annotation_type_vars(inner, subs))),
            AT::Parameterized(base, args) => AT::Parameterized(base.clone(), args.iter().map(|a| self.substitute_annotation_type_vars(a, subs)).collect()),
            AT::Backtick(inner) => AT::Backtick(Box::new(self.substitute_annotation_type_vars(inner, subs))),
            AT::NonNil(inner) => AT::NonNil(Box::new(self.substitute_annotation_type_vars(inner, subs))),
            AT::VarArgs(inner) => AT::VarArgs(Box::new(self.substitute_annotation_type_vars(inner, subs))),
            AT::Fun(params, returns, is_vararg) => AT::Fun(
                params.iter().map(|p| ParamInfo {
                    name: p.name.clone(),
                    typ: self.substitute_annotation_type_vars(&p.typ, subs),
                    optional: p.optional,
                    description: p.description.clone(),
                }).collect(),
                returns.iter().map(|r| self.substitute_annotation_type_vars(r, subs)).collect(),
                *is_vararg,
            ),
            AT::TableLiteral(fields) => AT::TableLiteral(
                fields.iter().map(|(n, ft)| (n.clone(), self.substitute_annotation_type_vars(ft, subs))).collect(),
            ),
            AT::IndexedAccess(base, key) => {
                let substituted_base = subs.get(base)
                    .map(|vt| self.format_value_type_depth(vt, 1))
                    .unwrap_or_else(|| base.clone());
                AT::IndexedAccess(
                    substituted_base,
                    Box::new(self.substitute_annotation_type_vars(key, subs)),
                )
            }
            AT::Tuple(positions, desc) => AT::Tuple(
                positions.iter().map(|p| TuplePosition {
                    typ: self.substitute_annotation_type_vars(&p.typ, subs),
                    name: p.name.clone(),
                }).collect(),
                desc.clone(),
            ),
        }
    }

    /// Check if an annotation type contains any Simple names that can't be
    /// resolved to a known type, class, or alias. This detects stale generic
    /// type variables (e.g. `T` from a parent scope) that were substituted
    /// during resolution but remain in the raw annotation.
    fn annotation_has_unresolvable(
        &self, ann: &crate::annotations::AnnotationType,
        generics: &[(String, Option<ValueType>)],
    ) -> bool {
        use crate::annotations::AnnotationType;
        match ann {
            AnnotationType::Simple(s) => {
                match s.as_str() {
                    "" | "nil" | "boolean" | "bool" | "true" | "false"
                    | "number" | "integer" | "string" | "table"
                    | "function" | "fun" | "any" | "self" => false,
                    _ if s.starts_with('"') || s.starts_with('\'') => false,
                    _ if s.starts_with("fun(") => false,
                    _ if generics.iter().any(|(g, _)| g == s) => false,
                    _ if self.ir.classes.contains_key(s) => false,
                    _ if self.ir.aliases.contains_key(s) => false,
                    _ if self.ir.ext.classes.contains_key(s.as_str()) => false,
                    _ if self.ir.ext.aliases.contains_key(s.as_str()) => false,
                    _ => true,
                }
            }
            AnnotationType::Union(parts) => parts.iter().any(|p| self.annotation_has_unresolvable(p, generics)),
            AnnotationType::Intersection(parts) => parts.iter().any(|p| self.annotation_has_unresolvable(p, generics)),
            AnnotationType::Array(inner) => self.annotation_has_unresolvable(inner, generics),
            AnnotationType::Parameterized(base, args) => {
                self.annotation_has_unresolvable(&AnnotationType::Simple(base.clone()), generics)
                    || args.iter().any(|a| self.annotation_has_unresolvable(a, generics))
            }
            AnnotationType::Backtick(inner) => self.annotation_has_unresolvable(inner, generics),
            AnnotationType::NonNil(inner) => self.annotation_has_unresolvable(inner, generics),
            AnnotationType::Fun(params, returns, _) => {
                params.iter().any(|p| self.annotation_has_unresolvable(&p.typ, generics))
                    || returns.iter().any(|r| self.annotation_has_unresolvable(r, generics))
            }
            AnnotationType::TableLiteral(fields) => {
                fields.iter().any(|(_, ft)| self.annotation_has_unresolvable(ft, generics))
            }
            AnnotationType::VarArgs(inner) => self.annotation_has_unresolvable(inner, generics),
            AnnotationType::IndexedAccess(base, key) => {
                (!generics.iter().any(|(g, _)| g == base)
                    && self.annotation_has_unresolvable(&AnnotationType::Simple(base.clone()), generics))
                || self.annotation_has_unresolvable(key, generics)
            }
            AnnotationType::Tuple(positions, _) => positions.iter().any(|p| self.annotation_has_unresolvable(&p.typ, generics)),
        }
    }


    /// Format a function in declaration style for hover: `function name(params)\n  -> ret`
    /// If `skip_self` is true, the first "self" parameter is omitted (colon-style methods).
    /// Format inferred return types (no `@return` annotation case). Returns
    /// empty when there are no value-returning return statements (void).
    /// When there are inferred returns and the function has an implicit nil
    /// return, nil is unioned into each resolved position.
    pub(crate) fn format_inferred_returns(&self, func: &Function, depth: usize) -> Vec<String> {
        // When synthesized return-only overloads exist, derive the summary type
        // per position by unioning across the overloads. This is more accurate
        // than reading FunctionRet symbols which may hold stale placeholder types
        // (e.g. `Any` from before the overload refinement fixpoint settles).
        let return_only: Vec<&ResolvedOverload> = func.overloads.iter()
            .filter(|o| o.is_return_only).collect();
        if !return_only.is_empty() {
            let max_arity = return_only.iter().map(|o| o.returns.len()).max().unwrap_or(0);
            let mut result = Vec::new();
            for pos in 0..max_arity {
                let mut types: Vec<ValueType> = Vec::new();
                for o in &return_only {
                    let vt = o.return_type_at(pos);
                    if !types.contains(&vt) {
                        types.push(vt);
                    }
                }
                let merged = ValueType::make_union(types);
                result.push(self.format_type_depth(&merged, depth));
            }
            return result;
        }
        let inferred = dedup_return_types(&self.ir, &func.rets);
        let implicit_nil = func.implicit_nil_return;
        if inferred.is_empty() {
            return vec![];
        }
        inferred.iter().map(|rt| match rt.as_ref() {
            Some(rt) => {
                let display = if implicit_nil && !rt.contains_nil() && !matches!(rt, ValueType::Any) {
                    ValueType::make_union(vec![rt.clone(), ValueType::Nil])
                } else {
                    rt.clone()
                };
                self.format_type_depth(&display, depth)
            }
            // Unresolved position: leave as `?` — we don't know the type,
            // and artificially narrowing to `nil` would be misleading.
            None => "?".to_string(),
        }).collect()
    }

    /// Like `format_inferred_returns` but collapses anonymous shape tables for inlay hints.
    fn format_inferred_returns_for_hint(&self, func: &Function) -> Vec<String> {
        // Same overload-based summary as format_inferred_returns.
        let return_only: Vec<&ResolvedOverload> = func.overloads.iter()
            .filter(|o| o.is_return_only).collect();
        if !return_only.is_empty() {
            let max_arity = return_only.iter().map(|o| o.returns.len()).max().unwrap_or(0);
            let mut result = Vec::new();
            for pos in 0..max_arity {
                let mut types: Vec<ValueType> = Vec::new();
                for o in &return_only {
                    let vt = o.return_type_at(pos);
                    if !types.contains(&vt) {
                        types.push(vt);
                    }
                }
                let merged = ValueType::make_union(types);
                result.push(self.format_type_for_hint(&merged));
            }
            return result;
        }
        let inferred = dedup_return_types(&self.ir, &func.rets);
        let implicit_nil = func.implicit_nil_return;
        if inferred.is_empty() {
            return vec![];
        }
        inferred.iter().map(|rt| match rt.as_ref() {
            Some(rt) => {
                let display = if implicit_nil && !rt.contains_nil() && !matches!(rt, ValueType::Any) {
                    ValueType::make_union(vec![rt.clone(), ValueType::Nil])
                } else {
                    rt.clone()
                };
                self.format_type_for_hint(&display)
            }
            None => "?".to_string(),
        }).collect()
    }

    fn format_function_decl(
        &self,
        func_idx: FunctionIndex,
        name: &str,
        skip_self: bool,
        subs: Option<&HashMap<String, ValueType>>,
    ) -> String {
        let empty = HashMap::new();
        let subs = subs.unwrap_or(&empty);
        let func = self.func(func_idx);
        let args: Vec<String> = func.args.iter().enumerate()
            .filter(|&(i, &sym_idx)| {
                if skip_self && i == 0
                    && let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id {
                        return n != "self";
                    }
                true
            })
            .map(|(i, &sym_idx)| {
                let param_name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let optional = func.param_optional.get(i).copied().unwrap_or(false);
                let ann_has_nil = func.param_annotations.get(i)
                    .is_some_and(crate::annotations::annotation_type_is_nullable);
                let suffix = if optional && !ann_has_nil { "?" } else { "" };
                // Prefer raw annotation text (preserves alias names) over resolved type
                let type_str = self.param_annotation_text_subst(func, i, subs)
                    .or_else(|| {
                        // Use version 0 only (declaration type from @param), not a
                        // later version from type-guard narrowing in the body.
                        self.sym(sym_idx).versions.first()
                            .and_then(|v| v.resolved_type.as_ref())
                            .map(|rt| {
                                let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                                self.format_type_subst(&display_type, 1, subs)
                            })
                    });
                match type_str {
                    Some(t) => format!("{}{}: {}", param_name, suffix, t),
                    None => format!("{}{}", param_name, suffix),
                }
            }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let rets: Vec<String> = if func.returns_self {
            vec![self.self_return_text(func, subs)]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                // Prefer the raw annotation (preserves `Parameterized` class
                // type args like `Schema<T>`) with the receiver's class type-var
                // substitution applied, so a method on a `Stream<string>` shows
                // `-> Schema<string>` instead of the bare resolved `Schema`.
                let formatted = self.return_annotation_text_subst(func, i, subs)
                    .unwrap_or_else(|| self.format_type_subst(vt, 1, subs));
                let with_vararg = format_vararg_return(formatted, i, func);
                // Prepend `label: ` if a label exists for this position
                match func.return_labels.get(i).and_then(|n| n.as_ref()) {
                    Some(label) => format!("{}: {}", label, with_vararg),
                    None => with_vararg,
                }
            }).collect()
        } else {
            self.format_inferred_returns(func, 1)
        };
        let args_joined = all_args.join(", ");
        let single_line = format!("function {}({})", name, args_joined);
        let mut result = if single_line.len() > 80 && all_args.len() > 1 {
            format!("function {}(\n  {}\n)", name, all_args.join(",\n  "))
        } else {
            single_line
        };
        if !rets.is_empty() {
            result.push_str(&format!("\n  -> {}", rets.join(", ")));
        }
        // Partition overloads: return-only overloads render as a "cases:" table
        // below the primary signature (they don't vary the param list, so stacking
        // them as separate `function name(...)` blocks would be visual noise).
        // Regular overloads continue to stack above as before.
        if !func.overloads.is_empty() {
            for overload in &func.overloads {
                if overload.is_return_only { continue; }
                let ov_args: Vec<String> = overload.params.iter()
                    .filter(|p| !(skip_self && p.name == "self"))
                    .map(|p| {
                        let opt = if p.optional { "?" } else { "" };
                        match &p.typ {
                            Some(vt) => format!("{}{}: {}", p.name, opt, self.format_type_subst(vt, 1, subs)),
                            None => format!("{}{}", p.name, opt),
                        }
                    }).collect();
                let ov_rets: Vec<String> = if let Some(ref self_args) = overload.returns_self_type_args {
                    if self_args.is_empty() {
                        vec!["self".to_string()]
                    } else {
                        let inner = self_args.iter()
                            .map(|arg| self.format_self_return_arg(arg, subs))
                            .collect::<Vec<_>>()
                            .join(", ");
                        vec![format!("self<{inner}>")]
                    }
                } else {
                    overload.returns.iter()
                        .map(|vt| self.format_type_subst(vt, 1, subs))
                        .collect()
                };
                let ov_args_joined = ov_args.join(", ");
                let ov_single = format!("\nfunction {}({})", name, ov_args_joined);
                let mut ov_line = if ov_single.len() > 81 && ov_args.len() > 1 {
                    format!("\nfunction {}(\n  {}\n)", name, ov_args.join(",\n  "))
                } else {
                    ov_single
                };
                if !ov_rets.is_empty() {
                    ov_line.push_str(&format!("\n  -> {}", ov_rets.join(", ")));
                }
                result.push_str(&ov_line);
            }

            // Return-only overloads → cases table. Synthesized cases (from
            // `synthesize_correlated_return_overloads`) have no `@return` source
            // and no descriptions — mark them as inferred so hover doesn't imply
            // the author wrote them.
            let return_only: Vec<&ResolvedOverload> = func.overloads.iter()
                .filter(|o| o.is_return_only).collect();
            if !return_only.is_empty() {
                let mut rows: Vec<(String, Option<String>)> = return_only.iter().map(|ovl| {
                    let parts: Vec<String> = ovl.returns.iter()
                        .map(|vt| self.format_type_subst(vt, 1, subs))
                        .collect();
                    (format!("({})", parts.join(", ")), ovl.description.clone())
                }).collect();
                // Deduplicate identical formatted tuples (can arise when
                // different annotation representations resolve to the same type).
                rows.dedup_by(|a, b| a.0 == b.0);
                let widest = rows.iter().map(|(t, _)| t.len()).max().unwrap_or(0);
                let synthesized = func.return_annotations.is_empty();
                result.push_str(if synthesized { "\n  cases (inferred):" } else { "\n  cases:" });
                for (tuple_str, desc) in rows {
                    match desc {
                        Some(d) => result.push_str(&format!("\n    {:<width$}  -- {}", tuple_str, d, width = widest)),
                        None => result.push_str(&format!("\n    {}", tuple_str)),
                    }
                }
            }
        }
        result
    }

    // ── Inlay Hints ────────────────────────────────────────────────────────────

    pub fn inlay_hints(
        &self,
        tree: &SyntaxTree,
        range: (u32, u32),
        config: InlayHintConfig,
    ) -> Vec<InlayHintData> {
        let mut hints = Vec::new();
        let source = tree.source();

        if config.parameter_names {
            self.collect_param_name_hints(source, range, &mut hints);
        }

        if config.chained_return_types {
            self.collect_chained_return_hints(range, &mut hints);
        }

        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            let node_range = node.text_range();
            let node_start = u32::from(node_range.start());
            let node_end = u32::from(node_range.end());
            if node_end < range.0 || node_start > range.1 {
                continue;
            }

            match node.kind() {
                SyntaxKind::LocalAssignStatement if config.variable_types => {
                    self.collect_local_type_hints(tree, node, &mut hints);
                }
                SyntaxKind::FunctionDefinition if config.function_return_types || config.parameter_types => {
                    if config.function_return_types {
                        self.collect_function_return_hints(node, &mut hints);
                    }
                    if config.parameter_types {
                        self.collect_param_type_hints(node, &mut hints);
                    }
                }
                SyntaxKind::ForInLoop if config.for_variable_types => {
                    self.collect_forin_type_hints(tree, node, &mut hints);
                }
                _ => {}
            }
        }

        hints.sort_by_key(|h| h.position);
        hints
    }

    fn collect_param_name_hints(
        &self,
        source: &str,
        range: (u32, u32),
        hints: &mut Vec<InlayHintData>,
    ) {
        let mut seen_call_ranges: HashSet<(u32, u32)> = HashSet::new();
        for (&expr_id, cr) in &self.ir.call_resolutions {
            let call_range = match self.ir.expr(expr_id) {
                Expr::FunctionCall { call_range, .. } => *call_range,
                _ => continue,
            };
            // Multi-return expansion re-lowers the same source call multiple times,
            // creating duplicate call_resolutions entries with identical call_range.
            // Skip duplicates to avoid emitting repeated parameter hints.
            if !seen_call_ranges.insert(call_range) {
                continue;
            }
            if call_range.1 < range.0 || call_range.0 > range.1 {
                continue;
            }

            for arg in &cr.expected_args {
                let name = &arg.param_name;
                if name.is_empty() || name == "?" || name == "self" || name == "..." {
                    continue;
                }

                let arg_start = arg.start as usize;
                let arg_end = arg.end as usize;
                if arg_start >= source.len() || arg_end > source.len() {
                    continue;
                }
                let arg_text = source[arg_start..arg_end].trim();

                if arg_text == name {
                    continue;
                }

                hints.push(InlayHintData {
                    position: arg.start,
                    label: format!("{}:", name),
                    kind: InlayHintKindTag::Parameter,
                    padding_left: false,
                    padding_right: true,
                });
            }
        }
    }

    fn collect_local_type_hints(
        &self,
        tree: &SyntaxTree,
        node: SyntaxNode<'_>,
        hints: &mut Vec<InlayHintData>,
    ) {
        let Some(assign) = LocalAssign::cast(node) else { return };
        let Some(name_list) = assign.name_list() else { return };

        let rhs_exprs: Vec<Expression<'_>> = assign.expression_list()
            .map(|el| el.expressions())
            .unwrap_or_default();

        for (i, token) in name_list.name_tokens().iter().enumerate() {
            if token.text() == "_" {
                continue;
            }
            if rhs_exprs.get(i).is_some_and(|e| matches!(e, Expression::Function(_))) {
                continue;
            }

            let token_start = u32::from(token.text_range().start());
            let token_end = u32::from(token.text_range().end());

            let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, token_start) else { continue };

            if self.ir.symbol_type_annotations.contains_key(&symbol_idx) {
                continue;
            }

            let Some(resolved) = self.sym(symbol_idx).versions.first()
                .and_then(|v| v.resolved_type.as_ref())
            else { continue };

            if matches!(resolved, ValueType::Any | ValueType::Nil | ValueType::Function(Some(_))) {
                continue;
            }

            // For tables mutated via bracket assignment, show the constructor's
            // initial element type rather than the post-mutation type.
            let formatted = self.initial_array_display(resolved)
                .unwrap_or_else(|| self.format_type_for_hint(resolved));
            if formatted == "?" { continue; }

            // Append bound generic type args (e.g. Schema → Schema<string>)
            let type_args = self.get_symbol_type_args(symbol_idx, token_start);
            let formatted = self.append_type_args_to_class(&formatted, resolved, &type_args);

            hints.push(InlayHintData {
                position: token_end,
                label: format!(": {}", formatted),
                kind: InlayHintKindTag::Type,
                padding_left: true,
                padding_right: false,
            });
        }
    }

    fn collect_function_return_hints(
        &self,
        node: SyntaxNode<'_>,
        hints: &mut Vec<InlayHintData>,
    ) {
        let Some(func_def) = FunctionDefinition::cast(node) else { return };
        let node_start = u32::from(func_def.syntax().text_range().start());

        let func_idx = self.ir.functions.iter().enumerate()
            .find(|(_, f)| f.def_node.start == node_start)
            .map(|(i, _)| FunctionIndex(i));
        let Some(func_idx) = func_idx else { return };
        let func = self.func(func_idx);

        if !func.return_annotations.is_empty() || func.returns_self || func.explicit_void_return {
            return;
        }

        let rets = self.format_inferred_returns_for_hint(func);
        if rets.is_empty() { return; }

        let Some(pl) = func_def.params() else { return };
        let hint_pos = u32::from(pl.syntax().text_range().end());

        hints.push(InlayHintData {
            position: hint_pos,
            label: format!("-> {}", rets.join(", ")),
            kind: InlayHintKindTag::Type,
            padding_left: true,
            padding_right: false,
        });
    }

    fn collect_param_type_hints(
        &self,
        node: SyntaxNode<'_>,
        hints: &mut Vec<InlayHintData>,
    ) {
        let Some(func_def) = FunctionDefinition::cast(node) else { return };
        let node_start = u32::from(func_def.syntax().text_range().start());

        let func_idx = self.ir.functions.iter().enumerate()
            .find(|(_, f)| f.def_node.start == node_start)
            .map(|(i, _)| FunctionIndex(i));
        let Some(func_idx) = func_idx else { return };
        let func = self.func(func_idx);
        let sentinel = crate::annotations::AnnotationType::Simple(String::new());

        let Some(pl) = func_def.params() else { return };
        let src_params: Vec<_> = pl.syntax().children_with_tokens()
            .filter_map(|t| match t {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Parameter => Some(t),
                _ => None,
            })
            .collect();

        let self_injected = func.args.len() == src_params.len() + 1
            && matches!(&self.sym(func.args[0]).id,
                SymbolIdentifier::Name(n) if n == "self");
        let arg_offset = if self_injected { 1 } else { 0 };

        for (i, token) in src_params.iter().enumerate() {
            if token.text() == "self" { continue; }

            let arg_i = i + arg_offset;
            if arg_i >= func.args.len() { break; }

            let annotated = func.param_annotations.get(arg_i)
                .is_some_and(|a| a != &sentinel);
            if annotated { continue; }

            let sym_idx = func.args[arg_i];
            if sym_idx.is_external() { continue; }

            let Some(resolved) = self.sym(sym_idx).versions.first()
                .and_then(|v| v.resolved_type.as_ref())
            else { continue };

            if matches!(resolved, ValueType::Any | ValueType::Nil) { continue; }

            let formatted = self.format_type_for_hint(resolved);

            let token_start = u32::from(token.text_range().start());
            let type_args = self.get_symbol_type_args(sym_idx, token_start);
            let formatted = self.append_type_args_to_class(&formatted, resolved, &type_args);

            let token_end = u32::from(token.text_range().end());
            hints.push(InlayHintData {
                position: token_end,
                label: format!(": {}", formatted),
                kind: InlayHintKindTag::Type,
                padding_left: false,
                padding_right: false,
            });
        }

        // Varargs parameter hint (suppressed when the user wrote @param ...)
        if func.is_vararg
            && !self.vararg_user_annotated_fns.contains(&func_idx)
            && let Some(ref ann) = func.vararg_annotation
        {
            let vararg_token = pl.syntax().children_with_tokens()
                .find_map(|t| match t {
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::ParameterVarArgs => Some(t),
                    _ => None,
                });
            if let Some(token) = vararg_token {
                let type_text = crate::annotations::format_annotation_type(ann);
                let token_end = u32::from(token.text_range().end());
                hints.push(InlayHintData {
                    position: token_end,
                    label: format!(": {}", type_text),
                    kind: InlayHintKindTag::Type,
                    padding_left: false,
                    padding_right: false,
                });
            }
        }
    }

    fn collect_forin_type_hints(
        &self,
        tree: &SyntaxTree,
        node: SyntaxNode<'_>,
        hints: &mut Vec<InlayHintData>,
    ) {
        let Some(for_in) = ForInLoop::cast(node) else { return };
        let Some(name_list) = for_in.name_list() else { return };

        for token in name_list.name_tokens() {
            let token_start = u32::from(token.text_range().start());
            let token_end = u32::from(token.text_range().end());

            let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, token_start) else { continue };

            if self.ir.symbol_type_annotations.contains_key(&symbol_idx) {
                continue;
            }

            let Some(resolved) = self.sym(symbol_idx).versions.first()
                .and_then(|v| v.resolved_type.as_ref())
            else { continue };

            if matches!(resolved, ValueType::Any) { continue; }

            let formatted = self.format_type_for_hint(resolved);
            if formatted == "?" { continue; }

            let type_args = self.get_symbol_type_args(symbol_idx, token_start);
            let formatted = self.append_type_args_to_class(&formatted, resolved, &type_args);

            hints.push(InlayHintData {
                position: token_end,
                label: format!(": {}", formatted),
                kind: InlayHintKindTag::Type,
                padding_left: true,
                padding_right: false,
            });
        }
    }

    fn collect_chained_return_hints(
        &self,
        range: (u32, u32),
        hints: &mut Vec<InlayHintData>,
    ) {
        // Build a set of expr IDs that are used as the `table` of a FieldAccess.
        // These are the "chained" call results — their return type feeds into the next access.
        let mut chained_exprs: HashSet<ExprId> = HashSet::new();
        for expr in &self.ir.exprs {
            if let Expr::FieldAccess { table, .. } = expr {
                chained_exprs.insert(*table);
            }
        }

        for &expr_id in self.ir.call_resolutions.keys() {
            // Only emit for calls whose result is used as receiver of further access
            if !chained_exprs.contains(&expr_id) {
                continue;
            }

            let call_range = match self.ir.expr(expr_id) {
                Expr::FunctionCall { call_range, .. } => *call_range,
                _ => continue,
            };

            // Range filter
            if call_range.1 < range.0 || call_range.0 > range.1 {
                continue;
            }

            let Some(resolved) = self.resolve_expr_type(expr_id) else { continue };

            if matches!(resolved, ValueType::Any | ValueType::Nil) {
                continue;
            }

            let formatted = self.format_type_for_hint(&resolved);
            if formatted == "?" {
                continue;
            }

            let type_args = self.get_type_args_for_expr(expr_id);
            let formatted = self.append_type_args_to_class(&formatted, &resolved, &type_args);

            hints.push(InlayHintData {
                position: call_range.1,
                label: format!(": {}", formatted),
                kind: InlayHintKindTag::Type,
                padding_left: false,
                padding_right: false,
            });
        }
    }

    fn format_overload(&self, overload: &ResolvedOverload) -> String {
        let args: Vec<String> = overload.params.iter().map(|p| {
            let opt = if p.optional { "?" } else { "" };
            match &p.typ {
                Some(vt) => format!("{}{}: {}", p.name, opt, self.format_value_type_depth(vt, 1)),
                None => format!("{}{}", p.name, opt),
            }
        }).collect();
        let rets: Vec<String> = overload.returns.iter()
            .map(|vt| self.format_value_type_depth(vt, 1))
            .collect();
        if rets.is_empty() {
            format!("fun({})", args.join(", "))
        } else {
            format!("fun({}): {}", args.join(", "), rets.join(", "))
        }
    }

    // ── Call hierarchy ────────────────────────────────────────────────────────

    /// Resolve the function at `offset` for call hierarchy. Returns the function
    /// index and the display name (with class prefix for methods).
    pub fn call_hierarchy_item_at(
        &self,
        tree: &SyntaxTree,
        offset: u32,
    ) -> Option<(FunctionIndex, String)> {
        // Try symbol-based resolution first (cursor on a function name).
        if let Some((sym_idx, name, _)) = self.find_symbol_at(tree, offset)
            && let Some(func_idx) = self.resolve_symbol_to_function(sym_idx)
        {
            let display = self.call_hierarchy_display_name(func_idx, &name);
            return Some((func_idx, display));
        }
        // Try field-based resolution (cursor on a method name like `Foo:Bar`).
        if let Some((table_idx, field_name, expr_id, access_kind)) = self.resolve_field_chain_at(tree, offset)
        {
            let field_type = self.resolve_expr_type(expr_id);
            if let Some(ValueType::Function(Some(func_idx))) = field_type {
                let sep = match access_kind {
                    FieldAccessKind::Colon => ":",
                    FieldAccessKind::Dot => ".",
                };
                let class = self.table(table_idx).class_name.as_deref().unwrap_or("?");
                let display = format!("{}{}{}", class, sep, field_name);
                return Some((func_idx, display));
            }
        }
        None
    }

    fn resolve_symbol_to_function(&self, sym_idx: SymbolIndex) -> Option<FunctionIndex> {
        let sym = self.sym(sym_idx);
        for ver in &sym.versions {
            if let Some(ValueType::Function(Some(idx))) = &ver.resolved_type {
                return Some(*idx);
            }
            if let Some(src) = ver.type_source
                && let Expr::FunctionDef(idx) = self.expr(src) {
                    return Some(*idx);
            }
        }
        None
    }

    pub fn call_hierarchy_display_name(&self, func_idx: FunctionIndex, base_name: &str) -> String {
        if let Some(class_name) = self.function_owner_class.get(&func_idx) {
            let func = self.func(func_idx);
            let has_self = func.args.first().is_some_and(|&s| {
                matches!(&self.sym(s).id, SymbolIdentifier::Name(n) if n == "self")
            });
            let sep = if has_self { ":" } else { "." };
            format!("{}{}{}", class_name, sep, base_name)
        } else {
            base_name.to_string()
        }
    }

    /// Returns the class name at the cursor position, or `None` if the cursor is
    /// not on a class name.
    ///
    /// Checks two contexts:
    /// 1. Annotation context (`---@class Foo`, `---@type Foo`, etc.) — `annotation_word_at`
    ///    extracts the word and we verify it is a known class.
    /// 2. Non-annotation context — resolves the symbol under the cursor and checks
    ///    whether its type is a class table.
    pub fn type_hierarchy_class_at(&self, tree: &SyntaxTree, offset: u32) -> Option<String> {
        // Annotation context: cursor on a class name inside a ---@ comment.
        if let Some(word) = self.annotation_word_at(tree, offset)
            && (self.ir.classes.contains_key(&word) || self.ir.ext.classes.contains_key(&word))
        {
            return Some(word);
        }
        // Non-annotation context: symbol whose resolved type is a class table.
        let (sym_idx, _, _) = self.find_symbol_at(tree, offset)?;
        let sym = self.sym(sym_idx);
        for version in sym.versions.iter().rev() {
            if let Some(ValueType::Table(Some(table_idx))) = &version.resolved_type
                && let Some(class_name) = self.table(*table_idx).class_name.as_deref()
            {
                return Some(class_name.to_string());
            }
        }
        None
    }

    /// Find the innermost function containing the given byte offset.
    /// Returns `None` for file-level code outside any function.
    pub fn enclosing_function_at(&self, offset: u32) -> Option<FunctionIndex> {
        let scope_idx = self.scope_at_offset(offset)?;
        let scope_to_func = self.build_scope_to_function_map();
        Self::enclosing_function_for_scope(&self.ir, scope_idx, &scope_to_func)
    }

    fn build_scope_to_function_map(&self) -> HashMap<ScopeIndex, FunctionIndex> {
        let mut map = HashMap::new();
        for (i, func) in self.ir.functions.iter().enumerate() {
            map.insert(func.scope, FunctionIndex(i));
        }
        map
    }

    fn enclosing_function_for_scope(
        ir: &super::Ir,
        scope_idx: ScopeIndex,
        scope_to_func: &HashMap<ScopeIndex, FunctionIndex>,
    ) -> Option<FunctionIndex> {
        let mut cur = Some(scope_idx);
        while let Some(s) = cur {
            if s.is_external() { break; }
            if let Some(&func_idx) = scope_to_func.get(&s) {
                return Some(func_idx);
            }
            cur = ir.scopes.get(s.val()).and_then(|sc| sc.parent);
        }
        None
    }

    fn find_event_vararg_types_at_scope(&self, scope_idx: ScopeIndex) -> Option<&Vec<ValueType>> {
        super::ancestor_scopes(&self.ir.scopes, scope_idx)
            .find_map(|s| self.event_vararg_types.get(&s))
    }

    /// Produce hover info when the cursor is on a transparent `@accessor` token
    /// (e.g. `__private` in `Widget.__private:Method()`).
    fn accessor_hover_at(&self, tree: &SyntaxTree, offset: u32, enclosing_class: Option<TableIndex>) -> Option<HoverResult> {
        let text_size = TextSize::from(offset);
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(left, right) => {
                if right.kind() == SyntaxKind::Name { right }
                else if left.kind() == SyntaxKind::Name { left }
                else { return None; }
            }
            TokenAtOffset::None => return None,
        };
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        let parent = token.parent()?;
        if !parent.kind().is_identifier() {
            return None;
        }
        let names: Vec<_> = parent.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();
        let our_index = names.iter().position(|n| n.text_range() == token.text_range())?;
        if our_index == 0 {
            return None;
        }

        // Resolve root symbol to a table
        let root_name = names[0].text().to_string();
        let scope_idx = self.scope_at_offset(text_size)?;
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
        let ver = self.sym(symbol_idx).versions.last()?;
        let mut table_idx = Self::extract_table_idx(ver.resolved_type.as_ref()?)?;

        // Walk intermediate fields before our position
        for name_token in &names[1..our_index] {
            let name = name_token.text().to_string();
            if self.ir.has_accessor(table_idx, &name) {
                continue;
            }
            table_idx = self.resolve_field_or_g_env(table_idx, &name)?;
        }

        // Check if our token is an accessor on this table
        let vis = self.ir.get_accessor(table_idx, token.text())?;
        let kind = match vis {
            crate::annotations::Visibility::Public => "accessor",
            crate::annotations::Visibility::Private => "private accessor",
            crate::annotations::Visibility::Protected => "protected accessor",
        };
        // Format the class type the same way as hovering on the base class variable
        let table_type = ValueType::Table(Some(table_idx));
        let formatted = self.format_type_accessible(&table_type, enclosing_class);
        let mut type_str = format!("({}) {}: {}", kind, token.text(), formatted);
        let doc = self.doc_for_type(&table_type);
        let doc = self.append_call_hover(table_idx, &mut type_str, doc);
        Some(HoverResult { type_str, doc })
    }

    fn varargs_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        let text_size = TextSize::from(offset);
        let is_vararg = |k: SyntaxKind| k == SyntaxKind::TripleDot || k == SyntaxKind::ParameterVarArgs;
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(left, right) => {
                if is_vararg(right.kind()) { right }
                else if is_vararg(left.kind()) { left }
                else { return None; }
            }
            TokenAtOffset::None => return None,
        };
        if !is_vararg(token.kind()) {
            return None;
        }

        let is_param_decl = token.kind() == SyntaxKind::ParameterVarArgs;
        let func_idx = self.enclosing_function_at(offset)?;
        let func = self.func(func_idx);

        if is_param_decl {
            if let Some(ref ann) = func.vararg_annotation {
                let vararg_text = format_vararg_param(ann);
                let type_str = format!("(param) {}", vararg_text);
                return Some(HoverResult { type_str, doc: func.vararg_description.clone() });
            }
            return Some(HoverResult { type_str: "(param) ...".to_string(), doc: None });
        }

        // Expression site: check event vararg types first, then annotation
        let scope_idx = self.scope_at_offset(text_size)?;
        if let Some(types) = self.find_event_vararg_types_at_scope(scope_idx) {
            let formatted: Vec<String> = types.iter()
                .map(|vt| self.format_type(vt))
                .collect();
            let type_str = format!("(varargs) ...: {}", formatted.join(", "));
            return Some(HoverResult { type_str, doc: None });
        }

        if let Some(ref ann) = func.vararg_annotation {
            let vararg_text = format_vararg_param(ann);
            let type_str = format!("(varargs) {}", vararg_text);
            return Some(HoverResult { type_str, doc: func.vararg_description.clone() });
        }

        Some(HoverResult { type_str: "(varargs) ...: ?".to_string(), doc: None })
    }

    pub fn outgoing_calls_from_function(
        &self,
        func_idx: FunctionIndex,
    ) -> Vec<OutgoingCallResult> {
        let func = self.func(func_idx);
        let body_start = func.def_node.start;
        let body_end = func.def_node.end;

        // Collect ranges of nested function definitions to exclude their calls.
        let nested_ranges: Vec<(u32, u32)> = self.ir.functions.iter()
            .filter(|f| {
                let dn = &f.def_node;
                dn.start > body_start && dn.end <= body_end
            })
            .map(|f| (f.def_node.start, f.def_node.end))
            .collect();

        let mut calls: HashMap<FunctionIndex, (String, Vec<(u32, u32)>)> = HashMap::new();

        for (expr_id, expr) in self.ir.exprs.iter().enumerate() {
            if let Expr::FunctionCall { call_range, func: callee_expr, ret_index, .. } = expr {
                if *ret_index != 0 { continue; }
                if call_range.0 < body_start || call_range.1 > body_end { continue; }
                if nested_ranges.iter().any(|&(ns, ne)| call_range.0 >= ns && call_range.1 <= ne) {
                    continue;
                }

                if let Some(resolution) = self.ir.call_resolutions.get(&ExprId(expr_id)) {
                    let target_idx = resolution.func_idx;
                    let name = self.callee_display_name(target_idx, *callee_expr);
                    let entry = calls.entry(target_idx).or_insert_with(|| (name, Vec::new()));
                    entry.1.push(*call_range);
                }
            }
        }

        calls.into_iter()
            .map(|(func_idx, (name, call_ranges))| OutgoingCallResult { func_idx, name, call_ranges })
            .collect()
    }

    fn callee_display_name(&self, func_idx: FunctionIndex, callee_expr: ExprId) -> String {
        if let Expr::FieldAccess { field, table, .. } = self.expr(callee_expr) {
            // Try class_name first, then fall back to the expression's symbol name
            let table_name = self.resolve_expr_type(*table)
                .and_then(|vt| match vt {
                    ValueType::Table(Some(idx)) => self.table(idx).class_name.clone(),
                    _ => None,
                })
                .or_else(|| self.expr_symbol_name(*table).map(str::to_owned));

            if let Some(name) = table_name {
                let func = self.func(func_idx);
                let has_self = func.args.first().is_some_and(|&s| {
                    matches!(&self.sym(s).id, SymbolIdentifier::Name(n) if n == "self")
                });
                let sep = if has_self { ":" } else { "." };
                return format!("{}{}{}", name, sep, field);
            }
        }
        self.function_name(func_idx).unwrap_or_else(|| "(anonymous)".to_string())
    }

    /// Get the symbol name for an expression if it's a simple symbol reference.
    fn expr_symbol_name(&self, expr_id: ExprId) -> Option<&str> {
        if let Expr::SymbolRef(sym_idx, _) = self.expr(expr_id)
            && let SymbolIdentifier::Name(name) = &self.sym(*sym_idx).id
        {
            return Some(name.as_str());
        }
        None
    }

    /// Collect all function call expression ranges where the callee resolves to
    /// `target_func_idx`. Used by incoming-calls to find call sites.
    pub fn call_sites_for_function(
        &self,
        target_func_idx: FunctionIndex,
    ) -> Vec<CallSiteResult> {
        let scope_to_func = self.build_scope_to_function_map();
        let mut results: Vec<CallSiteResult> = Vec::new();

        for (expr_id, expr) in self.ir.exprs.iter().enumerate() {
            if let Expr::FunctionCall { call_range, ret_index, .. } = expr {
                if *ret_index != 0 { continue; }
                if let Some(resolution) = self.ir.call_resolutions.get(&ExprId(expr_id))
                    && resolution.func_idx == target_func_idx
                {
                    let enclosing = self.scope_at_offset(call_range.0)
                        .and_then(|s| Self::enclosing_function_for_scope(&self.ir, s, &scope_to_func));
                    results.push(CallSiteResult {
                        call_range: *call_range,
                        enclosing_func: enclosing,
                    });
                }
            }
        }

        results
    }

    // ── Document symbols ────────────────────────────────────────────────────

    pub fn document_symbols(&self, tree: &SyntaxTree) -> Vec<DocumentSymbolEntry> {
        let mut class_children: HashMap<String, Vec<DocumentSymbolEntry>> = HashMap::new();
        let mut top_level: Vec<DocumentSymbolEntry> = Vec::new();

        // Build func start offset → FunctionIndex lookup for nested symbol enrichment
        let func_map: HashMap<u32, FunctionIndex> = self.ir.functions.iter().enumerate()
            .filter(|(_, f)| f.def_node != DefNode::DUMMY)
            .map(|(i, f)| (f.def_node.start, FunctionIndex::from(i)))
            .collect();

        // Collect scope-0 symbols (file-level definitions)
        for (id, &sym_idx) in &self.ir.scopes[0].symbols {
            let SymbolIdentifier::Name(name) = id else { continue };
            if sym_idx.is_external() { continue; }
            let sym = self.sym(sym_idx);
            let ver = match sym.versions.first() {
                Some(v) => v,
                None => continue,
            };
            let def = ver.def_node;
            if def == DefNode::DUMMY { continue; }

            match &ver.resolved_type {
                Some(ValueType::Function(Some(func_idx))) => {
                    let func = self.func(*func_idx);
                    let func_def = func.def_node;
                    let base_range = if func_def != DefNode::DUMMY { func_def } else { def };
                    let range = enclose_range(base_range, def);
                    let detail = self.document_symbol_func_detail(*func_idx, name);
                    top_level.push(DocumentSymbolEntry {
                        name: name.clone(),
                        detail: Some(detail),
                        kind: DocumentSymbolKind::Function,
                        range,
                        selection_range: def,
                        children: Vec::new(),
                        deprecated: func.deprecated,
                    });
                }
                Some(ValueType::Table(Some(table_idx))) => {
                    let table = self.table(*table_idx);
                    if let Some(cn) = &table.class_name {
                        // Local @class tables are handled below via ir.classes.
                        // But if the class is external (e.g. Frame), collect methods here.
                        if !self.ir.classes.contains_key(cn) {
                            let children = self.collect_table_func_children(*table_idx);
                            if !children.is_empty() {
                                top_level.push(DocumentSymbolEntry {
                                    name: name.clone(),
                                    detail: None,
                                    kind: DocumentSymbolKind::Variable,
                                    range: def,
                                    selection_range: def,
                                    children,
                                    deprecated: false,
                                });
                            }
                        }
                        continue;
                    }
                    // Non-class table: collect function-typed fields as children
                    let children = self.collect_table_func_children(*table_idx);
                    top_level.push(DocumentSymbolEntry {
                        name: name.clone(),
                        detail: None,
                        kind: DocumentSymbolKind::Variable,
                        range: def,
                        selection_range: def,
                        children,
                        deprecated: false,
                    });
                }
                _ => {
                    let detail = ver.resolved_type.as_ref().map(|vt| self.format_type_depth(vt, 0));
                    top_level.push(DocumentSymbolEntry {
                        name: name.clone(),
                        detail,
                        kind: DocumentSymbolKind::Variable,
                        range: def,
                        selection_range: def,
                        children: Vec::new(),
                        deprecated: false,
                    });
                }
            }
        }

        // Collect class methods from table fields
        for (class_name, &table_idx) in &self.ir.classes {
            if table_idx.is_external() { continue; }
            let children = self.collect_table_func_children(table_idx);
            class_children.entry(class_name.clone()).or_default().extend(children);
        }

        // Emit @class declarations as Class symbols with methods as children
        for (class_name, &table_idx) in &self.ir.classes {
            if table_idx.is_external() { continue; }
            let (range_start, range_end) = if let Some(&(s, e)) = self.ir.class_def_ranges.get(class_name) {
                (s, e)
            } else {
                continue;
            };
            let range = DefNode { start: range_start, end: range_end, node_id: None };
            let children = class_children.remove(class_name).unwrap_or_default();
            top_level.push(DocumentSymbolEntry {
                name: class_name.clone(),
                detail: None,
                kind: DocumentSymbolKind::Class,
                range,
                selection_range: range,
                children,
                deprecated: false,
            });
        }

        // Any methods whose class wasn't found as a local @class go top-level
        for (_class, methods) in class_children {
            top_level.extend(methods);
        }

        // Enrich function/method entries with nested block children
        self.enrich_with_nested_symbols(&mut top_level, tree, &func_map);

        // Extend parent ranges to encompass all children (required for sticky scroll)
        extend_ranges_to_children(&mut top_level);

        // Sort by position in file (recursively)
        sort_entries_recursive(&mut top_level);

        top_level
    }

    /// Recursively walk function/method entries and add nested blocks as children.
    fn enrich_with_nested_symbols(
        &self,
        entries: &mut [DocumentSymbolEntry],
        tree: &SyntaxTree,
        func_map: &HashMap<u32, FunctionIndex>,
    ) {
        for entry in entries.iter_mut() {
            if matches!(entry.kind, DocumentSymbolKind::Function | DocumentSymbolKind::Method)
                && let Some(node_id) = entry.range.node_id
            {
                // node_id points to the FunctionDefinition AST node; find its Block child
                let func_node = SyntaxNode { tree, id: node_id };
                if let Some(block) = func_node.children().find(|c| c.kind() == SyntaxKind::Block) {
                    let nested = self.collect_block_symbols(block, tree, func_map);
                    entry.children.extend(nested);
                }
            }
            // Recurse into existing children (e.g. class methods, table methods)
            self.enrich_with_nested_symbols(&mut entry.children, tree, func_map);
        }
    }

    /// Walk a Block AST node and collect nested document symbol entries for
    /// functions and control flow blocks.
    fn collect_block_symbols(
        &self,
        block: SyntaxNode<'_>,
        tree: &SyntaxTree,
        func_map: &HashMap<u32, FunctionIndex>,
    ) -> Vec<DocumentSymbolEntry> {
        let mut entries = Vec::new();
        let source = tree.source();

        for child in block.children() {
            let kind = child.kind();
            match kind {
                SyntaxKind::FunctionDefinition => {
                    if !is_multiline(&child, source) { continue; }
                    let start = u32::from(child.text_range().start());

                    let Some(func_def) = FunctionDefinition::cast(child) else { continue };
                    let name = func_def.name().unwrap_or_else(|| "function".to_string());
                    let detail = func_map.get(&start)
                        .map(|&idx| self.document_symbol_func_detail(idx, &name));
                    let is_method = func_map.get(&start).is_some_and(|&idx| {
                        let f = self.func(idx);
                        f.args.first().is_some_and(|&sym_idx| {
                            matches!(&self.sym(sym_idx).id, SymbolIdentifier::Name(n) if n == "self")
                        })
                    });
                    let deprecated = func_map.get(&start)
                        .is_some_and(|&idx| self.func(idx).deprecated);
                    let def_node = DefNode::from_node(child);

                    let mut entry = DocumentSymbolEntry {
                        name,
                        detail,
                        kind: if is_method { DocumentSymbolKind::Method } else { DocumentSymbolKind::Function },
                        range: def_node,
                        selection_range: def_node,
                        children: Vec::new(),
                        deprecated,
                    };

                    // Recurse into function body
                    if let Some(body) = func_def.block() {
                        entry.children = self.collect_block_symbols(body.syntax(), tree, func_map);
                    }
                    entries.push(entry);
                }
                SyntaxKind::IfChain => {
                    let Some(if_chain) = crate::ast::IfChain::cast(child) else { continue };
                    for branch in if_chain.if_branches() {
                        let br = branch.syntax();
                        if !is_multiline(&br, source) { continue; }
                        let name = extract_block_header(&br, SyntaxKind::ThenKeyword);
                        entries.push(make_block_entry(self, br, name, tree, func_map));
                    }
                    if let Some(else_branch) = if_chain.else_branch() {
                        let eb = else_branch.syntax();
                        if !is_multiline(&eb, source) { continue; }
                        entries.push(make_block_entry(self, eb, "else".to_string(), tree, func_map));
                    }
                }
                SyntaxKind::WhileLoop | SyntaxKind::ForCountLoop | SyntaxKind::ForInLoop => {
                    if !is_multiline(&child, source) { continue; }
                    let name = extract_block_header(&child, SyntaxKind::DoKeyword);
                    entries.push(make_block_entry(self, child, name, tree, func_map));
                }
                SyntaxKind::DoBlock => {
                    if !is_multiline(&child, source) { continue; }
                    entries.push(make_block_entry(self, child, "do".to_string(), tree, func_map));
                }
                SyntaxKind::RepeatUntilLoop => {
                    if !is_multiline(&child, source) { continue; }
                    entries.push(make_block_entry(self, child, "repeat".to_string(), tree, func_map));
                }
                _ => {}
            }
        }
        entries
    }

    fn field_func_idx(&self, field: &FieldInfo) -> Option<FunctionIndex> {
        if let Some(Some(ValueType::Function(Some(idx)))) = self.resolved_expr_cache.get(field.expr.val()) {
            return Some(*idx);
        }
        if let Some(ValueType::Function(Some(idx))) = &field.annotation {
            return Some(*idx);
        }
        if let Expr::FunctionDef(idx) = self.expr(field.expr) {
            return Some(*idx);
        }
        None
    }

    fn collect_table_func_children(&self, table_idx: TableIndex) -> Vec<DocumentSymbolEntry> {
        let table = self.table(table_idx);
        let mut children = Vec::new();
        for (field_name, field) in &table.fields {
            let func_idx = self.field_func_idx(field);
            let Some(func_idx) = func_idx else { continue };
            let func = self.func(func_idx);
            let func_def = func.def_node;
            if func_def == DefNode::DUMMY { continue; }
            let has_self_param = func.args.first()
                .is_some_and(|&sym_idx| matches!(&self.sym(sym_idx).id, SymbolIdentifier::Name(n) if n == "self"));
            let kind = if has_self_param { DocumentSymbolKind::Method } else { DocumentSymbolKind::Function };
            let detail = self.document_symbol_func_detail(func_idx, field_name);
            let sel_range = match field.def_range {
                Some((s, e)) => DefNode { start: s, end: e, node_id: None },
                None => func_def,
            };
            children.push(DocumentSymbolEntry {
                name: field_name.clone(),
                detail: Some(detail),
                kind,
                range: enclose_range(func_def, sel_range),
                selection_range: sel_range,
                children: Vec::new(),
                deprecated: func.deprecated,
            });
        }
        children
    }

    // ── Code lens ────────────────────────────────────────────────────────────

    pub fn code_lens(&self) -> Vec<CodeLensData> {
        let mut results = Vec::new();

        // Build class_name → set of methods defined on that class (across all
        // tables). In per-file analysis, methods from `function Class:Method()`
        // end up on the variable-backed table, not the prescan class table.
        // Use function_owner_class to associate methods with classes.
        let mut class_methods: HashMap<&str, HashSet<&str>> = HashMap::new();
        for table in &self.ir.tables {
            for (field_name, field) in &table.fields {
                if let Some(func_idx) = self.field_func_idx(field)
                    && let Some(cn) = self.function_owner_class.get(&func_idx)
                {
                    class_methods.entry(cn.as_str()).or_default().insert(field_name.as_str());
                }
            }
        }
        // Also include methods from external class tables.
        for (name, &table_idx) in &self.ir.ext.classes {
            let table = &self.ir.ext.tables[table_idx.ext_offset()];
            for (field_name, field) in &table.fields {
                let is_func = field.annotation.as_ref().is_some_and(|a| matches!(a, ValueType::Function(_)))
                    || (field.expr.is_external()
                        && matches!(self.ir.ext.exprs.get(field.expr.ext_offset()), Some(Expr::FunctionDef(_))));
                if is_func {
                    class_methods.entry(name.as_str()).or_default().insert(field_name.as_str());
                }
            }
        }

        // Build child-count map: parent_class_name → count of direct subclasses.
        let mut child_counts: HashMap<&str, usize> = HashMap::new();
        for &table_idx in self.ir.classes.values() {
            for &parent_idx in &self.table(table_idx).parent_classes {
                if let Some(parent_name) = &self.table(parent_idx).class_name {
                    *child_counts.entry(parent_name.as_str()).or_insert(0) += 1;
                }
            }
        }
        for &table_idx in self.ir.ext.classes.values() {
            for &parent_idx in &self.ir.ext.tables[table_idx.ext_offset()].parent_classes {
                if let Some(parent_name) = &self.table(parent_idx).class_name {
                    *child_counts.entry(parent_name.as_str()).or_insert(0) += 1;
                }
            }
        }

        // Emit "N implementations" lens for each local @class declaration.
        for (class_name, &(range_start, range_end)) in &self.ir.class_def_ranges {
            let count = child_counts.get(class_name.as_str()).copied().unwrap_or(0);
            results.push(CodeLensData {
                range_start,
                range_end,
                kind: CodeLensKind::Implementations {
                    count,
                    class_name: class_name.clone(),
                },
            });
        }

        // Emit "overrides Parent" lens for methods that override a parent method.
        for table in &self.ir.tables {
            for (field_name, field) in &table.fields {
                let func_idx = match self.field_func_idx(field) {
                    Some(idx) => idx,
                    None => continue,
                };
                let class_name = match self.function_owner_class.get(&func_idx) {
                    Some(n) => n,
                    None => continue,
                };
                let func = self.func(func_idx);
                if func.def_node == DefNode::DUMMY { continue; }
                let class_table_idx = match self.ir.classes.get(class_name.as_str())
                    .or_else(|| self.ir.ext.classes.get(class_name.as_str()))
                {
                    Some(&idx) => idx,
                    None => continue,
                };
                if self.table(class_table_idx).parent_classes.is_empty() { continue; }
                if let Some(parent_name) = self.find_overridden_parent(class_table_idx, field_name, &class_methods) {
                    results.push(CodeLensData {
                        range_start: func.def_node.start,
                        range_end: func.def_node.end,
                        kind: CodeLensKind::Overrides {
                            parent_class: parent_name,
                        },
                    });
                }
            }
        }

        results.sort_by_key(|l| l.range_start);
        results
    }

    fn find_overridden_parent(
        &self,
        table_idx: TableIndex,
        method_name: &str,
        class_methods: &HashMap<&str, HashSet<&str>>,
    ) -> Option<String> {
        let mut visited = HashSet::new();
        self.find_overridden_parent_inner(table_idx, method_name, class_methods, &mut visited)
    }

    fn find_overridden_parent_inner(
        &self,
        table_idx: TableIndex,
        method_name: &str,
        class_methods: &HashMap<&str, HashSet<&str>>,
        visited: &mut HashSet<TableIndex>,
    ) -> Option<String> {
        let table = self.table(table_idx);
        for &parent_idx in &table.parent_classes {
            if !visited.insert(parent_idx) { continue; }
            let parent = self.table(parent_idx);
            let Some(parent_name) = parent.class_name.as_deref() else { continue; };
            if class_methods.get(parent_name).is_some_and(|m| m.contains(method_name)) {
                return Some(parent_name.to_string());
            }
            if let Some(name) = self.find_overridden_parent_inner(parent_idx, method_name, class_methods, visited) {
                return Some(name);
            }
        }
        None
    }

    fn document_symbol_func_detail(&self, func_idx: FunctionIndex, display_name: &str) -> String {
        let func = self.func(func_idx);
        let args: Vec<String> = func.args.iter().enumerate()
            .filter(|&(_, &sym_idx)| {
                if let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id {
                    return n != "self";
                }
                true
            })
            .map(|(i, &sym_idx)| {
                let param_name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let optional = func.param_optional.get(i).copied().unwrap_or(false);
                let ann_has_nil = func.param_annotations.get(i)
                    .is_some_and(crate::annotations::annotation_type_is_nullable);
                let suffix = if optional && !ann_has_nil { "?" } else { "" };
                let type_str = self.param_annotation_text(func, i)
                    .or_else(|| {
                        self.sym(sym_idx).versions.first()
                            .and_then(|v| v.resolved_type.as_ref())
                            .map(|rt| {
                                let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                                self.format_type_depth(&display_type, 1)
                            })
                    });
                match type_str {
                    Some(t) => format!("{}{}: {}", param_name, suffix, t),
                    None => format!("{}{}", param_name, suffix),
                }
            }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let rets: Vec<String> = func.return_annotations.iter()
            .map(|vt| self.format_type_depth(vt, 1))
            .collect();
        if rets.is_empty() {
            format!("function {}({})", display_name, all_args.join(", "))
        } else {
            format!("function {}({}): {}", display_name, all_args.join(", "), rets.join(", "))
        }
    }

    /// Collect code-lens targets: one entry per non-external function definition
    /// in this file. Each entry carries the function name, definition range, and
    /// a byte offset inside the name token suitable for `reference_target_at`.
    pub fn code_lens_targets(&self, tree: &SyntaxTree) -> Vec<CodeLensTarget> {
        let mut targets = Vec::new();

        // Top-level named functions (scope 0)
        for (id, &sym_idx) in &self.ir.scopes[0].symbols {
            let SymbolIdentifier::Name(name) = id else { continue };
            if sym_idx.is_external() { continue; }
            let sym = self.sym(sym_idx);
            let ver = match sym.versions.first() {
                Some(v) => v,
                None => continue,
            };
            if ver.def_node == DefNode::DUMMY { continue; }
            let Some(ValueType::Function(Some(func_idx))) = &ver.resolved_type else { continue };
            let func = self.func(*func_idx);
            let func_def = func.def_node;
            if func_def == DefNode::DUMMY { continue; }

            if let Some(name_offset) = self.def_name_token_offset(tree, ver.def_node.start, ver.def_node.end, name) {
                targets.push(CodeLensTarget {
                    name: name.clone(),
                    def_start: func_def.start,
                    def_end: func_def.end,
                    name_offset,
                });
            }
        }

        // Class/table methods and non-class table functions
        let mut visited_tables: HashSet<TableIndex> = HashSet::new();

        // Class tables (from ir.classes)
        for &table_idx in self.ir.classes.values() {
            if table_idx.is_external() { continue; }
            visited_tables.insert(table_idx);
            self.collect_field_lens_targets(tree, table_idx, &mut targets);
        }

        // Scope-0 non-class tables (e.g. `local M = {}; function M.foo() end`)
        for (id, &sym_idx) in &self.ir.scopes[0].symbols {
            let SymbolIdentifier::Name(_) = id else { continue };
            if sym_idx.is_external() { continue; }
            let sym = self.sym(sym_idx);
            let ver = match sym.versions.first() {
                Some(v) => v,
                None => continue,
            };
            if let Some(ValueType::Table(Some(table_idx))) = &ver.resolved_type
                && !table_idx.is_external() && visited_tables.insert(*table_idx) {
                    self.collect_field_lens_targets(tree, *table_idx, &mut targets);
                }
        }

        targets.sort_by_key(|t| t.def_start);
        targets.dedup_by_key(|t| t.def_start);
        targets
    }

    fn collect_field_lens_targets(&self, tree: &SyntaxTree, table_idx: TableIndex, targets: &mut Vec<CodeLensTarget>) {
        let table = self.table(table_idx);
        for (field_name, field) in &table.fields {
            let Some(func_idx) = self.field_func_idx(field) else { continue };
            let func = self.func(func_idx);
            if func.def_node == DefNode::DUMMY { continue; }
            let search_start = match field.def_range {
                Some((s, _)) => s,
                None => func.def_node.start,
            };
            let search_end = func.def_node.end;
            let Some(name_offset) = self.def_name_token_offset(tree, search_start, search_end, field_name) else {
                continue;
            };
            targets.push(CodeLensTarget {
                name: field_name.clone(),
                def_start: func.def_node.start,
                def_end: func.def_node.end,
                name_offset,
            });
        }
    }

    fn def_name_token_offset(&self, tree: &SyntaxTree, def_start: u32, def_end: u32, name: &str) -> Option<u32> {
        // Binary-search into the flat token array (sorted by byte offset)
        // instead of walking the entire tree from the root. O(log N + k)
        // where k is the number of tokens in the def_start..def_end range.
        let tokens = &tree.tokens;
        debug_assert!(tokens.len() <= u32::MAX as usize, "token count exceeds u32 — TokenId would overflow");
        let start_idx = tokens.partition_point(|t| t.start < def_start);
        for (i, t) in tokens[start_idx..].iter().enumerate() {
            if t.start > def_end { break; }
            if t.kind == SyntaxKind::Name && tree.token_text(TokenId((start_idx + i) as u32)) == name {
                return Some(t.start);
            }
        }
        None
    }

}

pub struct CallSiteResult {
    pub call_range: (u32, u32),
    pub enclosing_func: Option<FunctionIndex>,
}

pub struct OutgoingCallResult {
    pub func_idx: FunctionIndex,
    pub name: String,
    pub call_ranges: Vec<(u32, u32)>,
}

