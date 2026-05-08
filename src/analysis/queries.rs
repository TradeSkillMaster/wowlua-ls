use std::collections::{BTreeMap, HashMap, HashSet};

use crate::types::*;
use super::{AnalysisResult, Ir};
use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken, TextSize, TextRange, TokenAtOffset};
use crate::ast::{AstNode, Expression, ForInLoop, FunctionCall, FunctionDefinition, Identifier, LocalAssign, Operator};

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
    resolved_expr_cache: &HashMap<ExprId, Option<ValueType>>,
    expr_id: ExprId,
    visited: &mut HashSet<ExprId>,
    depth: usize,
) -> Option<ValueType> {
    // Check Phase 2 resolve cache first — builder chains (@builds-field / @built-name /
    // @return self) are resolved during the fixpoint loop and the result is cached here.
    // The read-only resolver can't replicate the mutable table-cloning logic, so we
    // rely on the cached result for these expressions.
    if let Some(cached) = resolved_expr_cache.get(&expr_id) {
        return cached.clone();
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
                        for eid in all_exprs {
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, eid, visited, depth + 1)
                                && !field_types.contains(&vt) {
                                    field_types.push(vt);
                                }
                        }
                    }
                    continue;
                }
                // Check parent classes
                for &parent_idx in &ir.table(idx).parent_classes {
                    if let Some(fi) = ir.get_field(parent_idx, &field) {
                        if let Some(ref ann) = fi.annotation {
                            if !field_types.contains(ann) {
                                field_types.push(ann.clone());
                            }
                        } else {
                            let expr = fi.expr;
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, expr, visited, depth + 1)
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
                    let mut vts: Vec<ValueType> = Vec::new();
                    for t in types {
                        if let ValueType::Table(Some(idx)) = t
                            && let Some(vt) = &ir.table(*idx).value_type
                                && !vts.contains(vt) { vts.push(vt.clone()); }
                    }
                    if vts.is_empty() { None } else { Some(ValueType::make_union(vts)) }
                }
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
                .map(|vt| vt.strip_type(&cast_type))
        }
        _ => None,
    }
}

