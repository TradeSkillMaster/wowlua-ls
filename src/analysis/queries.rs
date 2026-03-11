use std::collections::HashSet;

use crate::types::*;
use super::Analysis;
use crate::diagnostics::WowDiagnostic;
use crate::syntax::SyntaxKind;
use crate::ast::{AstNode, FunctionCall, Operator};

// ── LSP Queries ──────────────────────────────────────────────────────────────

impl Analysis {
    pub(crate) fn find_symbol_at(&self, offset: u32) -> Option<(SymbolIndex, String, u32)> {
        let text_size = rowan::TextSize::from(offset);
        let is_name_or_param = |k: SyntaxKind| k == SyntaxKind::Name || k == SyntaxKind::Parameter;
        let token = match self.root.token_at_offset(text_size) {
            rowan::TokenAtOffset::Single(t) => t,
            rowan::TokenAtOffset::Between(left, right) => {
                if is_name_or_param(right.kind()) { right }
                else if is_name_or_param(left.kind()) { left }
                else { return None; }
            }
            rowan::TokenAtOffset::None => return None,
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

    pub fn is_meta(&self) -> bool {
        self.is_meta
    }

    pub fn diagnostics(&self) -> &[WowDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn function_name(&self, func_idx: FunctionIndex) -> Option<String> {
        // Search local symbols
        for sym in &self.ir.symbols {
            if let SymbolIdentifier::Name(name) = &sym.id {
                for ver in &sym.versions {
                    if let Some(ValueType::Function(Some(idx))) = &ver.resolved_type {
                        if *idx == func_idx { return Some(name.clone()); }
                    }
                }
            }
        }
        // Search external symbols
        for sym in &self.ir.ext.symbols {
            if let SymbolIdentifier::Name(name) = &sym.id {
                for ver in &sym.versions {
                    if let Some(ValueType::Function(Some(idx))) = &ver.resolved_type {
                        if *idx == func_idx { return Some(name.clone()); }
                    }
                }
            }
        }
        None
    }

    pub fn definition_at(&self, offset: u32) -> Option<DefinitionResult> {
        // Try field access first so that a same-named global doesn't shadow the field.
        if let Some((_field_name, expr_id)) = self.find_field_at(offset) {
            return self.definition_for_expr(expr_id);
        }
        if let Some((symbol_idx, _, _)) = self.find_symbol_at(offset) {
            if symbol_idx >= EXT_BASE {
                if let Some(loc) = self.ir.ext.symbol_locations.get(&symbol_idx) {
                    return Some(DefinitionResult::External(loc.clone()));
                }
                return None;
            }
            let symbol = self.sym(symbol_idx);
            let version = symbol.versions.first()?;
            return Some(DefinitionResult::Local(version.def_node.text_range()));
        }
        // Table constructor field: definition is itself
        if let Some((_, _)) = self.find_constructor_field_at(offset) {
            let text_size = rowan::TextSize::from(offset);
            if let rowan::TokenAtOffset::Single(t) | rowan::TokenAtOffset::Between(t, _) = self.root.token_at_offset(text_size) {
                return Some(DefinitionResult::Local(t.text_range()));
            }
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
                Some(DefinitionResult::Local(func.def_node.text_range()))
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
                Some(DefinitionResult::Local(version.def_node.text_range()))
            }
            _ => None,
        }
    }

    pub fn hover_at(&self, offset: u32) -> Option<HoverResult> {
        // Compute enclosing class for visibility filtering in hover tooltips
        let enclosing_class = {
            let text_size = rowan::TextSize::from(offset);
            let node = self.root.token_at_offset(text_size)
                .right_biased()
                .and_then(|t| t.parent());
            node.and_then(|n| self.find_enclosing_class(&n))
        };
        // Try field access first (e.g. "GetText" in Inbox.GetText) so that
        // a same-named global doesn't shadow the field result.
        if let Some((table_idx, field_name, expr_id, access_kind)) = self.resolve_field_chain_at(offset) {
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
                    let doc = self.func(*func_idx).doc.clone();
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
        if let Some((symbol_idx, name, token_start)) = self.find_symbol_at(offset) {
            let symbol = self.sym(symbol_idx);
            // Use the version that was actually referenced at this token's start offset
            // (recorded during build_ir), falling back to the latest resolved version.
            let resolved = if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
                symbol.versions.get(ver_idx).and_then(|v| v.resolved_type.as_ref())
            } else {
                symbol.versions.iter().rev()
                    .find_map(|v| v.resolved_type.as_ref())
            };
            // Determine kind prefix
            let kind = if symbol_idx >= EXT_BASE || symbol.scope_idx == 0 {
                "global"
            } else if self.is_param_symbol(symbol_idx) {
                "param"
            } else {
                "local"
            };
            if let Some(resolved) = resolved {
                let display_type = self.narrow_type_for_display(resolved, symbol_idx, offset);
                let display_ref = display_type.as_ref().unwrap_or(resolved);
                let doc = self.doc_for_type(display_ref);
                // Declaration-style for functions
                if let ValueType::Function(Some(func_idx)) = display_ref {
                    let type_str = format!("({}) {}", kind, self.format_function_decl(*func_idx, &name, false));
                    return Some(HoverResult { type_str, doc });
                }
                let type_str = format!("({}) {}: {}", kind, name, self.format_type_accessible(display_ref, enclosing_class));
                return Some(HoverResult { type_str, doc });
            }
            return Some(HoverResult { type_str: format!("({}) {}: ?", kind, name), doc: None });
        }
        // Try table constructor field (e.g. hovering over "count" in { count = 42 })
        if let Some((field_name, field_info)) = self.find_constructor_field_at(offset) {
            if let Some(ref text) = field_info.annotation_text {
                let type_str = format!("(field) {}: {}", field_name, text);
                return Some(HoverResult { type_str, doc: None });
            }
            let type_str = format!("(field) {}: {}", field_name, self.format_field_type(&field_info, 0));
            return Some(HoverResult { type_str, doc: None });
        }
        None
    }

    fn narrow_type_for_display(&self, resolved: &ValueType, symbol_idx: SymbolIndex, offset: u32) -> Option<ValueType> {
        let scope_idx = self.scope_at_offset(rowan::TextSize::from(offset))?;
        // Check for type() guard narrowing first (e.g. type(x) == "string")
        if let Some(narrowed_vt) = self.get_type_narrowing(symbol_idx, scope_idx) {
            return Some(narrowed_vt.clone());
        }
        if !self.is_symbol_narrowed(symbol_idx, scope_idx) {
            return None;
        }
        // Strip Nil from union types
        if let ValueType::Union(types) = resolved {
            let filtered: Vec<_> = types.iter().filter(|t| **t != ValueType::Nil).cloned().collect();
            if filtered.len() == types.len() {
                return None; // no nil to strip
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
            ValueType::Union(types) => {
                types.iter().find_map(|t| match t {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                })
            }
            _ => None,
        }
    }

    fn doc_for_type(&self, st: &ValueType) -> Option<String> {
        match st {
            ValueType::Function(Some(func_idx)) => {
                self.func(*func_idx).doc.clone()
            }
            _ => None,
        }
    }

    pub fn completions_at(&self, offset: u32, source: &str) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        if offset == 0 {
            return None;
        }

        let prev_char = source.as_bytes().get((offset - 1) as usize).copied()?;

        if prev_char == b'.' || prev_char == b':' {
            // Dot/colon completion: resolve the prefix to a table, enumerate fields
            if offset < 2 { return None; }
            let prefix_offset = offset - 2;
            let text_size = rowan::TextSize::from(prefix_offset);
            let token = self.root.token_at_offset(text_size).right_biased()?;

            // Handle function call return completions: func(). or func():
            // The token before the dot is ')' (RightBracket), so resolve the FunctionCall
            let table_idx = if token.kind() == SyntaxKind::RightBracket {
                let funcall_node = token.parent().filter(|p| p.kind() == SyntaxKind::ArgumentList)
                    .and_then(|al| al.parent())
                    .filter(|p| p.kind() == SyntaxKind::FunctionCall)?;
                Some(self.resolve_funcall_node_to_table(&funcall_node, text_size)?)
            } else if token.kind() != SyntaxKind::Name {
                return None;
            } else if let Some(parent) = token.parent() {
                if parent.kind() == SyntaxKind::Identifier {
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
                            let field_expr_id = self.get_field(idx, &name)?.expr;
                            let field_type = self.resolve_expr_type(field_expr_id)?;
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
                let node = self.root.token_at_offset(text_size)
                    .right_biased()
                    .and_then(|t| t.parent());
                node.and_then(|n| self.find_enclosing_class(&n))
            };
            // Collect all fields: base table + overlay (for external tables)
            let overlay = self.ir.overlay_fields.get(&table_idx);
            let mut all_fields: Vec<(&String, &FieldInfo)> = table.fields.iter().collect();
            if let Some(ov) = overlay {
                for (name, fi) in ov.iter() {
                    if !table.fields.contains_key(name) {
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
                    let (detail, kind) = match &resolved {
                        Some(ValueType::Function(_)) => {
                            (Some(self.format_type(resolved.as_ref().unwrap())),
                             CompletionItemKind::METHOD)
                        }
                        Some(st) => {
                            if is_colon {
                                return None; // colon completions only show methods
                            }
                            (Some(self.format_type(st)), CompletionItemKind::FIELD)
                        }
                        None => {
                            if is_colon { return None; }
                            (None, CompletionItemKind::FIELD)
                        }
                    };
                    Some(CompletionItem {
                        label: name.to_string(),
                        kind: Some(kind),
                        detail,
                        ..CompletionItem::default()
                    })
                })
                .collect();
            items.sort_by(|a, b| a.label.cmp(&b.label));
            Some(items)
        } else {
            // Scope completion: enumerate all visible symbols
            let text_size = rowan::TextSize::from(offset);
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
                            let (detail, kind) = match resolved {
                                Some(ValueType::Function(_)) => {
                                    (Some(self.format_type(resolved.unwrap())),
                                     CompletionItemKind::FUNCTION)
                                }
                                Some(ValueType::Table(Some(idx))) => {
                                    let k = if self.table(*idx).class_name.is_some() {
                                        CompletionItemKind::CLASS
                                    } else {
                                        CompletionItemKind::VARIABLE
                                    };
                                    (Some(self.format_type(resolved.unwrap())), k)
                                }
                                Some(st) => {
                                    (Some(self.format_type(st)), CompletionItemKind::VARIABLE)
                                }
                                None => (None, CompletionItemKind::VARIABLE),
                            };
                            items.push(CompletionItem {
                                label: name.clone(),
                                kind: Some(kind),
                                detail,
                                ..CompletionItem::default()
                            });
                        }
                    }
                }
                current_scope = scope.parent;
            }

            // Include external globals (WoW API functions, tables, etc.)
            for (id, &sym_idx) in &self.ir.ext.scope0_symbols {
                if let SymbolIdentifier::Name(name) = id {
                    if seen.insert(name.clone()) {
                        let resolved = self.sym(sym_idx).versions.iter().rev()
                            .find_map(|v| v.resolved_type.as_ref());
                        let (detail, kind) = match resolved {
                            Some(ValueType::Function(_)) => {
                                (Some(self.format_type(resolved.unwrap())),
                                 CompletionItemKind::FUNCTION)
                            }
                            Some(ValueType::Table(Some(idx))) => {
                                let k = if self.table(*idx).class_name.is_some() {
                                    CompletionItemKind::CLASS
                                } else {
                                    CompletionItemKind::MODULE
                                };
                                (Some(self.format_type(resolved.unwrap())), k)
                            }
                            Some(st) => {
                                (Some(self.format_type(st)), CompletionItemKind::VARIABLE)
                            }
                            None => (None, CompletionItemKind::VARIABLE),
                        };
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(kind),
                            detail,
                            ..CompletionItem::default()
                        });
                    }
                }
            }

