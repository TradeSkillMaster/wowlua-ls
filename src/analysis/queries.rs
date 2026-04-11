use std::collections::{HashMap, HashSet};

use crate::types::*;
use super::{Analysis, AnalysisResult, Ir};
use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken, TextSize, TextRange, TokenAtOffset};
use crate::ast::{AstNode, Expression, FunctionCall, Identifier, Operator};

// ── Shared free functions (used by both Analysis and AnalysisResult) ─────────

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
    if expr_id < EXT_BASE && !visited.insert(expr_id) {
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
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, eid, visited, depth + 1) {
                                if !field_types.contains(&vt) {
                                    field_types.push(vt);
                                }
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
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, expr, visited, depth + 1) {
                                if !field_types.contains(&vt) {
                                    field_types.push(vt);
                                }
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
            let func_idx = match func_type {
                ValueType::Function(Some(idx)) => idx,
                ValueType::Table(Some(table_idx)) => {
                    ir.table(table_idx).call_func?
                }
                _ => return None,
            };
            let func_info = ir.func(func_idx);
            // Handle @return self
            if func_info.returns_self && ret_index == 0 {
                if let Expr::FieldAccess { table: receiver_expr, .. } = ir.expr(func).clone() {
                    if let Some(rt) = resolve_expr_type_impl(ir, resolved_expr_cache, receiver_expr, visited, depth + 1) {
                        return Some(rt);
                    }
                }
            }
            // Handle @return built: return the accumulated built_table from the receiver
            if func_info.returns_built && ret_index == 0 {
                if let Expr::FieldAccess { table: receiver_expr, .. } = ir.expr(func).clone() {
                    if let Some(ValueType::Table(Some(recv_idx))) = resolve_expr_type_impl(ir, resolved_expr_cache, receiver_expr, visited, depth + 1) {
                        if let Some(built_idx) = ir.table(recv_idx).built_table {
                            return Some(ValueType::Table(Some(built_idx)));
                        }
                        return Some(ValueType::Table(None));
                    }
                }
            }
            let ret_id = SymbolIdentifier::FunctionRet(func_idx, ret_index);
            let ret_sym_idx = ir.get_symbol(&ret_id, func_info.scope)?;
            ir.sym(ret_sym_idx).versions.first()?.resolved_type.clone()
        }
        Expr::BracketIndex { table, .. } => {
            let table = *table;
            let table_type = resolve_expr_type_impl(ir, resolved_expr_cache, table, visited, depth + 1)?;
            match &table_type {
                ValueType::Table(Some(idx)) => ir.table(*idx).value_type.clone(),
                ValueType::Union(types) => {
                    let mut vts: Vec<ValueType> = Vec::new();
                    for t in types {
                        if let ValueType::Table(Some(idx)) = t {
                            if let Some(vt) = &ir.table(*idx).value_type {
                                if !vts.contains(vt) { vts.push(vt.clone()); }
                            }
                        }
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
                        ir.ext.addon_table_idx.map(|idx| ValueType::Table(Some(idx)))
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
        _ => None,
    }
}

/// Simplified type formatter for diagnostic messages (resolve/check phases).
/// The full `AnalysisResult::format_value_type_depth` handles additional display
/// concerns (overloads, overlay fields, annotation text) needed for hover/completion.
pub(super) fn format_value_type_depth_impl(
    ir: &Ir,
    resolved_expr_cache: &HashMap<ExprId, Option<ValueType>>,
    vt: &ValueType,
    depth: usize,
) -> String {
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
            let func_idx = *func_idx;
            let func = ir.func(func_idx);
            let args_str = func.args.iter().enumerate().map(|(i, &sym_idx)| {
                let name = match &ir.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let optional = func.param_optional.get(i).copied().unwrap_or(false);
                let ann_has_nil = func.param_annotations.get(i)
                    .map_or(false, |ann| crate::annotations::annotation_type_is_nullable(ann));
                let suffix = if optional && !ann_has_nil { "?" } else { "" };
                let type_str = ir.sym(sym_idx).versions.first()
                    .and_then(|v| v.resolved_type.as_ref())
                    .map(|rt| {
                        let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                        format_value_type_depth_impl(ir, resolved_expr_cache, &display_type, depth + 1)
                    });
                match type_str {
                    Some(t) => format!("{}{}: {}", name, suffix, t),
                    None => format!("{}{}", name, suffix),
                }
            }).collect::<Vec<_>>().join(", ");
            let rets_str = if func.returns_self {
                "self".to_string()
            } else if !func.return_annotations.is_empty() {
                func.return_annotations.iter().map(|vt| format_value_type_depth_impl(ir, resolved_expr_cache, vt, depth + 1)).collect::<Vec<_>>().join(", ")
            } else {
                func.rets.iter().map(|&sym_idx| {
                    match ir.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                        Some(rt) => format_value_type_depth_impl(ir, resolved_expr_cache, rt, depth + 1),
                        None => "?".to_string(),
                    }
                }).collect::<Vec<_>>().join(", ")
            };
            if rets_str.is_empty() {
                format!("fun({})", args_str)
            } else {
                format!("fun({}): {}", args_str, rets_str)
            }
        }
        ValueType::Function(None) => "function".to_string(),
        ValueType::Table(Some(idx)) => {
            let table = ir.table(*idx);
            if let Some(ref class_name) = table.class_name {
                class_name.clone()
            } else if depth > 2 {
                "table".to_string()
            } else if let (Some(key_type), Some(value_type)) = (&table.key_type, &table.value_type) {
                if *key_type == ValueType::Number {
                    format!("{}[]", format_value_type_depth_impl(ir, resolved_expr_cache, value_type, depth + 1))
                } else {
                    format!("table<{}, {}>", format_value_type_depth_impl(ir, resolved_expr_cache, key_type, depth + 1), format_value_type_depth_impl(ir, resolved_expr_cache, value_type, depth + 1))
                }
            } else if !table.fields.is_empty() {
                let fields: Vec<String> = table.fields.iter()
                    .take(10)
                    .map(|(name, fi)| {
                        let type_str = if let Some(ref ann) = fi.annotation {
                            format_value_type_depth_impl(ir, resolved_expr_cache, ann, depth + 1)
                        } else {
                            let mut v = HashSet::new();
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, fi.expr, &mut v, 0) {
                                format_value_type_depth_impl(ir, resolved_expr_cache, &vt, depth + 1)
                            } else {
                                "?".to_string()
                            }
                        };
                        format!("{}: {}", name, type_str)
                    }).collect();
                let suffix = if table.fields.len() > 10 { ", ..." } else { "" };
                format!("{{ {} {} }}", fields.join(", "), suffix)
            } else {
                "table".to_string()
            }
        }
        ValueType::Table(None) => "table".to_string(),
        ValueType::Union(types) => {
            types.iter().map(|t| format_value_type_depth_impl(ir, resolved_expr_cache, t, depth + 1)).collect::<Vec<_>>().join(" | ")
        }
        ValueType::Intersection(types) => {
            types.iter().map(|t| format_value_type_depth_impl(ir, resolved_expr_cache, t, depth + 1)).collect::<Vec<_>>().join(" & ")
        }
        ValueType::TypeVariable(name) => name.clone(),
        ValueType::Userdata => "userdata".to_string(),
        ValueType::Thread => "thread".to_string(),
    }
}

