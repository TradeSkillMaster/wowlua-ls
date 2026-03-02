use crate::types::*;
use super::Analysis;
use crate::diagnostics::WowDiagnostic;
use crate::syntax::SyntaxKind;
use crate::ast::{AstNode, FunctionCall};

// ── LSP Queries ──────────────────────────────────────────────────────────────

impl Analysis {
    pub(crate) fn find_symbol_at(&self, offset: u32) -> Option<(SymbolIndex, String)> {
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
        let name = token.text().to_string();
        let scope_idx = self.scope_at_offset(text_size)?;
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx)?;
        Some((symbol_idx, name))
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
        if let Some((symbol_idx, _)) = self.find_symbol_at(offset) {
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
        if let Some((_field_name, expr_id)) = self.find_field_at(offset) {
            return self.definition_for_expr(expr_id);
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
        if let Some((symbol_idx, name)) = self.find_symbol_at(offset) {
            let symbol = self.sym(symbol_idx);
            let resolved = symbol.versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            // Show narrowed type inside nil-guard scopes
            let display_type = self.narrow_type_for_display(resolved, symbol_idx, offset);
            let display_ref = display_type.as_ref().unwrap_or(resolved);
            let type_str = format!("{}: {}", name, self.format_type(display_ref));
            let doc = self.doc_for_type(display_ref);
            return Some(HoverResult { type_str, doc });
        }
        // Try field access (e.g. hovering over "new" in shash.new)
        let (field_name, expr_id) = self.find_field_at(offset)?;
        let resolved = self.resolve_expr_type(expr_id)?;
        let type_str = format!("{}: {}", field_name, self.format_type(&resolved));
        let doc = self.doc_for_type(&resolved);
        Some(HoverResult { type_str, doc })
    }

    fn narrow_type_for_display(&self, resolved: &ValueType, symbol_idx: SymbolIndex, offset: u32) -> Option<ValueType> {
        let scope_idx = self.scope_at_offset(rowan::TextSize::from(offset))?;
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
            let prefix_offset = offset - 2;
            let text_size = rowan::TextSize::from(prefix_offset);
            let token = self.root.token_at_offset(text_size).right_biased()?;
            if token.kind() != SyntaxKind::Name {
                return None;
            }

            // Find the Identifier parent and resolve the full chain
            let table_idx = if let Some(parent) = token.parent() {
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
                        if i > our_index { break; }
                        if i <= our_index && i < names.len() {
                            let name = names[i].text().to_string();
                            let field_expr_id = self.table(idx).fields.get(&name)?.expr;
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
            let mut items: Vec<CompletionItem> = table.fields.iter()
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
                        label: name.clone(),
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
            items.sort_by(|a, b| a.label.cmp(&b.label));
            if items.is_empty() { None } else { Some(items) }
        }
    }

    /// Resolve a dot/colon chain at offset, returning (owning_table_idx, field_name, field_expr_id).
    pub(crate) fn resolve_field_chain_at(&self, offset: u32) -> Option<(TableIndex, String, ExprId)> {
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
        if parent.kind() != SyntaxKind::Identifier {
            return None;
        }
        // Collect all Name tokens in the Identifier
        let names: Vec<_> = parent.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();
        if names.len() < 2 {
            return None;
        }
        let our_index = names.iter().position(|n| n.text_range() == token.text_range())?;
        if our_index == 0 {
            return None; // Root name is a symbol, handled by find_symbol_at
        }

        // Resolve chain: root symbol → table → field
        let root_name = names[0].text().to_string();
        let scope_idx = self.scope_at_offset(text_size)?;
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
        let ver = self.sym(symbol_idx).versions.last()?;
        let resolved = ver.resolved_type.as_ref()?;
        let mut table_idx = Self::extract_table_idx(resolved)?;

        // Walk intermediate fields
        for i in 1..our_index {
            let name = names[i].text().to_string();
            let field_expr_id = self.table(table_idx).fields.get(&name)?.expr;
            let field_type = self.resolve_expr_type(field_expr_id)?;
            table_idx = Self::extract_table_idx(&field_type)?;
        }

        let field_name = names[our_index].text().to_string();
        let field_expr_id = self.table(table_idx).fields.get(&field_name)?.expr;
        Some((table_idx, field_name, field_expr_id))
    }

    pub(crate) fn find_field_at(&self, offset: u32) -> Option<(String, ExprId)> {
        let (_, name, expr_id) = self.resolve_field_chain_at(offset)?;
        Some((name, expr_id))
    }

    /// Find all references to the symbol or field at the given offset.
    /// Returns a list of TextRanges covering each Name token that references the target.
    pub fn references_at(&self, offset: u32, include_declaration: bool) -> Option<Vec<rowan::TextRange>> {
        // Determine what we're looking for
        if let Some((symbol_idx, name)) = self.find_symbol_at(offset) {
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
        } else if let Some((table_idx, field_name, _)) = self.resolve_field_chain_at(offset) {
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
                    match self.table(cur_table).fields.get(&n) {
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
        if let Some((symbol_idx, _)) = self.find_symbol_at(offset) {
            if symbol_idx >= EXT_BASE {
                return None; // Cannot rename external symbols
            }
            return Some((token.text_range(), name));
        }
        // Try field
        if let Some((table_idx, _, _)) = self.resolve_field_chain_at(offset) {
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
        match self.expr(expr_id) {
            Expr::Literal(vt) => Some(vt.clone()),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                self.sym(*sym_idx).versions[*ver_idx].resolved_type.clone()
            }
            Expr::FunctionDef(func_idx) => {
                Some(ValueType::Function(Some(*func_idx)))
            }
            Expr::TableConstructor(table_idx) => {
                Some(ValueType::Table(Some(*table_idx)))
            }
            Expr::Grouped(inner) => self.resolve_expr_type(*inner),
            Expr::FieldAccess { table, field, .. } => {
                let table = *table;
                let field = field.clone();
                let table_type = self.resolve_expr_type(table)?;
                let idx = match &table_type {
                    ValueType::Table(Some(idx)) => *idx,
                    ValueType::Union(types) => {
                        *types.iter().find_map(|t| match t {
                            ValueType::Table(Some(idx)) => Some(idx),
                            _ => None,
                        })?
                    }
                    _ => return None,
                };
                let field_expr_id = self.table(idx).fields.get(&field)?.expr;
                self.resolve_expr_type(field_expr_id)
            }
            Expr::FunctionCall { func, ret_index, .. } => {
                let func = *func;
                let ret_index = *ret_index;
                let func_type = self.resolve_expr_type(func)?;
                let ValueType::Function(Some(func_idx)) = func_type else { return None };
                let func_info = self.func(func_idx);
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_info.scope)?;
                self.sym(ret_sym_idx).versions.first()?.resolved_type.clone()
            }
            _ => None,
        }
    }

    pub(crate) fn format_type(&self, vt: &ValueType) -> String {
        self.format_type_depth(vt, 0)
    }

    pub(crate) fn format_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        self.format_value_type_depth(vt, depth)
    }

    pub(crate) fn format_value_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        match vt {
            ValueType::Nil => "nil".to_string(),
            ValueType::Boolean(Some(true)) => "true".to_string(),
            ValueType::Boolean(Some(false)) => "false".to_string(),
            ValueType::Boolean(None) => "boolean".to_string(),
            ValueType::Number => "number".to_string(),
            ValueType::String => "string".to_string(),
            ValueType::Function(Some(func_idx)) => {
                let func = self.func(*func_idx);
                let args: Vec<String> = func.args.iter().map(|&sym_idx| {
                    let name = match &self.sym(sym_idx).id {
                        SymbolIdentifier::Name(n) => n.clone(),
                        _ => "?".to_string(),
                    };
                    let type_str = self.sym(sym_idx).versions.iter()
                        .find_map(|v| v.resolved_type.as_ref())
                        .map(|rt| self.format_type_depth(rt, depth + 1));
                    match type_str {
                        Some(t) => format!("{}: {}", name, t),
                        None => name,
                    }
                }).collect();
                let rets: Vec<String> = func.rets.iter().map(|&sym_idx| {
                    match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                        Some(rt) => self.format_type_depth(rt, depth + 1),
                        None => "?".to_string(),
                    }
                }).collect();
                let primary = if rets.is_empty() {
                    format!("fun({})", args.join(", "))
                } else {
                    format!("fun({}): {}", args.join(", "), rets.join(", "))
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
                if let Some(ref class_name) = table.class_name {
                    if table.fields.is_empty() || depth > 0 {
                        return class_name.clone();
                    }
                    let indent = "  ".repeat(depth + 1);
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        let type_str = self.resolve_expr_type(field_info.expr)
                            .map(|t| self.format_type_depth(&t, depth + 1))
                            .unwrap_or_else(|| "?".to_string());
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
                    fields.sort();
                    return format!("{} {{\n{}\n}}", class_name, fields.join(",\n"));
                }
                if table.fields.is_empty() || depth > 0 {
                    "table".to_string()
                } else {
                    let indent = "  ".repeat(depth + 1);
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        let type_str = self.resolve_expr_type(field_info.expr)
                            .map(|t| self.format_type_depth(&t, depth + 1))
                            .unwrap_or_else(|| "?".to_string());
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
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
                let field_expr = self.table(table_idx).fields.get(name)?.expr;
                let ft = self.resolve_expr_type(field_expr)?;
                table_idx = Self::extract_table_idx(&ft)?;
            }
            let method_name = &names[names.len() - 1];
            let field_expr = self.table(table_idx).fields.get(method_name)?.expr;
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
            .map(|&sym_idx| {
                let name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let type_str = self.sym(sym_idx).versions.iter()
                    .find_map(|v| v.resolved_type.as_ref())
                    .map(|rt| self.format_type_depth(rt, 1));
                (name, type_str)
            })
            .filter(|(name, _)| !(skip_self && name == "self"))
            .collect();

        let rets: Vec<String> = func.rets.iter().map(|&sym_idx| {
            match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                Some(rt) => self.format_type_depth(rt, 1),
                None => "?".to_string(),
            }
        }).collect();

        let params: Vec<String> = args.iter().map(|(name, type_str)| {
            match type_str {
                Some(t) => format!("{}: {}", name, t),
                None => name.clone(),
            }
        }).collect();

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