            items.sort_by(|a, b| a.label.cmp(&b.label));
            if items.is_empty() { None } else { Some(items) }
        }
    }

    /// Resolve a dot/colon chain at offset, returning (owning_table_idx, field_name, field_expr_id, access_kind).
    pub(crate) fn resolve_field_chain_at(&self, offset: u32) -> Option<(TableIndex, String, ExprId, FieldAccessKind)> {
        let text_size = rowan::TextSize::from(offset);
        let token = match self.root.token_at_offset(text_size) {
            rowan::TokenAtOffset::Single(t) => t,
            rowan::TokenAtOffset::Between(left, right) => {
                if right.kind() == SyntaxKind::Name { right }
                else if left.kind() == SyntaxKind::Name { left }
                else { return None; }
            }
            rowan::TokenAtOffset::None => return None,
        };
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        let parent = token.parent()?;

        // Handle method name in FunctionCall: expr:method(args)
        // The Name token is a direct child of FunctionCall, preceded by Colon
        if parent.kind() == SyntaxKind::FunctionCall {
            let has_colon = parent.children_with_tokens().any(|t|
                t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::Colon));
            if has_colon {
                let method_name = token.text().to_string();
                // Find the receiver: could be an Identifier or a FunctionCall (chained methods)
                let table_idx = if let Some(ident_node) = parent.children().find(|c| c.kind() == SyntaxKind::Identifier) {
                    self.resolve_identifier_to_table(&ident_node, text_size)
                } else if let Some(funcall_node) = parent.children().find(|c| c.kind() == SyntaxKind::FunctionCall) {
                    self.resolve_funcall_node_to_table(&funcall_node, text_size)
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

        if parent.kind() != SyntaxKind::Identifier {
            return None;
        }
        // Collect direct Name tokens in the Identifier
        let names: Vec<_> = parent.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();

        // Handle method/field after a child Identifier or FunctionCall (e.g. t[k]:method, chained calls)
        // The parent Identifier has a child node (the base) and one direct Name (the field/method).
        let has_child_ident = parent.children().any(|c| c.kind() == SyntaxKind::Identifier);
        let has_child_funcall = parent.children().any(|c| c.kind() == SyntaxKind::FunctionCall);
        if (has_child_ident || has_child_funcall) && names.len() == 1 {
            let has_colon = parent.children_with_tokens().any(|t|
                t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::Colon));
            let access = if has_colon { FieldAccessKind::Colon } else { FieldAccessKind::Dot };
            let table_idx = if let Some(child_ident) = parent.children().find(|c| c.kind() == SyntaxKind::Identifier) {
                self.resolve_identifier_to_table(&child_ident, text_size)
            } else if let Some(funcall_node) = parent.children().find(|c| c.kind() == SyntaxKind::FunctionCall) {
                self.resolve_funcall_node_to_table(&funcall_node, text_size)
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
                    if grandparent.kind() == SyntaxKind::Identifier {
                        if let Some(funcall_node) = grandparent.children().find(|c| c.kind() == SyntaxKind::FunctionCall) {
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
                if grandparent.kind() == SyntaxKind::Identifier {
                    if let Some(funcall_node) = grandparent.children().find(|c| c.kind() == SyntaxKind::FunctionCall) {
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
            if grandparent.kind() == SyntaxKind::Identifier {
                if let Some(funcall_node) = grandparent.children().find(|c| c.kind() == SyntaxKind::FunctionCall) {
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
            if self.table(table_idx).accessors.contains_key(&name) {
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
    fn detect_access_before_token(parent: &crate::syntax::SyntaxNode, token: &crate::syntax::SyntaxToken) -> FieldAccessKind {
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

    fn resolve_func_return_table_with_node(&self, func_idx: FunctionIndex, call_node: Option<&crate::syntax::SyntaxNode>) -> Option<TableIndex> {
        // For @defclass functions, resolve the class from the string literal argument
        let func_info = self.func(func_idx);
        if func_info.defclass.is_some() {
            if let Some(node) = call_node {
                if let Some(arg_list) = node.children().find(|c| c.kind() == SyntaxKind::ArgumentList) {
                    // Get first string literal argument
                    for child in arg_list.descendants_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = child {
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
    fn resolve_backtick_generic_return(&self, func_idx: FunctionIndex, call_node: &crate::syntax::SyntaxNode) -> Option<TableIndex> {
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
            .filter(|c| c.kind() == SyntaxKind::Expression || c.kind() == SyntaxKind::Literal)
            .collect();
        let target_expr = arg_exprs.get(target_idx)?;
        // Find the string token in this expression
        let string_token = target_expr.descendants_with_tokens()
            .find_map(|child| {
                if let rowan::NodeOrToken::Token(t) = child {
                    if t.kind() == SyntaxKind::String { return Some(t); }
                }
                None
            })?;
        let class_name = string_token.text().trim_matches(|c| c == '"' || c == '\'').to_string();
        self.ir.classes.get(&class_name).copied()
            .or_else(|| self.ir.ext.classes.get(&class_name).copied())
    }

    /// Resolve a FunctionCall syntax node to the table its return type represents.
    /// Handles colon method calls, dot-calls, and chained combinations.
    fn resolve_funcall_node_to_table(&self, node: &crate::syntax::SyntaxNode, scope_offset: rowan::TextSize) -> Option<TableIndex> {
        if let Some(ident_node) = node.children().find(|c| c.kind() == SyntaxKind::Identifier) {
            let has_colon = ident_node.children_with_tokens().any(|t|
                t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::Colon));

            let names: Vec<_> = ident_node.children_with_tokens()
                .filter_map(|it| it.into_token())
                .filter(|t| t.kind() == SyntaxKind::Name)
                .collect();

            if has_colon {
                // Colon method call: receiver:method(args)
                let method_name = names.last()?.text().to_string();
                let receiver_table = if let Some(child_ident) = ident_node.children().find(|c| c.kind() == SyntaxKind::Identifier) {
                    self.resolve_identifier_to_table(&child_ident, scope_offset)?
                } else if let Some(child_funcall) = ident_node.children().find(|c| c.kind() == SyntaxKind::FunctionCall) {
                    self.resolve_funcall_node_to_table(&child_funcall, scope_offset)?
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
                if names.len() >= 2 {
                    // Dot chain: resolve up to the table, then get the function field
                    let child_funcall = ident_node.children().find(|c| c.kind() == SyntaxKind::FunctionCall);
                    let child_ident = ident_node.children().find(|c| c.kind() == SyntaxKind::Identifier);
                    let base_table = if let Some(ci) = child_ident {
                        self.resolve_identifier_to_table(&ci, scope_offset)?
                    } else if let Some(cf) = child_funcall {
                        self.resolve_funcall_node_to_table(&cf, scope_offset)?
                    } else {
                        // Simple dot chain with no nested nodes
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
                    let func_idx = match resolved {
                        ValueType::Function(Some(idx)) => *idx,
                        _ => return None,
                    };
                    return self.resolve_func_return_table_with_node(func_idx, Some(node));
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
        let receiver_table = if let Some(funcall_node) = node.children().find(|c| c.kind() == SyntaxKind::FunctionCall) {
            self.resolve_funcall_node_to_table(&funcall_node, scope_offset)?
        } else {
            return None;
        };
        self.resolve_method_return_table(receiver_table, &method_name)
    }

    /// Resolve an Identifier syntax node to the table it represents.
    /// Handles simple dot chains and bracket-indexed chains (e.g. `t.f[k]`).
    fn resolve_identifier_to_table(&self, node: &crate::syntax::SyntaxNode, scope_offset: rowan::TextSize) -> Option<TableIndex> {
        let child_names: Vec<_> = node.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();

        // Check for nested Identifier (bracket indexing like private.tbl[k])
        let child_ident = node.children().find(|c| c.kind() == SyntaxKind::Identifier);
        let has_bracket = node.children_with_tokens().any(|t|
            t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::LeftSquareBracket));

        let child_funcall = node.children().find(|c| c.kind() == SyntaxKind::FunctionCall);

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

    pub(crate) fn find_field_at(&self, offset: u32) -> Option<(String, ExprId)> {
        let (_, name, expr_id, _) = self.resolve_field_chain_at(offset)?;
        Some((name, expr_id))
    }

    /// Resolve a field name inside a table constructor (e.g. `components` in `{ components = {} }`).
    /// Returns (field_name, field_info) if the token at offset is a named field key.
    pub(crate) fn find_constructor_field_at(&self, offset: u32) -> Option<(String, FieldInfo)> {
        let text_size = rowan::TextSize::from(offset);
        let token = match self.root.token_at_offset(text_size) {
            rowan::TokenAtOffset::Single(t) => t,
            rowan::TokenAtOffset::Between(left, right) => {
                if right.kind() == SyntaxKind::Name { right }
                else if left.kind() == SyntaxKind::Name { left }
                else { return None; }
            }
            rowan::TokenAtOffset::None => return None,
        };
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        // Field names in constructors are wrapped: Field > Identifier > Name
        let parent = token.parent()?;
        let field_node = if parent.kind() == SyntaxKind::Identifier {
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
            matches!(n, rowan::NodeOrToken::Token(ref t) if t.kind() == SyntaxKind::Assign)
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
    pub fn references_at(&self, offset: u32, include_declaration: bool) -> Option<Vec<rowan::TextRange>> {
        // Determine what we're looking for
        if let Some((symbol_idx, name, _)) = self.find_symbol_at(offset) {
            // Symbol reference: find all Name tokens that resolve to the same SymbolIndex
            let mut results = Vec::new();

            // Add definition-site Name tokens from all symbol versions.
            // This catches parameter defs that are outside the function body scope
            // and wouldn't be found by the token walk below.
            if symbol_idx < EXT_BASE {
                for ver in &self.sym(symbol_idx).versions {
                    let def_end = ver.def_node.text_range().end();
                    if let Some(start_token) = self.root.token_at_offset(ver.def_node.text_range().start()).right_biased() {
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

            for token in self.root.descendants_with_tokens().filter_map(|it| it.into_token()) {
                if token.kind() != SyntaxKind::Name || token.text() != name {
                    continue;
                }
                // Skip tokens that are part of a field chain (not the root position)
                if let Some(parent) = token.parent() {
                    if parent.kind() == SyntaxKind::Identifier {
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
                if let Some(first_def) = self.sym(symbol_idx).versions.first().map(|v| v.def_node.text_range()) {
                    results.retain(|r| *r != first_def);
                }
            }

            if results.is_empty() { None } else { Some(results) }
        } else if let Some((table_idx, field_name, _, _)) = self.resolve_field_chain_at(offset) {
            // Field reference: find all Name tokens in dot/colon chains that resolve to the same table+field
            let mut results = Vec::new();
            for token in self.root.descendants_with_tokens().filter_map(|it| it.into_token()) {
                if token.kind() != SyntaxKind::Name || token.text() != field_name {
                    continue;
                }
                // Must be in a multi-part Identifier and not the root
                let parent = match token.parent() {
                    Some(p) if p.kind() == SyntaxKind::Identifier => p,
                    _ => continue,
                };
                let names: Vec<_> = parent.children_with_tokens()
                    .filter_map(|it| it.into_token())
                    .filter(|t| t.kind() == SyntaxKind::Name)
                    .collect();
                if names.len() < 2 {
                    continue;
                }
                let our_index = match names.iter().position(|n| n.text_range() == token.text_range()) {
                    Some(idx) if idx > 0 => idx,
                    _ => continue,
                };
                // Walk the chain to check if it resolves to the same table+field
                let root_name = names[0].text().to_string();
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
                for i in 1..our_index {
                    let n = names[i].text().to_string();
                    match self.get_field(cur_table, &n) {
                        Some(field_info) => match self.resolve_expr_type(field_info.expr).as_ref().and_then(Self::extract_table_idx) {
                            Some(next) => cur_table = next,
                            _ => { matched = false; break; }
                        },
                        None => { matched = false; break; }
                    }
                }
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
    pub fn prepare_rename_at(&self, offset: u32) -> Option<(rowan::TextRange, String)> {
        let text_size = rowan::TextSize::from(offset);
        let token = self.root.token_at_offset(text_size).right_biased()?;
        if token.kind() != SyntaxKind::Name && token.kind() != SyntaxKind::Parameter {
            return None;
        }
        let name = token.text().to_string();

        // Try symbol first
        if let Some((symbol_idx, _, _)) = self.find_symbol_at(offset) {
            if symbol_idx >= EXT_BASE {
                return None; // Cannot rename external symbols
            }
            return Some((token.text_range(), name));
        }
        // Try field
        if let Some((table_idx, _, _, _)) = self.resolve_field_chain_at(offset) {
            if table_idx >= EXT_BASE {
                return None; // Cannot rename external table fields
            }
            return Some((token.text_range(), name));
        }
        None
    }

    /// Find all locations that need to be renamed. Built on top of references_at.
    pub fn rename_at(&self, offset: u32, _new_name: &str) -> Option<Vec<rowan::TextRange>> {
        self.prepare_rename_at(offset)?;
        self.references_at(offset, true)
    }

    pub(crate) fn resolve_expr_type(&self, expr_id: ExprId) -> Option<ValueType> {
        let mut visited = HashSet::new();
        self.resolve_expr_type_inner(expr_id, &mut visited)
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

    fn resolve_expr_type_inner(&self, expr_id: ExprId, visited: &mut HashSet<ExprId>) -> Option<ValueType> {
        // External exprs (>= EXT_BASE) are immutable/shared and can legitimately appear
        // multiple times in method chains (e.g. repeated :AddField() calls on the same class).
        // Only track local exprs for cycle detection.
        if expr_id < EXT_BASE && !visited.insert(expr_id) {
            return None;
        }
        match self.expr(expr_id) {
            Expr::Literal(vt) => Some(vt.clone()),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let sym = self.sym(*sym_idx);
                sym.versions[*ver_idx].resolved_type.clone()
            }
            Expr::FunctionDef(func_idx) => {
                Some(ValueType::Function(Some(*func_idx)))
            }
            Expr::TableConstructor(table_idx) => {
                Some(ValueType::Table(Some(*table_idx)))
            }
            Expr::Grouped(inner) => self.resolve_expr_type_inner(*inner, visited),
            Expr::BinaryOp { op, lhs, rhs } => {
                let (op, lhs, rhs) = (*op, *lhs, *rhs);
                let lhs_type = self.resolve_expr_type_inner(lhs, visited);
                let rhs_type = self.resolve_expr_type_inner(rhs, visited);
                match (lhs_type, rhs_type) {
                    (Some(l), Some(r)) => self.resolve_binary_op(op, l, r),
                    (Some(ValueType::Number), None) | (None, Some(ValueType::Number))
                        if op.is_arithmetic() => Some(ValueType::Number),
                    (Some(ref t), None) | (None, Some(ref t))
                        if op == Operator::Concatenate && t.can_concat_to_string() => Some(ValueType::String),
                    _ if op.is_comparison() => Some(ValueType::Boolean(None)),
                    _ => None,
                }
            }
            Expr::UnaryOp { op, operand } => {
                let (op, operand) = (*op, *operand);
                let operand_type = self.resolve_expr_type_inner(operand, visited)?;
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
                let table_type = self.resolve_expr_type_inner(table, visited)?;
                let table_indices: Vec<TableIndex> = match &table_type {
                    ValueType::Table(Some(idx)) => vec![*idx],
                    ValueType::Union(types) => types.iter().filter_map(|t| match t {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }).collect(),
                    _ => return None,
                };
                // Try each table in the union for the field, including parent classes
                let mut field_types: Vec<ValueType> = Vec::new();
                for &idx in &table_indices {
                    if let Some(fi) = self.get_field(idx, &field) {
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
                                && matches!(self.resolve_expr_type_inner(primary, visited), Some(ValueType::Nil));
                            let all_exprs: Vec<ExprId> = if skip_primary {
                                extras
                            } else {
                                std::iter::once(primary).chain(extras).collect()
                            };
                            for eid in all_exprs {
                                if let Some(vt) = self.resolve_expr_type_inner(eid, visited) {
                                    if !field_types.contains(&vt) {
                                        field_types.push(vt);
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    // Check parent classes
                    for &parent_idx in &self.table(idx).parent_classes {
                        if let Some(field_expr_id) = self.get_field(parent_idx, &field).map(|fi| fi.expr) {
                            if let Some(vt) = self.resolve_expr_type_inner(field_expr_id, visited) {
                                field_types.push(vt);
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
                let func_type = self.resolve_expr_type_inner(func, visited)?;
                let func_idx = match func_type {
                    ValueType::Function(Some(idx)) => idx,
                    ValueType::Table(Some(table_idx)) => {
                        self.table(table_idx).call_func?
                    }
                    _ => return None,
                };
                let func_info = self.func(func_idx);
                // Handle @return self
                if func_info.returns_self && ret_index == 0 {
                    if let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(func).clone() {
                        if let Some(rt) = self.resolve_expr_type_inner(receiver_expr, visited) {
                            return Some(rt);
                        }
                    }
                }
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_info.scope)?;
                self.sym(ret_sym_idx).versions.first()?.resolved_type.clone()
            }
            Expr::BracketIndex { table, .. } => {
                let table = *table;
                let table_type = self.resolve_expr_type_inner(table, visited)?;
                match &table_type {
                    ValueType::Table(Some(idx)) => self.table(*idx).value_type.clone(),
                    ValueType::Union(types) => {
                        let mut vts: Vec<ValueType> = Vec::new();
                        for t in types {
                            if let ValueType::Table(Some(idx)) = t {
                                if let Some(vt) = &self.table(*idx).value_type {
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
                        0 => Some(ValueType::String),
                        1 => {
                            self.ir.ext.addon_table_idx.map(|idx| ValueType::Table(Some(idx)))
                        }
                        _ => Some(ValueType::Nil),
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
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
            return text.clone();
        }
        if let Some(ref ann) = field_info.annotation {
            return self.format_type_depth(ann, depth + 1);
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
            ValueType::String => "string".to_string(),
            ValueType::Function(Some(func_idx)) => {
                let func = self.func(*func_idx);
                let args: Vec<String> = func.args.iter().enumerate().map(|(i, &sym_idx)| {
                    let name = match &self.sym(sym_idx).id {
                        SymbolIdentifier::Name(n) => n.clone(),
                        _ => "?".to_string(),
                    };
                    let optional = func.param_optional.get(i).copied().unwrap_or(false);
                    let suffix = if optional { "?" } else { "" };
                    let type_str = self.sym(sym_idx).versions.iter()
                        .find_map(|v| v.resolved_type.as_ref())
                        .map(|rt| {
                            let display_type = if optional { rt.strip_nil() } else { rt.clone() };
                            self.format_type_depth(&display_type, depth + 1)
                        });
                    match type_str {
                        Some(t) => format!("{}{}: {}", name, suffix, t),
                        None => format!("{}{}", name, suffix),
                    }
                }).collect();
                let mut all_args = args;
                if func.is_vararg {
                    all_args.push("...".to_string());
                }
                let rets: Vec<String> = func.rets.iter().map(|&sym_idx| {
                    match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                        Some(rt) => self.format_type_depth(rt, depth + 1),
                        None => "?".to_string(),
                    }
                }).collect();
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
                // Array/map types: table has value_type, no class_name, no named fields
                if table.class_name.is_none() && !has_fields {
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
            ValueType::TypeVariable(name) => name.clone(),
        }
    }

    pub(crate) fn scope_at_offset(&self, offset: rowan::TextSize) -> Option<ScopeIndex> {
        let mut best: Option<(rowan::TextRange, ScopeIndex)> = None;
        for &(range, scope_idx) in &self.ir.block_scopes {
            if range.contains(offset) {
                match best {
                    None => best = Some((range, scope_idx)),
                    Some((best_range, _)) if range.len() < best_range.len() => {
                        best = Some((range, scope_idx));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, idx)| idx)
    }

    pub fn signature_help_at(&self, offset: u32) -> Option<SignatureHelpResult> {
        let text_size = rowan::TextSize::from(offset);
        let token = self.root.token_at_offset(text_size).left_biased()?;

        // Walk up to find the enclosing FunctionCall node
        let call_node = token.parent_ancestors()
            .find(|n| n.kind() == SyntaxKind::FunctionCall)?;
        let call = FunctionCall::cast(call_node.clone())?;

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
        let args: Vec<(String, Option<String>)> = func.args.iter()
            .enumerate()
            .map(|(i, &sym_idx)| {
                let name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let optional = func.param_optional.get(i).copied().unwrap_or(false);
                let suffix = if optional { "?" } else { "" };
                let display_name = format!("{}{}", name, suffix);
                let type_str = self.sym(sym_idx).versions.iter()
                    .find_map(|v| v.resolved_type.as_ref())
                    .map(|rt| {
                        let display_type = if optional { rt.strip_nil() } else { rt.clone() };
                        self.format_type_depth(&display_type, 1)
                    });
                (display_name, type_str)
            })
            .filter(|(name, _)| !(skip_self && name == "self"))
            .collect();

        let rets: Vec<String> = func.rets.iter().map(|&sym_idx| {
            match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                Some(rt) => self.format_type_depth(rt, 1),
                None => "?".to_string(),
            }
        }).collect();

        let mut params: Vec<String> = args.iter().map(|(name, type_str)| {
            match type_str {
                Some(t) => format!("{}: {}", name, t),
                None => name.clone(),
            }
        }).collect();
        if func.is_vararg {
            params.push("...".to_string());
        }

        let label = if rets.is_empty() {
            format!("fun({})", params.join(", "))
        } else {
            format!("fun({}): {}", params.join(", "), rets.join(", "))
        };

        SignatureInfo { label, params, doc: func.doc.clone() }
    }

    fn build_overload_signature_info(&self, overload: &ResolvedOverload) -> SignatureInfo {
        let params: Vec<String> = overload.params.iter().map(|(name, vt)| {
            match vt {
                Some(vt) => format!("{}: {}", name, self.format_value_type_depth(vt, 1)),
                None => name.clone(),
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

        SignatureInfo { label, params, doc: None }
    }

    /// Check if a symbol is a function parameter.
    fn is_param_symbol(&self, symbol_idx: SymbolIndex) -> bool {
        if symbol_idx >= EXT_BASE {
            return false;
        }
        self.ir.functions.iter().any(|f| f.args.contains(&symbol_idx))
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
                let suffix = if optional { "?" } else { "" };
                let type_str = self.sym(sym_idx).versions.iter()
                    .find_map(|v| v.resolved_type.as_ref())
                    .map(|rt| {
                        let display_type = if optional { rt.strip_nil() } else { rt.clone() };
                        self.format_type_depth(&display_type, 1)
                    });
                match type_str {
                    Some(t) => format!("{}{}: {}", param_name, suffix, t),
                    None => format!("{}{}", param_name, suffix),
                }
            }).collect();
        let mut all_args = args;
        if func.is_vararg {
            all_args.push("...".to_string());
        }
        let rets: Vec<String> = func.rets.iter().map(|&sym_idx| {
            match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                Some(rt) => self.format_type_depth(rt, 1),
                None => "?".to_string(),
            }
        }).collect();
        let mut result = format!("function {}({})", name, all_args.join(", "));
        if !rets.is_empty() {
            result.push_str(&format!("\n  -> {}", rets.join(", ")));
        }
        // Append overloads
        if !func.overloads.is_empty() {
            for overload in &func.overloads {
                let ov_args: Vec<String> = overload.params.iter()
                    .filter(|(param_name, _)| !(skip_self && param_name == "self"))
                    .map(|(param_name, vt)| {
                        match vt {
                            Some(vt) => format!("{}: {}", param_name, self.format_value_type_depth(vt, 1)),
                            None => param_name.clone(),
                        }
                    }).collect();
                let ov_rets: Vec<String> = overload.returns.iter()
                    .map(|vt| self.format_value_type_depth(vt, 1))
                    .collect();
                let mut ov_line = format!("\nfunction {}({})", name, ov_args.join(", "));
                if !ov_rets.is_empty() {
                    ov_line.push_str(&format!("\n  -> {}", ov_rets.join(", ")));
                }
                result.push_str(&ov_line);
            }
        }
        result
    }

    fn format_overload(&self, overload: &ResolvedOverload) -> String {
        let args: Vec<String> = overload.params.iter().map(|(name, vt)| {
            match vt {
                Some(vt) => format!("{}: {}", name, self.format_value_type_depth(vt, 1)),
                None => name.clone(),
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