// ── LSP Queries ──────────────────────────────────────────────────────────────

impl AnalysisResult {
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
        Some((symbol_idx, name, token_start))
    }

    pub fn definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        // Try field access first so that a same-named global doesn't shadow the field.
        if let Some((table_idx, field_name, expr_id, _)) = self.resolve_field_chain_at(tree, offset) {
            if let Some(result) = self.definition_for_expr(expr_id) {
                return Some(result);
            }
            // Fall back to the field's definition range (e.g. table constructor field)
            if let Some(fi) = self.get_field(table_idx, &field_name) {
                if let Some((start, end)) = fi.def_range {
                    let range = TextRange::new(
                        TextSize::from(start),
                        TextSize::from(end),
                    );
                    return Some(DefinitionResult::Local(range));
                }
            }
            // Fall back to external field location (stubs / workspace @field annotations)
            if let Some(loc) = self.find_external_field_location(table_idx, &field_name) {
                return Some(DefinitionResult::External(loc.clone()));
            }
        }
        if let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, offset) {
            if symbol_idx >= EXT_BASE {
                if let Some(loc) = self.ir.ext.symbol_locations.get(&symbol_idx) {
                    return Some(DefinitionResult::External(loc.clone()));
                }
                return None;
            }
            let symbol = self.sym(symbol_idx);
            let version = symbol.versions.first()?;
            return Some(DefinitionResult::Local(TextRange::new(
                TextSize::from(version.def_node.start),
                TextSize::from(version.def_node.end),
            )));
        }
        // Table constructor field: definition is itself
        if let Some((_, _)) = self.find_constructor_field_at(tree, offset) {
            let text_size = TextSize::from(offset);
            if let TokenAtOffset::Single(t) | TokenAtOffset::Between(t, _) = SyntaxNode::new_root(tree).token_at_offset(text_size) {
                return Some(DefinitionResult::Local(t.text_range()));
            }
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
                if func_idx >= EXT_BASE {
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
                if sym_idx >= EXT_BASE {
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
    /// (own fields → parent classes → metatable chain).
    fn find_external_field_location(&self, table_idx: TableIndex, field_name: &str) -> Option<&ExternalLocation> {
        let fl = &self.ir.ext.field_locations;
        // Check direct table
        if let Some(loc) = fl.get(&table_idx).and_then(|m| m.get(field_name)) {
            return Some(loc);
        }
        // Walk parent classes
        for &parent_idx in &self.table(table_idx).parent_classes {
            if let Some(loc) = fl.get(&parent_idx).and_then(|m| m.get(field_name)) {
                return Some(loc);
            }
        }
        // Walk metatable __index chain
        let mut visited = std::collections::HashSet::new();
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
            // Try to resolve the field's type for function detection
            let resolved_type = self.resolve_expr_type(expr_id);
            let is_func = matches!(&resolved_type, Some(ValueType::Function(Some(_))));
            let table_name = self.table(table_idx).class_name.clone();
            let sep = match access_kind {
                FieldAccessKind::Colon => ":",
                FieldAccessKind::Dot => ".",
            };

            if is_func {
                if let Some(ValueType::Function(Some(func_idx))) = &resolved_type {
                    let skip_self = access_kind == FieldAccessKind::Colon;
                    let qualified_name = match &table_name {
                        Some(tname) => format!("{}{}{}", tname, sep, field_name),
                        None => field_name.clone(),
                    };
                    let kind_label = if access_kind == FieldAccessKind::Colon { "method" } else { "field" };
                    let type_str = format!("({}) {}", kind_label, self.format_function_decl(*func_idx, &qualified_name, skip_self));
                    let doc = self.format_function_doc(*func_idx);
                    return Some(HoverResult { type_str, doc });
                }
            }

            if let Some(field_info) = self.get_field(table_idx, &field_name) {
                let formatted = {
                    if let Some(ref text) = field_info.annotation_text {
                        // Check if annotation_text is a function type for declaration-style display
                        text.clone()
                    } else if let Some(ref ann) = field_info.annotation {
                        self.format_type_accessible(ann, enclosing_class)
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
                            if let Some(vt) = self.resolve_expr_type(eid) {
                                if !types.contains(&vt) {
                                    types.push(vt);
                                }
                            }
                        }
                        if types.is_empty() {
                            "?".to_string()
                        } else {
                            let unified = ValueType::make_union(types);
                            self.format_type_accessible(&unified, enclosing_class)
                        }
                    }
                };
                let type_str = format!("(field) {}: {}", field_name, formatted);
                let doc = resolved_type.as_ref().and_then(|r| self.doc_for_type(r));
                return Some(HoverResult { type_str, doc });
            }
            if let Some(resolved) = resolved_type {
                let type_str = format!("(field) {}: {}", field_name, self.format_type(&resolved));
                let doc = self.doc_for_type(&resolved);
                return Some(HoverResult { type_str, doc });
            }
            return None;
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
            } else {
                symbol.versions.iter().rev()
                    .find_map(|v| v.resolved_type.as_ref())
            };
            // Determine kind prefix
            let kind = if symbol_idx >= EXT_BASE || symbol.scope_idx == 0 {
                "global"
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
                if kind == "param" && ver_idx == 0 && display_type.is_none() {
                    if let Some(ann_text) = self.find_param_annotation_text(symbol_idx) {
                        let optional = self.is_param_optional(symbol_idx) || display_ref.contains_nil();
                        let suffix = if optional { "?" } else { "" };
                        let value_suffix = self.get_string_value(symbol_idx, token_start)
                            .map(|s| format!(" = \"{}\"", s))
                            .or_else(|| self.get_number_value(symbol_idx, token_start)
                                .map(|n| format!(" = {}", n)))
                            .unwrap_or_default();
                        let type_str = format!("({}) {}: {}{}{}", kind, name, ann_text, suffix, value_suffix);
                        return Some(HoverResult { type_str, doc });
                    }
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
                let type_str = format!("({}) {}: {}{}{}", kind, name, self.format_type_accessible(type_to_format, enclosing_class), optional_suffix, value_suffix);
                return Some(HoverResult { type_str, doc });
            }
            return Some(HoverResult { type_str: format!("({}) {}: ?", kind, name), doc: None });
        }
        // Try table constructor field (e.g. hovering over "count" in { count = 42 })
        if let Some((field_name, field_info)) = self.find_constructor_field_at(tree, offset) {
            if let Some(ref text) = field_info.annotation_text {
                let type_str = format!("(field) {}: {}", field_name, text);
                return Some(HoverResult { type_str, doc: None });
            }
            let type_str = format!("(field) {}: {}", field_name, self.format_field_type(&field_info, 0));
            return Some(HoverResult { type_str, doc: None });
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
            let type_str = if has_fields {
                format!("(class) {}", self.format_type_accessible(&ValueType::Table(Some(table_idx)), None))
            } else {
                format!("(class) {}", word)
            };
            return Some(HoverResult { type_str, doc: None });
        }
        // Check aliases (local + external)
        if let Some(vt) = self.ir.aliases.get(&word).or_else(|| self.ir.ext.aliases.get(&word)) {
            let type_str = format!("(alias) {} = {}", word, self.format_type(vt));
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
        if symbol_idx >= EXT_BASE {
            return self.ir.ext.string_values.get(&symbol_idx).map(|s| s.as_str());
        }
        // Local symbol: find the version's type_source and check string_literals
        let symbol = self.sym(symbol_idx);
        let version = if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
            symbol.versions.get(ver_idx)
        } else {
            symbol.versions.last()
        };
        version
            .and_then(|v| v.type_source)
            .and_then(|expr_id| self.ir.string_literals.get(&expr_id))
            .map(|s| s.as_str())
    }

    /// Get the number literal value for a symbol, checking both local and external sources.
    fn get_number_value(&self, symbol_idx: SymbolIndex, token_start: u32) -> Option<&str> {
        if symbol_idx >= EXT_BASE {
            return self.ir.ext.number_values.get(&symbol_idx).map(|s| s.as_str());
        }
        let symbol = self.sym(symbol_idx);
        let version = if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
            symbol.versions.get(ver_idx)
        } else {
            symbol.versions.last()
        };
        version
            .and_then(|v| v.type_source)
            .and_then(|expr_id| self.ir.number_literals.get(&expr_id))
            .map(|s| s.as_str())
    }

    fn narrow_type_for_display(&self, resolved: &ValueType, symbol_idx: SymbolIndex, offset: u32) -> Option<ValueType> {
        let scope_idx = self.scope_at_offset(offset)?;
        // Start from a type-narrowed base if one exists (e.g. type(x) == "string")
        let base = if let Some(narrowed_vt) = self.get_type_narrowing(symbol_idx, scope_idx) {
            Some(narrowed_vt.clone())
        } else if let Some(guard_vt) = self.get_type_filtering(symbol_idx, scope_idx) {
            Some(resolved.filter_type_with(guard_vt, &|idx| self.table(idx).is_enum))
        } else if let Some(stripped_vt) = self.get_type_stripping(symbol_idx, scope_idx) {
            Some(resolved.strip_type_with(stripped_vt, &|idx| self.table(idx).is_enum))
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
                return None; // nothing to strip
            }
            if filtered.len() == 1 {
                return Some(filtered.into_iter().next().unwrap());
            }
            if !filtered.is_empty() {
                return Some(ValueType::Union(filtered));
            }
        }
        None
    }

    fn extract_table_idx(resolved: &ValueType) -> Option<TableIndex> {
        match resolved {
            ValueType::Table(Some(idx)) => Some(*idx),
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

    fn doc_for_type(&self, st: &ValueType) -> Option<String> {
        match st {
            ValueType::Function(Some(func_idx)) => {
                self.format_function_doc(*func_idx)
            }
            _ => None,
        }
    }

    /// Build a rich doc string for a function, including its doc comment and @param descriptions.
    fn format_function_doc(&self, func_idx: FunctionIndex) -> Option<String> {
        let func = self.func(func_idx);
        let has_descriptions = func.param_descriptions.iter().any(|d| d.is_some());
        if func.doc.is_none() && !has_descriptions {
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
                        .map_or(false, |ann| crate::annotations::annotation_type_is_nullable(ann));
                    let suffix = if optional && !ann_has_nil { "?" } else { "" };
                    param_lines.push(format!("@*param* `{}{}` — {}", name, suffix, desc));
                }
            }
            if !param_lines.is_empty() {
                parts.push(param_lines.join("\n\n"));
            }
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

        // --- Annotation completion: detect if cursor is inside a ---@ comment ---
        {
            let text_size = TextSize::from(offset.saturating_sub(1));
            let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased();
            if let Some(tok) = token {
                if tok.kind() == SyntaxKind::Comment {
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
                if parent.kind() .is_identifier() {
                    let names: Vec<_> = parent.children_with_tokens()
                        .filter_map(|it| it.into_token())
                        .filter(|t| t.kind() == SyntaxKind::Name)
                        .collect();
                    let our_index = names.iter().position(|n| n.text_range() == token.text_range())?;
                    let root_name = names[0].text().to_string();
                    let scope_idx = self.scope_at_offset(text_size)?;
                    let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
                    let ver = self.sym(symbol_idx).versions.last()?;
                    let resolved = ver.resolved_type.as_ref()?;
                    let mut idx = Self::extract_table_idx(resolved)?;
                    // Walk intermediate fields
                    for i in 1..=our_index {
                        if i < names.len() {
                            let name = names[i].text().to_string();
                            let fi = self.get_field(idx, &name)?;
                            let field_type = self.resolve_field_type(fi)?;
                            idx = Self::extract_table_idx(&field_type)?;
                        }
                    }
                    Some(idx)
                } else {
                    // Single name, not in an Identifier chain
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
                        data: Some(serde_json::json!({"member": true, "offset": offset})),
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

            let mut seen = std::collections::HashSet::new();
            let mut items = Vec::new();
            let mut current_scope = Some(scope_idx);
            while let Some(si) = current_scope {
                let scope = &self.ir.scopes[si];
                for (id, &sym_idx) in &scope.symbols {
                    if let SymbolIdentifier::Name(name) = id {
                        if seen.insert(name.clone()) {
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
                                data: Some(serde_json::json!({"scope": true, "offset": offset})),
                                ..CompletionItem::default()
                            });
                        }
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
                    if let SymbolIdentifier::Name(name) = id {
                        if seen.insert(name.clone()) {
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
                                format!("1{}", name)
                            } else {
                                format!("0{}", name)
                            };
                            items.push(CompletionItem {
                                label: name.clone(),
                                kind: Some(kind),
                                sort_text: Some(sort_text),
                                data: Some(serde_json::json!({"scope": true, "offset": offset})),
                                ..CompletionItem::default()
                            });
                        }
                    }
                }
            }

            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            if items.is_empty() { None } else { Some(items) }
        }
    }

    /// Lazily resolve a completion item's `detail` field (called by completionItem/resolve).
    pub fn resolve_completion(&self, tree: &SyntaxTree, item: &mut lsp_types::CompletionItem) {
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
            if let Some(scope_idx) = scope_idx {
                if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx) {
                    let resolved = self.sym(sym_idx).versions.iter().rev()
                        .find_map(|v| v.resolved_type.as_ref());
                    if let Some(vt) = resolved {
                        item.detail = Some(self.format_type(vt));
                    }
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

        if let Some(items) = self.try_tag_completions(after_at) {
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

    fn try_tag_completions(&self, after_at: &str) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        if after_at.contains(' ') || after_at.contains('\t') {
            return None;
        }

        const TAGS: &[(&str, &str)] = &[
            ("param", "Document a function parameter"),
            ("return", "Document return type(s)"),
            ("type", "Declare variable type"),
            ("class", "Define a class"),
            ("field", "Define a class field"),
            ("alias", "Define a type alias"),
            ("enum", "Define an enum"),
            ("overload", "Define an overload signature"),
            ("defclass", "Generic that auto-creates classes"),
            ("generic", "Declare generic type parameter(s)"),
            ("cast", "Cast a variable's type"),
            ("as", "Inline type assertion"),
            ("builds-field", "Builder method adds field to built type"),
            ("built-name", "Set built table class name from param"),
            ("built-extends", "Built type inherits from receiver"),
            ("constructor", "Mark as constructor method"),
            ("deprecated", "Mark as deprecated"),
            ("nodiscard", "Warn if return value is ignored"),
            ("private", "Mark as private visibility"),
            ("protected", "Mark as protected visibility"),
            ("accessor", "Define accessor with visibility"),
            ("meta", "Mark file as meta (declaration-only)"),
            ("diagnostic", "Control diagnostic suppression"),
            ("type-narrows", "Type guard that narrows target param"),
            ("correlated", "Declare fields that are always nil/non-nil together"),
        ];

        let partial = after_at;
        let items: Vec<CompletionItem> = TAGS.iter()
            .filter(|(name, _)| name.starts_with(partial))
            .map(|(name, detail)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(detail.to_string()),
                ..CompletionItem::default()
            })
            .collect();

        if items.is_empty() { None } else { Some(items) }
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
                if let Some(func_def) = FunctionDefinition::cast(n.clone()) {
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

        // Local classes
        for name in self.ir.classes.keys() {
            if name.starts_with(type_prefix) && seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    ..CompletionItem::default()
                });
            }
        }

        // Local aliases
        for name in self.ir.aliases.keys() {
            if name.starts_with(type_prefix) && seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::INTERFACE),
                    ..CompletionItem::default()
                });
            }
        }

        // External classes (WoW API)
        for name in self.ir.ext.classes.keys() {
            if name.starts_with(type_prefix) && seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    ..CompletionItem::default()
                });
            }
        }

        // External aliases
        for name in self.ir.ext.aliases.keys() {
            if name.starts_with(type_prefix) && seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::INTERFACE),
                    ..CompletionItem::default()
                });
            }
        }

        // Local parameterized aliases
        for name in self.ir.parameterized_aliases.keys() {
            if name.starts_with(type_prefix) && seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::INTERFACE),
                    ..CompletionItem::default()
                });
            }
        }

        // External parameterized aliases
        for name in self.ir.ext.parameterized_aliases.keys() {
            if name.starts_with(type_prefix) && seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::INTERFACE),
                    ..CompletionItem::default()
                });
            }
        }

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
                t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::Colon));
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
                t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::Colon));
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
            }
            return None;
        }

        if names.len() < 2 {
            // Check grandparent: for `func().field`, the parent Identifier wraps just "field",
            // but the grandparent Identifier has a FunctionCall sibling we can resolve through.
            if names.len() == 1 {
                if let Some(grandparent) = parent.parent() {
                    if grandparent.kind() .is_identifier() {
                        if let Some(funcall_node) = grandparent.children().find(|c| c.kind() == SyntaxKind::FunctionCall || c.kind() == SyntaxKind::MethodCall) {
                            if let Some(table_idx) = self.resolve_funcall_node_to_table(&funcall_node, text_size) {
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
                        }
                    }
                }
            }
            return None;
        }
        let our_index = names.iter().position(|n| n.text_range() == token.text_range())?;
        if our_index == 0 {
            // Check if grandparent has a FunctionCall: for `func().field.sub`, cursor is on "field"
            // which is names[0] in the inner Identifier, but the root is the FunctionCall in grandparent
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() .is_identifier() {
                    if let Some(funcall_node) = grandparent.children().find(|c| c.kind() == SyntaxKind::FunctionCall || c.kind() == SyntaxKind::MethodCall) {
                        if let Some(table_idx) = self.resolve_funcall_node_to_table(&funcall_node, text_size) {
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
        for i in 1..our_index {
            let name = names[i].text().to_string();
            // Check for transparent @accessor — skip without changing table
            if self.ir.has_accessor(table_idx, &name) {
                continue;
            }
            let fi = self.get_field(table_idx, &name)?;
            let field_type = self.resolve_field_type(fi)?;
            table_idx = Self::extract_table_idx(&field_type)?;
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
        if func_info.defclass.is_some() {
            if let Some(node) = call_node {
                if let Some(arg_list) = node.children().find(|c| c.kind() == SyntaxKind::ArgumentList) {
                    // Get first string literal argument
                    for child in arg_list.descendants_with_tokens() {
                        if let NodeOrToken::Token(t) = child {
                            if t.kind() == SyntaxKind::String {
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
                }
            }
        }
        // For backtick generic functions (e.g. `@generic T` + `@param name \`T\`` + `@return T`),
        // resolve the class from the string literal at the backtick parameter position.
        if !func_info.generics.is_empty() {
            if let Some(node) = call_node {
                if let Some(result) = self.resolve_backtick_generic_return(func_idx, node) {
                    return Some(result);
                }
            }
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
            if let crate::annotations::AnnotationType::Backtick(inner) = ann {
                if let crate::annotations::AnnotationType::Simple(name) = inner.as_ref() {
                    if name == &return_generic {
                        backtick_arg_index = Some(ann_idx.saturating_sub(self_off));
                        break;
                    }
                }
            }
        }
        let target_idx = backtick_arg_index?;

        // Extract the string literal at that argument position from the call node
        let arg_list = call_node.children().find(|c| c.kind() == SyntaxKind::ArgumentList)?;
        let arg_exprs: Vec<_> = arg_list.children()
            .filter(|c| Expression::cast(c.clone()).is_some())
            .collect();
        let target_expr = arg_exprs.get(target_idx)?;
        // Find the string token in this expression
        let string_token = target_expr.descendants_with_tokens()
            .find_map(|child| {
                if let NodeOrToken::Token(t) = child {
                    if t.kind() == SyntaxKind::String { return Some(t); }
                }
                None
            })?;
        let class_name = string_token.text().trim_matches(|c| c == '"' || c == '\'').to_string();
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
                t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::Colon));

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
                    for i in 1..names.len() - 1 {
                        let name = names[i].text().to_string();
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
                        for i in 1..names.len() - 1 {
                            let name = names[i].text().to_string();
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
            t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::Colon));
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
            t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::LeftSquareBracket));

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
                    let fi = self.get_field(idx, &name)?;
                    let ft = self.resolve_field_type(fi)?;
                    idx = Self::extract_table_idx(&ft)?;
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
            let mut idx = Self::extract_table_idx(resolved)?;
            for i in 1..child_names.len() {
                let name = child_names[i].text().to_string();
                let fi = self.get_field(idx, &name)?;
                let ft = self.resolve_field_type(fi)?;
                idx = Self::extract_table_idx(&ft)?;
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
            parent.clone()
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

    /// Find all references to the symbol or field at the given offset.
    /// Returns a list of TextRanges covering each Name token that references the target.
    pub fn references_at(&self, tree: &SyntaxTree, offset: u32, include_declaration: bool) -> Option<Vec<TextRange>> {
        // Determine what we're looking for
        if let Some((symbol_idx, name, _)) = self.find_symbol_at(tree, offset) {
            // Symbol reference: find all Name tokens that resolve to the same SymbolIndex
            let mut results = Vec::new();

            // Add definition-site Name tokens from all symbol versions.
            // This catches parameter defs that are outside the function body scope
            // and wouldn't be found by the token walk below.
            if symbol_idx < EXT_BASE {
                for ver in &self.sym(symbol_idx).versions {
                    let def_end = TextSize::from(ver.def_node.end);
                    if let Some(start_token) = SyntaxNode::new_root(tree).token_at_offset(TextSize::from(ver.def_node.start)).right_biased() {
                        let mut cursor = start_token;
                        loop {
                            if (cursor.kind() == SyntaxKind::Name || cursor.kind() == SyntaxKind::Parameter)
                                && cursor.text() == name
                            {
                                results.push(cursor.text_range());
                                break;
                            }
                            match cursor.next_token() {
                                Some(next) if next.text_range().start() < def_end => cursor = next,
                                _ => break,
                            }
                        }
                    }
                }
            }

            for token in SyntaxNode::new_root(tree).descendants_with_tokens().filter_map(|it| it.into_token()) {
                if token.kind() != SyntaxKind::Name || token.text() != name {
                    continue;
                }
                // Skip tokens that are part of a field chain (not the root position)
                if let Some(parent) = token.parent() {
                    if parent.kind() .is_identifier() {
                        let names: Vec<_> = parent.children_with_tokens()
                            .filter_map(|it| it.into_token())
                            .filter(|t| t.kind() == SyntaxKind::Name)
                            .collect();
                        if names.len() >= 2 {
                            if let Some(pos) = names.iter().position(|n| n.text_range() == token.text_range()) {
                                if pos > 0 {
                                    continue; // This is a field, not a symbol reference
                                }
                            }
                        }
                    }
                }
                let text_size = token.text_range().start();
                if let Some(scope_idx) = self.scope_at_offset(text_size) {
                    if let Some(resolved) = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx) {
                        if resolved == symbol_idx {
                            results.push(token.text_range());
                        }
                    }
                }
            }

            // Deduplicate (def sites may overlap with walk results)
            results.sort_by_key(|r| (r.start(), r.end()));
            results.dedup();

            // Filter out declaration if not requested
            if !include_declaration && symbol_idx < EXT_BASE {
                if let Some(first_def) = self.sym(symbol_idx).versions.first().map(|v| TextRange::new(TextSize::from(v.def_node.start), TextSize::from(v.def_node.end))) {
                    results.retain(|r| *r != first_def);
                }
            }

            if results.is_empty() { None } else { Some(results) }
        } else if let Some((table_idx, field_name, _, _)) = self.resolve_field_chain_at(tree, offset) {
            // Field reference: find all Name tokens in dot/colon chains that resolve to the same table+field
            let mut results = Vec::new();
            for token in SyntaxNode::new_root(tree).descendants_with_tokens().filter_map(|it| it.into_token()) {
                if token.kind() != SyntaxKind::Name || token.text() != field_name {
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
                let root_name = if is_parser2_field {
                    // Parser2 DotAccess: walk nested identifiers to find root name
                    let Some(ident) = Identifier::cast(parent.clone()) else { continue };
                    let chain_names = ident.names();
                    if chain_names.is_empty() { continue; }
                    chain_names[0].clone()
                } else {
                    // Old-style flat Identifier: need at least 2 Name tokens
                    let flat_names: Vec<_> = parent.children_with_tokens()
                        .filter_map(|it| it.into_token())
                        .filter(|t| t.kind() == SyntaxKind::Name)
                        .collect();
                    if flat_names.len() < 2 { continue; }
                    let our_idx = match flat_names.iter().position(|n| n.text_range() == token.text_range()) {
                        Some(idx) if idx > 0 => idx,
                        _ => continue,
                    };
                    let _ = our_idx;
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
                    // Old-style flat Identifier: walk intermediate names
                    let our_index = {
                        let names_list: Vec<_> = parent.children_with_tokens()
                            .filter_map(|it| it.into_token())
                            .filter(|t| t.kind() == SyntaxKind::Name)
                            .collect();
                        names_list.iter().position(|n| n.text_range() == token.text_range()).unwrap_or(0)
                    };
                    let names_list: Vec<_> = parent.children_with_tokens()
                        .filter_map(|it| it.into_token())
                        .filter(|t| t.kind() == SyntaxKind::Name)
                        .collect();
                    for i in 1..our_index {
                        let n = names_list[i].text().to_string();
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
                if matched && cur_table == table_idx {
                    results.push(token.text_range());
                }
            }
            if results.is_empty() { None } else { Some(results) }
        } else {
            None
        }
    }

    /// Validate that the symbol at offset can be renamed. Returns (token_range, current_name).
    /// Rejects external symbols (WoW API stubs) and external table fields.
    pub fn prepare_rename_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(TextRange, String)> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).right_biased()?;
        if token.kind() != SyntaxKind::Name && token.kind() != SyntaxKind::Parameter {
            return None;
        }
        let name = token.text().to_string();

        // Try symbol first
        if let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, offset) {
            if symbol_idx >= EXT_BASE {
                return None; // Cannot rename external symbols
            }
            return Some((token.text_range(), name));
        }
        // Try field
        if let Some((table_idx, _, _, _)) = self.resolve_field_chain_at(tree, offset) {
            if table_idx >= EXT_BASE {
                return None; // Cannot rename external table fields
            }
            return Some((token.text_range(), name));
        }
        None
    }

    /// Find all locations that need to be renamed. Built on top of references_at.
    pub fn rename_at(&self, tree: &SyntaxTree, offset: u32, _new_name: &str) -> Option<Vec<TextRange>> {
        self.prepare_rename_at(tree, offset)?;
        self.references_at(tree, offset, true)
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
            if let Some(vt) = self.resolve_expr_type(eid) {
                if !types.contains(&vt) { types.push(vt); }
            }
        }
        if types.is_empty() { None } else { Some(ValueType::make_union(types)) }
    }

    pub(crate) fn format_type(&self, vt: &ValueType) -> String {
        self.format_type_depth(vt, 0)
    }

    /// Format a type for hover display, filtering out inaccessible private/protected fields.
    fn format_type_accessible(&self, vt: &ValueType, enclosing_class: Option<TableIndex>) -> String {
        if let ValueType::Table(Some(table_idx)) = vt {
            let table = self.table(*table_idx);
            let overlay = self.ir.overlay_fields.get(table_idx);
            let has_fields = !table.fields.is_empty() || overlay.is_some_and(|o| !o.is_empty());
            if let Some(ref class_name) = table.class_name {
                if !has_fields {
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
                let mut fields: Vec<String> = table.fields.iter()
                    .filter(|(_, fi)| is_accessible(fi))
                    .map(|(name, field_info)| {
                        let type_str = self.format_field_type(field_info, 0);
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
                if let Some(ov) = overlay {
                    for (name, field_info) in ov.iter() {
                        if !table.fields.contains_key(name) && is_accessible(field_info) {
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
            if let Some(vt) = self.resolve_expr_type(expr_id) {
                if !types.contains(&vt) {
                    types.push(vt);
                }
            }
        }
        if types.is_empty() {
            return "?".to_string();
        }
        let unified = ValueType::make_union(types);
        self.format_type_depth(&unified, depth + 1)
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
                        .map_or(false, |ann| crate::annotations::annotation_type_is_nullable(ann));
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
                    func.return_annotations.iter().map(|vt| {
                        self.format_value_type_depth(vt, depth + 1)
                    }).collect()
                } else {
                    func.rets.iter().map(|&sym_idx| {
                        match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                            Some(rt) => self.format_type_depth(rt, depth + 1),
                            None => "?".to_string(),
                        }
                    }).collect()
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
                if table.class_name.is_none() {
                    if let Some(ref val_vt) = table.value_type {
                        let val_str = self.format_value_type_depth(val_vt, depth + 1);
                        return match &table.key_type {
                            Some(ValueType::Number) | None => format!("{}[]", val_str),
                            Some(key_vt) => {
                                let key_str = self.format_value_type_depth(key_vt, depth + 1);
                                format!("table<{}, {}>", key_str, val_str)
                            }
                        };
                    }
                }
                if let Some(ref class_name) = table.class_name {
                    if !has_fields || depth > 0 {
                        return class_name.clone();
                    }
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
                    return format!("{} {{\n{}\n}}", class_name, fields.join(",\n"));
                }
                if !has_fields || depth > 0 {
                    "table".to_string()
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
            ValueType::Union(types) => {
                let parts: Vec<String> = types.iter().map(|t| self.format_value_type_depth(t, depth + 1)).collect();
                parts.join(" | ")
            }
            ValueType::Intersection(types) => {
                let parts: Vec<String> = types.iter().map(|t| self.format_value_type_depth(t, depth + 1)).collect();
                parts.join(" & ")
            }
            ValueType::TypeVariable(name) => name.clone(),
            ValueType::Userdata => "userdata".to_string(),
            ValueType::Thread => "thread".to_string(),
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
                if skip_self {
                    if let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id {
                        return n != "self";
                    }
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
                    .map_or(false, |ann| crate::annotations::annotation_type_is_nullable(ann));
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
            func.return_annotations.iter().map(|vt| {
                self.format_value_type_depth(vt, 1)
            }).collect()
        } else {
            func.rets.iter().map(|&sym_idx| {
                match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                    Some(rt) => self.format_type_depth(rt, 1),
                    None => "?".to_string(),
                }
            }).collect()
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

    /// Check if a symbol is a function parameter.
    fn is_param_symbol(&self, symbol_idx: SymbolIndex) -> bool {
        if symbol_idx >= EXT_BASE {
            return false;
        }
        self.ir.functions.iter().any(|f| f.args.contains(&symbol_idx))
    }

    fn is_param_optional(&self, symbol_idx: SymbolIndex) -> bool {
        if symbol_idx >= EXT_BASE {
            return false;
        }
        for f in &self.ir.functions {
            if let Some(pos) = f.args.iter().position(|&s| s == symbol_idx) {
                return f.param_optional.get(pos).copied().unwrap_or(false);
            }
        }
        false
    }

    /// Find the annotation text for a param symbol by locating its function.
    /// Returns the formatted annotation with nil members stripped (since the
    /// caller adds `?` for optional/nil-containing types).
    fn find_param_annotation_text(&self, symbol_idx: SymbolIndex) -> Option<String> {
        if symbol_idx >= EXT_BASE {
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
        }
    }


    /// Format a function in declaration style for hover: `function name(params)\n  -> ret`
    /// If `skip_self` is true, the first "self" parameter is omitted (colon-style methods).
    fn format_function_decl(&self, func_idx: FunctionIndex, name: &str, skip_self: bool) -> String {
        let func = self.func(func_idx);
        let args: Vec<String> = func.args.iter().enumerate()
            .filter(|&(i, &sym_idx)| {
                if skip_self && i == 0 {
                    if let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id {
                        return n != "self";
                    }
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
                    .map_or(false, |ann| crate::annotations::annotation_type_is_nullable(ann));
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
            func.return_annotations.iter().map(|vt| {
                self.format_value_type_depth(vt, 1)
            }).collect()
        } else {
            func.rets.iter().map(|&sym_idx| {
                match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                    Some(rt) => self.format_type_depth(rt, 1),
                    None => "?".to_string(),
                }
            }).collect()
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
        // Append overloads
        if !func.overloads.is_empty() {
            for overload in &func.overloads {
                let ov_args: Vec<String> = overload.params.iter()
                    .filter(|p| !(skip_self && p.name == "self"))
                    .map(|p| {
                        match &p.typ {
                            Some(vt) => format!("{}: {}", p.name, self.format_value_type_depth(vt, 1)),
                            None => p.name.clone(),
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
        }
        result
    }

    fn format_overload(&self, overload: &ResolvedOverload) -> String {
        let args: Vec<String> = overload.params.iter().map(|p| {
            match &p.typ {
                Some(vt) => format!("{}: {}", p.name, self.format_value_type_depth(vt, 1)),
                None => p.name.clone(),
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

}

// ── Build-phase methods on Analysis (also used by resolve.rs / checks.rs) ────

impl<'a> Analysis<'a> {
    pub(crate) fn resolve_expr_type(&self, expr_id: ExprId) -> Option<ValueType> {
        let mut visited = HashSet::new();
        resolve_expr_type_impl(&self.ir, &self.resolved_expr_cache, expr_id, &mut visited, 0)
    }

    pub(crate) fn format_value_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        format_value_type_depth_impl(&self.ir, &self.resolved_expr_cache, vt, depth)
    }
}