/// Format a single return annotation, prefixing `...` if it's the last entry and vararg.
fn format_vararg_return(formatted: String, index: usize, func: &Function) -> String {
    if index == func.return_annotations.len() - 1 && func.has_vararg_return {
        format!("...{}", formatted)
    } else {
        formatted
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
        // Last resort: scan all field_locations for any external table that has this field
        // registered AND also has the field in its fields map. Handles cross-addon fields
        // where the build-time table index differs from the query-time table index (e.g.
        // the workspace build creates a sub-table for "Disenchant" with one index, but
        // per-file analysis resolves the type to a local table with a different index).
        if self.table(table_idx).fields.contains_key(field_name) {
            for (&other_idx, locs) in fl.iter() {
                if other_idx != table_idx
                    && other_idx.is_external()
                    && self.table(other_idx).fields.contains_key(field_name)
                    && let Some(loc) = locs.get(field_name) {
                        return Some(loc);
                    }
            }
        }
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
                    let type_str = format!("({}) {}", kind_label, self.format_function_decl(*func_idx, &qualified_name, skip_self));
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
                        let skip_primary = !field_info.extra_exprs.is_empty()
                            && matches!(self.resolve_expr_type(field_info.expr), Some(ValueType::Nil));
                        let mut types: Vec<ValueType> = Vec::new();
                        let exprs: Vec<ExprId> = if skip_primary {
                            field_info.extra_exprs.clone()
                        } else {
                            std::iter::once(field_info.expr).chain(field_info.extra_exprs.iter().copied()).collect()
                        };
                        for eid in exprs {
                            if let Some(vt) = self.resolve_expr_type(eid)
                                && !types.contains(&vt) {
                                    types.push(vt);
                                }
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
            // Use the version that was actually referenced at this token's start offset
            // (recorded during build_ir), falling back to the latest resolved version.
            // For parameters, always use version 0 (the declaration type from @param),
            // not a later version from reassignment in the body.
            let is_param = self.is_param_symbol(symbol_idx);
            let resolved = if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
                symbol.versions.get(ver_idx).and_then(|v| v.resolved_type.as_ref())
            } else if is_param {
                symbol.versions.first().and_then(|v| v.resolved_type.as_ref())
            } else if !symbol_idx.is_external() {
                // Declaration site fallback: find the version whose def_node
                // contains this token. For redefined locals (`local x = 1; local x = ""`),
                // each redefinition creates a new version with its own def_node, so we
                // must match the token offset to the correct version rather than always
                // using version 0.
                self.version_at_def_site(symbol, token_start)
                    .or_else(|| symbol.versions.first())
                    .and_then(|v| v.resolved_type.as_ref())
            } else {
                symbol.versions.iter().rev()
                    .find_map(|v| v.resolved_type.as_ref())
            };
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
                    let type_str = format!("({}) {}", kind, self.format_function_decl(*func_idx, &name, false));
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
                    if matches!(stripped, ValueType::Nil) {
                        // Type was only nil — don't strip, show as-is
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
                let formatted = self.format_type_accessible(type_to_format, enclosing_class);
                let type_args = self.get_symbol_type_args(symbol_idx, token_start);
                let formatted = self.append_type_args_to_class(&formatted, type_to_format, &type_args);
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

    /// Extract the identifier word at the given byte offset if it falls inside a `---` comment token.
    /// Returns `(word, token_text_range)` where word is the class/alias name.
    fn annotation_word_at(&self, tree: &SyntaxTree, offset: u32) -> Option<String> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;
        if token.kind() != SyntaxKind::Comment {
            return None;
        }
        let tok_text = token.text();
        if !tok_text.starts_with("---") {
            return None;
        }
        let tok_start = u32::from(token.text_range().start());
        let cursor_in_tok = (offset - tok_start) as usize;
        if cursor_in_tok >= tok_text.len() {
            return None;
        }
        let bytes = tok_text.as_bytes();
        if !(bytes[cursor_in_tok].is_ascii_alphanumeric() || bytes[cursor_in_tok] == b'_') {
            return None;
        }
        let start = tok_text[..cursor_in_tok]
            .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .map_or(0, |i| i + 1);
        let end = tok_text[cursor_in_tok..]
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .map_or(tok_text.len(), |i| cursor_in_tok + i);
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

        let call_node = token.ancestors()
            .find(|n| n.kind() == SyntaxKind::FunctionCall || n.kind() == SyntaxKind::MethodCall)?;
        let call = FunctionCall::cast(call_node)?;
        let ident = call.identifier()?;
        let is_colon = ident.is_call_to_self();

        let arg_list = call.syntax().children()
            .find(|n| n.kind() == SyntaxKind::ArgumentList)?;
        let tok_start = token.text_range().start();
        let mut arg_index = 0u32;
        for child in arg_list.children_with_tokens() {
            if child.text_range().start() >= tok_start {
                break;
            }
            if child.kind() == SyntaxKind::Comma {
                arg_index += 1;
            }
        }

        let names = ident.names();
        if names.is_empty() {
            return None;
        }
        let scope_idx = self.scope_at_offset(text_size)?;
        let func_idx = if names.len() == 1 {
            let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
            let ver = self.sym(symbol_idx).versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            match ver {
                ValueType::Function(Some(idx)) => *idx,
                _ => return None,
            }
        } else {
            let root_sym = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
            let ver = self.sym(root_sym).versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            let mut table_idx = Self::extract_table_idx(ver)?;
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
        let param_idx = if is_colon { arg_index + 1 } else { arg_index } as usize;
        let ann = func.param_annotations.get(param_idx)?;
        let event_type_name = match ann {
            crate::annotations::AnnotationType::Simple(s) => s.as_str(),
            _ => return None,
        };

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
        use crate::diagnostics::expression_type::{compute_content_start, strip_long_brackets};

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
        let content = strip_long_brackets(raw_content);
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

    fn narrow_type_for_display(&self, resolved: &ValueType, symbol_idx: SymbolIndex, offset: u32) -> Option<ValueType> {
        let scope_idx = self.scope_at_offset(offset)?;
        // If the symbol was reassigned in this scope, narrowing no longer applies.
        let narrowing_active = !self.is_narrowing_overridden(symbol_idx, scope_idx);
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
        let strip_falsy = self.is_symbol_falsy_narrowed(symbol_idx, scope_idx);
        let strip_nil = strip_falsy || self.is_symbol_narrowed(symbol_idx, scope_idx);
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

    pub fn completions_at(&self, tree: &SyntaxTree, offset: u32, source: &str) -> Option<Vec<lsp_types::CompletionItem>> {
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

                        if let Some(result) = self.annotation_completions(prefix, &tok) {
                            return Some(result);
                        }
                    }
                }
        }

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
                let funcall_node = token.parent().filter(|p| p.kind() == SyntaxKind::ArgumentList)
                    .and_then(|al| al.parent())
                    .filter(|p| p.kind() == SyntaxKind::FunctionCall || p.kind() == SyntaxKind::MethodCall)?;
                Some(self.resolve_funcall_node_to_table(&funcall_node, text_size)?)
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
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(kind),
                            sort_text: Some(sort_text),
                            data: Some(serde_json::json!({"member": true, "offset": offset, "replace_start": member_offset})),
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
                    Some(CompletionItem {
                        label: name.to_string(),
                        kind: Some(kind),
                        sort_text: Some(sort_text),
                        data: Some(serde_json::json!({"member": true, "offset": offset, "replace_start": member_offset})),
                        ..CompletionItem::default()
                    })
                })
                .collect();
            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            Some(items)
        } else {
            // Scope completion: enumerate all visible symbols
            let text_size = TextSize::from(offset);
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
                            items.push(CompletionItem {
                                label: name.clone(),
                                kind: Some(kind),
                                sort_text: Some(sort_text),
                                data: Some(serde_json::json!({"scope": true, "offset": offset, "replace_start": prefix_start})),
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
                            items.push(CompletionItem {
                                label: name.clone(),
                                kind: Some(kind),
                                sort_text: Some(sort_text),
                                data: Some(serde_json::json!({"scope": true, "offset": offset, "replace_start": prefix_start})),
                                ..CompletionItem::default()
                            });
                        }
                }
            }

            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            if items.is_empty() { None } else { Some(items) }
        }
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

        let literals = Self::collect_string_literals(&expected_type?);
        if literals.is_empty() {
            return None;
        }

        let tok_text = token.text();
        let quote_char = tok_text.as_bytes().first().copied().unwrap_or(b'"');
        let closing = if quote_char == b'\'' { "'" } else { "\"" };

        let items: Vec<CompletionItem> = literals.iter().enumerate().map(|(i, lit)| {
            CompletionItem {
                label: lit.clone(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                sort_text: Some(format!("{:04}", i)),
                insert_text: Some(format!("{}{}", lit, closing)),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                filter_text: Some(format!("{}{}{}", closing, lit, closing)),
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
        // Walk up to find a FunctionCall or MethodCall ancestor
        let call_node = token.ancestors()
            .find(|n| n.kind() == SyntaxKind::FunctionCall || n.kind() == SyntaxKind::MethodCall)?;

        // Determine which argument position our string is in
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

        // Find the matching CallResolution by matching the IR FunctionCall expr's call_range
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

        // expected_args already excludes `self` for method calls, so use arg_index directly
        if let Some(resolved_arg) = call_res.expected_args.get(arg_index)
            && let Some(ref et) = resolved_arg.expected_type
        {
            let literals = Self::collect_string_literals(et);
            if !literals.is_empty() {
                return Some(et.clone());
            }
        }

        let is_colon = call_node.kind() == SyntaxKind::MethodCall
            || FunctionCall::cast(call_node)
                .and_then(|c| c.identifier())
                .map(|id| id.is_call_to_self())
                .unwrap_or(false);
        let param_index = if is_colon { arg_index + 1 } else { arg_index };
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
    ) -> Option<Vec<lsp_types::CompletionItem>> {
        let after_dashes = prefix.trim_start_matches('-');

        if !after_dashes.starts_with('@') {
            return None;
        }

        let after_at = &after_dashes[1..];

        if let Some(items) = self.try_tag_completions(after_at, token) {
            return Some(items);
        }
        if let Some(items) = self.try_param_name_completions(after_at, token) {
            return Some(items);
        }
        if let Some(items) = self.try_type_completions(after_at) {
            return Some(items);
        }

        None
    }

    fn try_tag_completions(&self, after_at: &str, token: &SyntaxToken) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        if after_at.contains(' ') || after_at.contains('\t') {
            return None;
        }

        // Context flags for each tag
        const F: u8 = 1; // function context
        const C: u8 = 2; // class context
        const S: u8 = 4; // standalone / fresh context
        #[allow(clippy::identity_op)] // bare F/C/S without `|` triggers identity_op
        const TAGS: &[(&str, &str, u8)] = &[
            ("param",          "Document a function parameter",               F),
            ("return",         "Document return type(s)",                     F),
            ("type",           "Declare variable type",                       S),
            ("class",          "Define a class",                              S),
            ("field",          "Define a class field",                    C),
            ("alias",          "Define a type alias",                         S),
            ("enum",           "Define an enum",                              S),
            ("event",          "Declare an event with a typed payload",       S),
            ("overload",       "Define an overload signature",            F|C),
            ("defclass",       "Generic that auto-creates classes",       F),
            ("generic",        "Declare generic type parameter(s)",       F),
            ("cast",           "Cast a variable's type",                      S),
            ("as",             "Inline type assertion",                       S),
            ("builds-field",   "Builder method adds field to built type", F),
            ("built-name",     "Set built table class name from param",   F),
            ("built-extends",  "Built type inherits from receiver",       F),
            ("constructor",    "Mark as constructor method",              F|C),
            ("deprecated",     "Mark as deprecated",                      F|C|S),
            ("nodiscard",      "Warn if return value is ignored",         F|C),
            ("private",        "Mark as private visibility",              F|C|S),
            ("protected",      "Mark as protected visibility",            F|C|S),
            ("accessor",       "Define accessor with visibility",           C),
            ("meta",           "Mark file as meta (declaration-only)",         S),
            ("diagnostic",     "Control diagnostic suppression",          F|C|S),
            ("type-narrows",   "Type guard that narrows target param",    F),
            ("flavor-narrows", "Flavor guard that narrows WoW API availability", F),
            ("correlated",     "Declare fields that are always nil/non-nil together", C),
            ("see",            "Cross-reference link to related symbol or URL", F|C|S),
        ];

        let ctx = self.detect_annotation_context(token);
        let ctx_mask = match ctx {
            AnnotationContext::Function => F,
            AnnotationContext::Class => C,
            AnnotationContext::Any => F | C | S,
        };

        let partial = after_at;
        let items: Vec<CompletionItem> = TAGS.iter()
            .filter(|(name, _, flags)| name.starts_with(partial) && (flags & ctx_mask) != 0)
            .map(|(name, detail, _)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(detail.to_string()),
                ..CompletionItem::default()
            })
            .collect();

        if items.is_empty() { None } else { Some(items) }
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
                            | "built-extends" | "type-narrows" | "defclass" | "flavor-narrows" => {
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
                // Find the receiver: could be an Identifier or a FunctionCall/MethodCall (chained methods).
                // Check for FunctionCall/MethodCall children first (chained calls resolve through
                // return type), then fall back to pure identifier children.
                let is_call_node = |k: SyntaxKind| k == SyntaxKind::FunctionCall || k == SyntaxKind::MethodCall;
                let table_idx = if let Some(funcall_node) = parent.children().find(|c| is_call_node(c.kind())) {
                    self.resolve_funcall_node_to_table(&funcall_node, text_size)
                } else if let Some(ident_node) = parent.children().find(|c| c.kind().is_identifier()) {
                    self.resolve_identifier_to_table(&ident_node, text_size)
                } else {
                    None
                };
                if let Some(table_idx) = table_idx {
                    if let Some(fi) = self.get_field(table_idx, &method_name) {
                        return Some((table_idx, method_name, fi.expr, FieldAccessKind::Colon));
                    }
                    // Check parent classes
                    for &parent_idx in &self.table(table_idx).parent_classes.clone() {
                        if let Some(fi) = self.get_field(parent_idx, &method_name) {
                            return Some((parent_idx, method_name, fi.expr, FieldAccessKind::Colon));
                        }
                    }
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
            // Check call children first, then pure identifiers
            let table_idx = if let Some(funcall_node) = parent.children().find(|c| is_call_kind(c.kind())) {
                self.resolve_funcall_node_to_table(&funcall_node, text_size)
            } else if let Some(child_ident) = parent.children().find(|c| c.kind().is_identifier()) {
                self.resolve_identifier_to_table(&child_ident, text_size)
            } else {
                None
            };
            if let Some(table_idx) = table_idx {
                let field_name = names[0].text().to_string();
                if let Some(fi) = self.get_field(table_idx, &field_name) {
                    return Some((table_idx, field_name, fi.expr, access));
                }
                // Check parent classes
                for &parent_idx in &self.table(table_idx).parent_classes.clone() {
                    if let Some(fi) = self.get_field(parent_idx, &field_name) {
                        return Some((parent_idx, field_name, fi.expr, access));
                    }
                }
                if let Some(result) = self.resolve_g_env_field(table_idx, &field_name, access) {
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
            let ft = self.resolve_field_type(fi)?;
            return Self::extract_table_idx(&ft);
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

    /// Given a table and a method name, resolve the method's first return type to a table index.
    fn resolve_method_return_table(&self, table_idx: TableIndex, method_name: &str) -> Option<TableIndex> {
        // Find the method field in this table or parent classes
        let field_expr = self.get_field(table_idx, method_name).map(|fi| fi.expr)
            .or_else(|| {
                self.table(table_idx).parent_classes.clone().iter()
                    .find_map(|&p| self.get_field(p, method_name).map(|fi| fi.expr))
            })?;
        // Resolve to function type
        let func_type = self.resolve_expr_type(field_expr)?;
        let func_idx = match func_type {
            ValueType::Function(Some(idx)) => idx,
            _ => return None,
        };
        // @return self: return the receiver's table
        if self.func(func_idx).returns_self {
            return Some(table_idx);
        }
        self.resolve_func_return_table(func_idx)
    }

    /// Resolve a function call's return type to a table index.
    /// Given a func_idx, gets the first return type and extracts the table index.
    fn resolve_func_return_table(&self, func_idx: FunctionIndex) -> Option<TableIndex> {
        self.resolve_func_return_table_with_node(func_idx, None)
    }

    fn resolve_func_return_table_with_node(&self, func_idx: FunctionIndex, call_node: Option<&SyntaxNode>) -> Option<TableIndex> {
        // For @defclass functions, resolve the class from the string literal argument
        let func_info = self.func(func_idx);
        if func_info.defclass.is_some()
            && let Some(node) = call_node
                && let Some(arg_list) = node.children().find(|c| c.kind() == SyntaxKind::ArgumentList) {
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
            && let Some(node) = call_node
                && let Some(result) = self.resolve_backtick_generic_return(func_idx, node) {
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
            } else {
                return None;
            };
            return self.resolve_method_return_table(receiver_table, &method_name);
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
                return self.resolve_method_return_table(receiver_table, &method_name);
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
                    return self.resolve_func_return_table_with_node(func_idx, Some(node));
                } else {
                    // Simple function call: func(args)
                    let root_name = names[0].text().to_string();
                    let scope_idx = self.scope_at_offset(scope_offset)?;
                    let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
                    let ver = self.sym(symbol_idx).versions.last()?;
                    let resolved = ver.resolved_type.as_ref()?;
                    match resolved {
                        ValueType::Function(Some(func_idx)) => {
                            return self.resolve_func_return_table_with_node(*func_idx, Some(node));
                        }
                        ValueType::Table(Some(table_idx)) => {
                            // Constructor call: class table called as function
                            if let Some(call_func_idx) = self.table(*table_idx).call_func {
                                return self.resolve_func_return_table_with_node(call_func_idx, Some(node));
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
        self.resolve_method_return_table(receiver_table, &method_name)
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

    /// Validate that the symbol at offset can be renamed. Returns (token_range, current_name).
    /// Rejects external symbols (WoW API stubs) and external table fields.
    pub(crate) fn prepare_rename_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(TextRange, String)> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).right_biased()?;
        if token.kind() != SyntaxKind::Name && token.kind() != SyntaxKind::Parameter {
            return None;
        }
        let name = token.text().to_string();

        // Try symbol first
        if let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, offset) {
            if symbol_idx.is_external() {
                return None; // Cannot rename external symbols
            }
            return Some((token.text_range(), name));
        }
        // Try field
        if let Some((table_idx, _, _, _)) = self.resolve_field_chain_at(tree, offset) {
            if table_idx.is_external() {
                return None; // Cannot rename external table fields
            }
            return Some((token.text_range(), name));
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

    fn append_type_args_to_class(&self, formatted: &str, vt: &ValueType, type_args: &[ValueType]) -> String {
        if type_args.is_empty() {
            return formatted.to_string();
        }
        if let ValueType::Table(Some(idx)) = vt
            && let Some(ref class_name) = self.table(*idx).class_name
                && formatted.starts_with(class_name.as_str()) {
                    let args_str = type_args.iter()
                        .map(|a| self.format_type_depth(a, 1))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return format!("{}<{}>", class_name, args_str);
                }
        formatted.to_string()
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
                let indent = "  ";
                let is_accessible = |fi: &FieldInfo| -> bool {
                    match fi.visibility {
                        crate::annotations::Visibility::Public => true,
                        crate::annotations::Visibility::Private => {
                            enclosing_class.is_some_and(|ec| self.same_class(ec, *table_idx))
                        }
                        crate::annotations::Visibility::Protected => {
                            enclosing_class.is_some_and(|ec| self.is_subclass_of(ec, *table_idx))
                        }
                    }
                };
                let mut seen: HashSet<&str> = HashSet::new();
                let mut fields: Vec<String> = table.fields.iter()
                    .filter(|(_, fi)| is_accessible(fi))
                    .map(|(name, field_info)| {
                        seen.insert(name.as_str());
                        let type_str = self.format_field_type(field_info, 0);
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
                if let Some(ov) = overlay {
                    for (name, field_info) in ov.iter() {
                        if seen.insert(name.as_str()) && is_accessible(field_info) {
                            let type_str = self.format_field_type(field_info, 0);
                            fields.push(format!("{}{}: {}", indent, name, type_str));
                        }
                    }
                }
                // Include inherited fields from parent classes
                for &parent_idx in &table.parent_classes {
                    let parent_table = self.table(parent_idx);
                    for (name, field_info) in &parent_table.fields {
                        if seen.insert(name.as_str()) && is_accessible(field_info) {
                            let type_str = self.format_field_type(field_info, 0);
                            fields.push(format!("{}{}: {}", indent, name, type_str));
                        }
                    }
                }
                if fields.is_empty() {
                    return class_name.clone();
                }
                fields.sort();
                return format!("{} {{\n{}\n}}", class_name, fields.join(",\n"));
            }
        }
        self.format_type(vt)
    }

    pub(crate) fn format_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        self.format_value_type_depth(vt, depth)
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
                Some(ann) => {
                    let type_text = crate::annotations::format_annotation_type(ann);
                    format!("...: {}", type_text)
                }
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let rets: Vec<String> = if func.returns_self {
            vec!["self".to_string()]
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
        match vt {
            ValueType::Any => "any".to_string(),
            ValueType::Nil => "nil".to_string(),
            ValueType::Boolean(Some(true)) => "true".to_string(),
            ValueType::Boolean(Some(false)) => "false".to_string(),
            ValueType::Boolean(None) => "boolean".to_string(),
            ValueType::Number => "number".to_string(),
            ValueType::String(Some(val)) => format!("\"{}\"", val),
            ValueType::String(None) => "string".to_string(),
            ValueType::Function(Some(func_idx)) => {
                let func = self.func(*func_idx);
                let args: Vec<String> = func.args.iter().enumerate().map(|(i, &sym_idx)| {
                    let name = match &self.sym(sym_idx).id {
                        SymbolIdentifier::Name(n) => n.clone(),
                        _ => "?".to_string(),
                    };
                    let optional = func.param_optional.get(i).copied().unwrap_or(false);
                    let ann_has_nil = func.param_annotations.get(i)
                        .is_some_and(crate::annotations::annotation_type_is_nullable);
                    let suffix = if optional && !ann_has_nil { "?" } else { "" };
                    // Prefer raw annotation text (preserves alias names) over resolved type
                    let type_str = self.param_annotation_text(func, i)
                        .or_else(|| {
                            // Use version 0 only (declaration type from @param), not a
                            // later version from type-guard narrowing in the body.
                            self.sym(sym_idx).versions.first()
                                .and_then(|v| v.resolved_type.as_ref())
                                .map(|rt| {
                                    let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                                    self.format_type_depth(&display_type, depth + 1)
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
                        Some(ann) => {
                            let type_text = crate::annotations::format_annotation_type(ann);
                            format!("...: {}", type_text)
                        }
                        None => "...".to_string(),
                    };
                    all_args.push(vararg_str);
                }
                let rets: Vec<String> = if func.returns_self {
                    vec!["self".to_string()]
                } else if !func.return_annotations.is_empty() {
                    func.return_annotations.iter().enumerate().map(|(i, vt)| {
                        let formatted = self.format_value_type_depth(vt, depth + 1);
                        format_vararg_return(formatted, i, func)
                    }).collect()
                } else {
                    self.format_inferred_returns(func, depth + 1)
                };
                let primary = if rets.is_empty() {
                    format!("fun({})", all_args.join(", "))
                } else {
                    format!("fun({}): {}", all_args.join(", "), rets.join(", "))
                };
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
                    let mut seen: HashSet<&str> = HashSet::new();
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        seen.insert(name.as_str());
                        let type_str = self.format_field_type(field_info, depth);
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
                    if let Some(ov) = overlay {
                        for (name, field_info) in ov.iter() {
                            if seen.insert(name.as_str()) {
                                let type_str = self.format_field_type(field_info, depth);
                                fields.push(format!("{}{}: {}", indent, name, type_str));
                            }
                        }
                    }
                    // Include inherited fields from parent classes
                    for &parent_idx in &table.parent_classes {
                        let parent_table = self.table(parent_idx);
                        for (name, field_info) in &parent_table.fields {
                            if seen.insert(name.as_str()) {
                                let type_str = self.format_field_type(field_info, depth);
                                fields.push(format!("{}{}: {}", indent, name, type_str));
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
                    // Compact inline format for nested anonymous tables
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
        let active_parameter = {
            let mut commas = 0u32;
            for child in arg_list.children_with_tokens() {
                if child.text_range().start() >= text_size {
                    break;
                }
                if child.kind() == SyntaxKind::Comma {
                    commas += 1;
                }
            }
            commas
        };

        // Resolve the function being called
        let ident = call.identifier()?;
        let names = ident.names();
        if names.is_empty() {
            return None;
        }

        let scope_idx = self.scope_at_offset(text_size)?;
        let func_idx = if names.len() == 1 {
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

        // Primary signature
        let primary = self.build_signature_info(func, is_colon);
        signatures.push(primary);

        // Overload signatures
        for overload in &func.overloads {
            signatures.push(self.build_overload_signature_info(overload));
        }

        let active_signature = Some(0);

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

        let rets: Vec<String> = if func.returns_self {
            vec!["self".to_string()]
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
                Some(ann) => {
                    let type_text = crate::annotations::format_annotation_type(ann);
                    format!("...: {}", type_text)
                }
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
                Some(crate::annotations::format_annotation_type(ann))
            }
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
            AnnotationType::Tuple(positions, _) => positions.iter().any(|p| self.annotation_has_unresolvable(&p.typ, generics)),
        }
    }


    /// Format a function in declaration style for hover: `function name(params)\n  -> ret`
    /// If `skip_self` is true, the first "self" parameter is omitted (colon-style methods).
    /// Format inferred return types (no `@return` annotation case). Returns
    /// empty when there are no value-returning return statements (void).
    /// When there are inferred returns and the function has an implicit nil
    /// return, nil is unioned into each resolved position.
    fn format_inferred_returns(&self, func: &Function, depth: usize) -> Vec<String> {
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

    fn format_function_decl(&self, func_idx: FunctionIndex, name: &str, skip_self: bool) -> String {
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
                match type_str {
                    Some(t) => format!("{}{}: {}", param_name, suffix, t),
                    None => format!("{}{}", param_name, suffix),
                }
            }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => {
                    let type_text = crate::annotations::format_annotation_type(ann);
                    format!("...: {}", type_text)
                }
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let rets: Vec<String> = if func.returns_self {
            vec!["self".to_string()]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                let formatted = self.format_value_type_depth(vt, 1);
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
                            Some(vt) => format!("{}{}: {}", p.name, opt, self.format_value_type_depth(vt, 1)),
                            None => format!("{}{}", p.name, opt),
                        }
                    }).collect();
                let ov_rets: Vec<String> = overload.returns.iter()
                    .map(|vt| self.format_value_type_depth(vt, 1))
                    .collect();
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
                        .map(|vt| self.format_value_type_depth(vt, 1))
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

            let formatted = self.format_type_depth(resolved, 1);
            if formatted == "?" { continue; }

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

        let rets = self.format_inferred_returns(func, 1);
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

            let formatted = self.format_type_depth(resolved, 1);

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

            let formatted = self.format_type_depth(resolved, 1);
            if formatted == "?" { continue; }

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

            let formatted = self.format_type_depth(&resolved, 1);
            if formatted == "?" {
                continue;
            }

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
                let type_text = crate::annotations::format_annotation_type(ann);
                let type_str = format!("(param) ...: {}", type_text);
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
            let type_text = crate::annotations::format_annotation_type(ann);
            let type_str = format!("(varargs) ...: {}", type_text);
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
        if let Some(Some(ValueType::Function(Some(idx)))) = self.resolved_expr_cache.get(&field.expr) {
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
                Some(ann) => format!("...: {}", crate::annotations::format_annotation_type(ann)),
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
        let root = SyntaxNode::new_root(tree);
        for token in root.descendants_with_tokens().filter_map(|it| it.into_token()) {
            let start = u32::from(token.text_range().start());
            if start > def_end { break; }
            if start < def_start { continue; }
            if token.kind() == SyntaxKind::Name && token.text() == name {
                return Some(start);
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

