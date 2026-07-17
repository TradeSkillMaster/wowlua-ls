use super::*;
use std::sync::LazyLock;

/// Cached diagnostic codes for `@diagnostic` completions.
static KNOWN_CODES: LazyLock<Vec<&'static str>> = LazyLock::new(crate::diagnostics::known_codes);

/// All Lua reserved keywords, used for keyword completions in scope context.
const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for",
    "function", "if", "in", "local", "nil", "not", "or", "repeat",
    "return", "then", "true", "until", "while",
];

pub(super) enum AnnotationContext {
    Function,
    Class,
    Any,
}

/// Strip an optional `private`/`protected`/`public` visibility prefix from an
/// annotation body (e.g. the text after `@field`). Returns the remainder trimmed
/// past the visibility keyword, or the original slice unchanged.
fn strip_optional_visibility(s: &str) -> &str {
    for prefix in ["private", "protected", "public"] {
        if let Some(rest) = s.strip_prefix(prefix)
            && rest.starts_with(char::is_whitespace)
        {
            return rest.trim_start();
        }
    }
    s
}

/// Walk tokens in one direction from `start`, collecting `@field` names from
/// annotation comments. Stops at a blank line or a non-comment token.
fn collect_field_names_in_direction(start: Option<SyntaxToken>, forward: bool) -> Vec<String> {
    let mut names = Vec::new();
    let mut tok = start;
    let mut prev_was_newline = false;
    while let Some(t) = tok {
        let kind = t.kind();
        if kind == SyntaxKind::Newline {
            if prev_was_newline {
                break;
            }
            prev_was_newline = true;
            tok = if forward { t.next_token() } else { t.prev_token() };
            continue;
        }
        prev_was_newline = false;
        if kind == SyntaxKind::Whitespace {
            tok = if forward { t.next_token() } else { t.prev_token() };
            continue;
        }
        if kind == SyntaxKind::Comment {
            if let Some(name) = extract_field_name_from_annotation(t.text()) {
                names.push(name);
            }
            tok = if forward { t.next_token() } else { t.prev_token() };
            continue;
        }
        break;
    }
    names
}

/// Extract a field name from a `---@field [visibility] name type` annotation comment.
fn extract_field_name_from_annotation(text: &str) -> Option<String> {
    let content = text.strip_prefix("---@field")?;
    if !content.starts_with(' ') && !content.starts_with('\t') {
        return None;
    }
    let content = strip_optional_visibility(content.trim_start());
    let name = content
        .split(|c: char| c.is_whitespace() || c == '?')
        .next()
        .unwrap_or("");
    if name.is_empty() { None } else { Some(name.to_string()) }
}

/// Given the type-expression text preceding the cursor, return the trailing
/// partial type name being typed — the run of identifier characters after the
/// last structural delimiter of a type expression (union `|`, intersection `&`,
/// generic `<` / `,`, function/table shape `(` `{` `:`, whitespace, and the
/// matching closers). `.` is kept so namespaced type names still complete.
///
/// e.g. `table<Theme` → `Theme`, `table<string, Th` → `Th`, `A|B` → `B`.
fn trailing_type_ident(region: &str) -> &str {
    let mut start = region.len();
    for (i, c) in region.char_indices().rev() {
        if c.is_alphanumeric() || c == '_' || c == '.' {
            start = i;
        } else {
            break;
        }
    }
    &region[start..]
}

pub(super) fn collect_type_name_completions<'a>(
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

/// Whether snippet completions are offered at all (gated by client support +
/// `completion.snippets`). Independent of [`CallSnippets`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Snippets {
    Enabled,
    Disabled,
}

impl Snippets {
    pub fn from_enabled(enabled: bool) -> Self {
        if enabled { Self::Enabled } else { Self::Disabled }
    }
    fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

/// Whether a function-call completion auto-fills its parameter placeholders
/// (`foo(${1:a})`) vs. inserting just the name (gated by client support +
/// `completion.callSnippets`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CallSnippets {
    Enabled,
    Disabled,
}

impl CallSnippets {
    pub fn from_enabled(enabled: bool) -> Self {
        if enabled { Self::Enabled } else { Self::Disabled }
    }
    fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl AnalysisResult {
    /// [`Snippets`] gates all snippet completions (requires client support).
    /// [`CallSnippets`] additionally gates only the function-call parameter
    /// auto-fill (`foo(${1:a})`); when [`CallSnippets::Disabled`], function
    /// completions insert just the name while annotation-tag snippets still
    /// honor [`Snippets`].
    pub fn completions_at(&self, tree: &SyntaxTree, offset: u32, source: &str, snippets: Snippets, call_snippets: CallSnippets) -> Option<Vec<lsp_types::CompletionItem>> {
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

        // Inside a string literal but no value completions applied above: do NOT
        // fall through to scope/member completion. Offering identifier names (local
        // variables, WoW globals, keywords) inside a string is never correct — it's
        // what produced spurious `PLAY`/`PLAYER`-style suggestions inside a string
        // argument whose type carried no completable literals.
        if Self::offset_inside_string_literal(tree, offset) {
            return None;
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

                        if let Some(result) = self.annotation_completions(prefix, &tok, snippets.enabled()) {
                            return Some(result);
                        }
                    }
                }
        }

        // Suppress function-call snippets when a '(' already follows the cursor.
        // This handles swapping one function name for another in an existing call —
        // inserting parens+params would duplicate the existing ones.
        let paren_follows = source.get(offset as usize..)
            .is_some_and(|rest| rest.bytes()
                .find(|&b| b != b' ' && b != b'\t') == Some(b'('));
        let call_snippets = if paren_follows { CallSnippets::Disabled } else { call_snippets };

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
            self.complete_member_access(tree, source, member_offset, &member_prefix_lower, call_snippets.enabled())
        } else {
            self.complete_scope(tree, source, offset, call_snippets.enabled())
        }
    }

    /// Whether the cursor at `offset` sits in a string-literal token, mirroring
    /// exactly how [`string_literal_completions`](Self::string_literal_completions)
    /// locates the string (left-biased token at `offset - 1`). Used to suppress
    /// identifier completion inside string literals: when no string-value
    /// completion applied, the caller must return nothing rather than fall through
    /// to scope/member completion — the two must agree on "in a string" so plain
    /// strings don't leak scope symbols where enum strings offer value completions.
    fn offset_inside_string_literal(tree: &SyntaxTree, offset: u32) -> bool {
        if offset == 0 {
            return false;
        }
        let text_size = TextSize::from(offset - 1);
        SyntaxNode::new_root(tree)
            .token_at_offset(text_size)
            .left_biased()
            .is_some_and(|token| token.kind() == SyntaxKind::String)
    }

    /// Dot/colon member completion: resolve the receiver to a table and enumerate its fields.
    fn complete_member_access(&self, tree: &SyntaxTree, source: &str, offset: u32, member_prefix_lower: &str, call_snippets: bool) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        // Dot/colon completion: resolve the prefix to a table, enumerate fields
        if offset < 2 { return None; }
        let prev_char = source.as_bytes()[(offset - 1) as usize];
        let prefix_offset = offset - 2;
        let text_size = TextSize::from(prefix_offset);
        let mut token = SyntaxNode::new_root(tree).token_at_offset(text_size).right_biased()?;

        // Skip whitespace/newline/comment tokens backwards for multi-line chains like:
        //   func(args)
        //       -- :commentedOut()
        //       :method()
        while matches!(token.kind(), SyntaxKind::Whitespace | SyntaxKind::Newline | SyntaxKind::Comment) {
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
        } else if token.kind() == SyntaxKind::RightSquareBracket {
            // x[i]. or x[i]:  — the token before the separator is ']', the close
            // of a bracket-index expression. Resolve the `BracketAccess` node to
            // its element type (e.g. `x ---@type Foo[]` makes `x[1]` a `Foo`).
            let bracket_node = token.parent()
                .filter(|p| p.kind() == SyntaxKind::BracketAccess)?;
            Some(self.resolve_identifier_to_table(&bracket_node, text_size)?)
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
        // When the receiver is a simple local/global whose type is an
        // `Intersection` carrying an inline `TableShape` (cross-file injected
        // fields, e.g. `dropdown: Frame & { DropDown: ... }`), surface those
        // shape fields too. Guarded by matching the resolved `table_idx` so a
        // mis-resolved receiver (dotted chain reusing a local name) can't leak
        // unrelated completions.
        let receiver_shape_type: Option<ValueType> = if token.kind() == SyntaxKind::Name {
            self.scope_at_offset(text_size)
                .and_then(|s| self.get_symbol(&SymbolIdentifier::Name(token.text().to_string()), s))
                .and_then(|si| self.sym(si).versions.last().and_then(|v| v.resolved_type.clone()))
                .filter(|t| Self::extract_table_idx(t) == Some(table_idx))
        } else {
            None
        };
        // Determine enclosing class for visibility filtering
        let enclosing_class = {
            let node = SyntaxNode::new_root(tree).token_at_offset(text_size)
                .right_biased()
                .and_then(|t| t.parent());
            node.and_then(|n| self.find_enclosing_class(&n))
        };
        // Mirror access.rs: if the receiver is a defclass-created instance in
        // this file, protected fields are accessible at file scope (the module
        // file is the class's own implementation). Without this, inherited
        // `@protected` methods (e.g. a module's `OnModuleLoad`) are wrongly
        // hidden from completion even though calling them is allowed.
        let receiver_is_own_defclass = self.defclass_vars
            .get(token.text())
            .is_some_and(|&dc_table| self.is_subclass_of(dc_table, table_idx));
        // _G global-environment redirect: show all globals as completions
        if self.ir.is_global_env(table_idx) {
            return Some(self.complete_global_env_members(offset, member_prefix_lower, is_colon, call_snippets));
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
                            receiver_is_own_defclass
                                || enclosing_class.is_some_and(|ec| self.is_subclass_of(ec, table_idx))
                        }
                        crate::annotations::Visibility::Public => true,
                    };
                    if !accessible { return None; }
                }
                // Filter by typed prefix (e.g. "Regis" in `frame:Regis`)
                if !member_prefix_lower.is_empty()
                    && !name.to_ascii_lowercase().starts_with(member_prefix_lower)
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
                let (insert_text, insert_text_format) = if call_snippets {
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
                    data: Some(serde_json::json!({"member": true, "offset": offset, (DATA_REPLACE_START): offset})),
                    ..CompletionItem::default()
                })
            })
            .collect();
        // Append inline `TableShape` fields carried by the receiver type.
        if let Some(rt) = &receiver_shape_type {
            let existing: HashSet<String> = items.iter().map(|i| i.label.clone()).collect();
            let mut names: Vec<String> = Vec::new();
            rt.collect_shape_field_names(&mut names);
            for name in names {
                if existing.contains(&name) {
                    continue;
                }
                if !member_prefix_lower.is_empty()
                    && !name.to_ascii_lowercase().starts_with(member_prefix_lower)
                {
                    continue;
                }
                let mut tys: Vec<ValueType> = Vec::new();
                rt.collect_shape_field_types(&name, &mut tys);
                let is_func = matches!(
                    tys.first(),
                    Some(ValueType::Function(_) | ValueType::FunctionSig(_))
                );
                if is_colon && !is_func {
                    continue;
                }
                let kind = if is_func {
                    CompletionItemKind::METHOD
                } else {
                    CompletionItemKind::FIELD
                };
                let sort_text = if name.starts_with('_') {
                    format!("1{}", name)
                } else {
                    format!("0{}", name)
                };
                let detail = if tys.is_empty() {
                    None
                } else {
                    Some(self.format_type(&ValueType::make_union(tys)))
                };
                items.push(CompletionItem {
                    label: name,
                    kind: Some(kind),
                    detail,
                    sort_text: Some(sort_text),
                    data: Some(serde_json::json!({"member": true, "offset": offset, (DATA_REPLACE_START): offset})),
                    ..CompletionItem::default()
                });
            }
        }
        items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
        Some(items)
    }

    /// Build `_G` global-environment member completions (all globals as members of `_G`).
    fn complete_global_env_members(&self, offset: u32, member_prefix_lower: &str, is_colon: bool, call_snippets: bool) -> Vec<lsp_types::CompletionItem> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        let mut items: Vec<CompletionItem> = Vec::new();
        let mut seen = HashSet::new();
        // Collect from local scope0 and external scope0_symbols
        let scope0_iter = self.ir.scope0_local_symbols()
            .map(|(id, idx)| (id.clone(), idx));
        let ext_iter = self.ir.ext.scope0_symbols.iter()
            .map(|(id, &idx)| (id.clone(), idx));
        for (id, sym_idx) in scope0_iter.chain(ext_iter) {
            if let SymbolIdentifier::Name(name) = &id {
                if !seen.insert(name.clone()) { continue; }
                if !member_prefix_lower.is_empty()
                    && !name.to_ascii_lowercase().starts_with(member_prefix_lower)
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
                let (insert_text, insert_text_format) = if call_snippets {
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
                    data: Some(serde_json::json!({"member": true, "offset": offset, (DATA_REPLACE_START): offset})),
                    ..CompletionItem::default()
                });
            }
        }
        items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
        items
    }

    /// Scope/global completion: walk the scope chain, external globals, and Lua keywords.
    fn complete_scope(&self, tree: &SyntaxTree, source: &str, offset: u32, call_snippets: bool) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        // Block ranges are end-exclusive, so use offset-1 to stay inside the enclosing block.
        let scope_lookup = TextSize::from(offset.saturating_sub(1));

        // Suppress completions when the cursor is on a keyword token (e.g. "then", "end", "do").
        if let Some(tok) = SyntaxNode::new_root(tree).token_at_offset(scope_lookup).left_biased()
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

        let scope_idx = self.scope_at_offset(scope_lookup)?;

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

        // Cursor's enclosing block start — if it's past a def_node's start,
        // the cursor is in the body (keep the symbol), not the header (skip it).
        let cursor_block_start = SyntaxNode::new_root(tree)
            .token_at_offset(scope_lookup)
            .left_biased()
            .and_then(|t| t.parent())
            .and_then(|n| n.ancestors().find(|a| a.kind() == SyntaxKind::Block))
            .map(|b| u32::from(b.text_range().start()));

        let mut seen = HashSet::new();
        let mut items = Vec::new();
        let mut current_scope = Some(scope_idx);
        while let Some(si) = current_scope {
            let scope = self.scope(si);
            for (id, &sym_idx) in &scope.symbols {
                if let SymbolIdentifier::Name(name) = id
                    && seen.insert(name.clone()) {
                        if has_prefix && !name.to_ascii_lowercase().starts_with(&prefix_lower) {
                            continue;
                        }
                        let sym = self.sym(sym_idx);
                        if sym.versions.iter().any(|v| {
                            let d = &v.def_node;
                            offset >= d.start && offset < d.end
                                && cursor_block_start.is_none_or(|bs| bs <= d.start)
                        }) {
                            continue;
                        }
                        let resolved = sym.versions.iter().rev()
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
                        let (insert_text, insert_text_format) = if call_snippets {
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
                        // Sort-text prefixes "2"/"3" identify external globals;
                        // main_loop.rs depends on this to set isIncomplete.
                        let sort_text = if name.starts_with('_') {
                            format!("3{}", name)
                        } else {
                            format!("2{}", name)
                        };
                        let (insert_text, insert_text_format) = if call_snippets {
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
    pub(super) fn detect_keyword_only_position(tree: &SyntaxTree, prefix_start: usize) -> Option<&'static str> {
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
    pub(super) fn constructor_field_completions(&self, tree: &SyntaxTree, offset: u32, source: &str) -> Option<Vec<lsp_types::CompletionItem>> {
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
        let ctor_table = self.table(ctor_idx);
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
    pub(super) fn build_func_call_snippet(&self, label: &str, func_idx: crate::types::FunctionIndex, skip_self: bool) -> Option<String> {
        let func = self.func(func_idx);
        let self_offset = if skip_self && func.args.first()
            .map(|&sym_idx| self.sym(sym_idx).id == crate::types::SymbolIdentifier::Name("self".into()))
            .unwrap_or(false)
        { 1 } else { 0 };
        // Zip args with their optionality flags so filter_map keeps them aligned
        let mut params: Vec<(String, bool)> = func.args[self_offset..].iter()
            .zip(&func.param_optional[self_offset..])
            .filter_map(|(&sym_idx, &opt)| {
                if let crate::types::SymbolIdentifier::Name(n) = &self.sym(sym_idx).id {
                    Some((n.clone(), opt))
                } else {
                    None
                }
            })
            .collect();
        // Trim trailing optional parameters from the snippet
        while params.last().is_some_and(|(_, opt)| *opt) {
            params.pop();
        }
        let param_names: Vec<String> = params.into_iter().map(|(n, _)| n).collect();
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
    pub(super) fn resolve_member_detail(&self, tree: &SyntaxTree, offset: u32, field_name: &str) -> Option<String> {
        if offset < 1 { return None; }
        // Start just before the trigger character to land on the receiver token
        let prefix_offset = offset - 1;
        let text_size = TextSize::from(prefix_offset);
        let mut token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;

        while matches!(token.kind(), SyntaxKind::Whitespace | SyntaxKind::Newline | SyntaxKind::Comment) {
            token = token.prev_token()?;
        }

        let table_idx = if token.kind() == SyntaxKind::RightBracket {
            let funcall_node = token.parent().filter(|p| p.kind() == SyntaxKind::ArgumentList)
                .and_then(|al| al.parent())
                .filter(|p| p.kind() == SyntaxKind::FunctionCall || p.kind() == SyntaxKind::MethodCall)?;
            self.resolve_funcall_node_to_table(&funcall_node, text_size)?
        } else if token.kind() == SyntaxKind::RightSquareBracket {
            // x[i]. or x[i]:  — resolve the bracket-index expression's element type.
            let bracket_node = token.parent().filter(|p| p.kind() == SyntaxKind::BracketAccess)?;
            self.resolve_identifier_to_table(&bracket_node, text_size)?
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

    pub(super) fn string_literal_completions(
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
            .or_else(|| self.string_context_type_from_call_arg(&token))
            .or_else(|| self.string_context_callback_event_type(&token, tree));

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
    pub(super) fn string_context_type_from_binary(
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
    /// Whether the workspace declared any callback registries — a cheap guard so the
    /// `unknown-callback-event` diagnostic can skip token iteration entirely.
    pub fn has_callback_registries(&self) -> bool {
        !self.ir.ext.callback_registries.is_empty()
    }

    /// If `token` is a string literal at the event-name argument of a
    /// callback-registry consumer method (`@callback-event-arg`), and the call's
    /// receiver canonicalizes to a known registry, return that registry's event set.
    /// Purely syntactic: the receiver (`addonTable.CallbackRegistry`) usually types as
    /// bare `table`, so call resolution can't be relied on — the receiver path,
    /// method, and argument index are read straight from the AST, and the receiver is
    /// canonicalized with the same addon-namespace rewrite used at scan time. Shared by
    /// event-name completion and the `unknown-callback-event` diagnostic.
    pub fn callback_event_set_for_string<'a>(
        &'a self,
        token: &SyntaxToken,
        tree: &SyntaxTree,
    ) -> Option<&'a crate::pre_globals::CallbackEventSet> {
        if self.ir.ext.callback_event_methods.is_empty() || self.ir.ext.callback_registries.is_empty() {
            return None;
        }
        let call_node = token.ancestors()
            .find(|n| matches!(n.kind(), SyntaxKind::FunctionCall | SyntaxKind::MethodCall))?;
        let call = FunctionCall::cast(call_node)?;
        let ident = call.identifier()?;
        if !ident.is_call_to_self() {
            return None;
        }
        let names = ident.names();
        if names.len() < 2 {
            return None;
        }
        let event_arg = *self.ir.ext.callback_event_methods.get(names.last().unwrap())?;
        // Locate the string's 1-based position within the call's argument list.
        let arg_list = call_node.children().find(|n| n.kind() == SyntaxKind::ArgumentList)?;
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
        if arg_index + 1 != event_arg {
            return None;
        }
        let ns = crate::annotations::detect_addon_ns_var(SyntaxNode::new_root(tree));
        let path = crate::annotations::canonicalize_member_path(&names[..names.len() - 1], ns.as_deref())?;
        // Scope addon-namespace paths by this file's addon identity, matching scan time.
        let path = crate::annotations::scope_addon_ns_path(path, self.ir.addon_folder_name.as_deref());
        self.ir.ext.callback_registries.get(&path)
    }

    /// Event-name completions inside a callback consumer's string argument.
    fn string_context_callback_event_type(
        &self,
        token: &SyntaxToken,
        tree: &SyntaxTree,
    ) -> Option<ValueType> {
        let set = self.callback_event_set_for_string(token, tree)?;
        if set.events.is_empty() {
            return None;
        }
        let mut events: Vec<&str> = set.events.iter().map(String::as_str).collect();
        events.sort_unstable();
        Some(ValueType::Union(
            events.into_iter().map(|s| ValueType::String(Some(s.to_owned()))).collect(),
        ))
    }

    pub(super) fn string_context_type_from_call_arg(
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
            // Open string-enum alias (e.g. `UnitToken`): the resolved type collapsed
            // to bare `string`, but the alias's `---|"literal"` completion values are
            // preserved in `alias_string_literals`.
            if let Some(vt) = self.string_enum_alias_literal_type(ann) {
                return Some(vt);
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
                if let Some(ref_name) = keyof_target
                    && let Some(table_idx) = call_res.resolve_keyof_target(&ref_name)
                {
                    let fields = crate::analysis::collect_class_fields_impl(
                        &self.ir, &self.resolved_expr_cache, table_idx,
                    );
                    let mut names: Vec<&str> = fields.iter().map(|(n, _, _)| n.as_str()).collect();
                    names.sort_unstable();
                    let types = names.into_iter()
                        .map(|s| ValueType::String(Some(s.to_owned()))).collect();
                    return Some(ValueType::Union(types));
                }
            }

        // Backtick generic `T`: offer class names scoped to T's constraint.
        if let Some(ann) = func.param_annotations.get(param_index)
            && let Some(gen_name) = Self::backtick_generic_name(ann)
            && let Some((_, constraint)) = func.generic_constraints_raw.iter()
                .find(|(n, _)| n == gen_name)
        {
            let names = self.class_names_for_generic_constraint(constraint.as_deref());
            if !names.is_empty() {
                let types = names.into_iter()
                    .map(|s| ValueType::String(Some(s))).collect();
                return Some(ValueType::Union(types));
            }
        }

        None
    }

    /// If `ann` names an "open" string-enum alias (`@alias UnitToken string`
    /// followed by `---|"player"` continuation lines), return a union of its
    /// string-literal completion suggestions. The alias's resolved type is bare
    /// `string` (the literals were collapsed away by `make_union`), so they are
    /// recovered from `alias_string_literals` — which holds both stub aliases and
    /// file-local ones. Searches through `nil`-optional unions and `!`/backtick
    /// wrappers to find the aliased name.
    fn string_enum_alias_literal_type(&self, ann: &crate::annotations::AnnotationType) -> Option<ValueType> {
        use crate::annotations::AnnotationType;
        match ann {
            AnnotationType::Simple(name) => {
                let literals = self.ir.alias_string_literals.get(name)?;
                if literals.is_empty() {
                    return None;
                }
                Some(ValueType::Union(
                    literals.iter().map(|s| ValueType::String(Some(s.clone()))).collect(),
                ))
            }
            AnnotationType::NonNil(inner) | AnnotationType::Backtick(inner) => {
                self.string_enum_alias_literal_type(inner)
            }
            AnnotationType::Union(parts) => {
                parts.iter().find_map(|p| self.string_enum_alias_literal_type(p))
            }
            _ => None,
        }
    }

    /// Extract the generic type-parameter name from a backtick annotation
    /// (`@param p `T``), searching inside unions and non-nil wrappers.
    pub(super) fn backtick_generic_name(ann: &crate::annotations::AnnotationType) -> Option<&str> {
        use crate::annotations::AnnotationType;
        match ann {
            AnnotationType::Backtick(inner) => match inner.as_ref() {
                AnnotationType::Simple(name) => Some(name.as_str()),
                _ => None,
            },
            AnnotationType::Union(members) => members.iter().find_map(Self::backtick_generic_name),
            AnnotationType::NonNil(inner) => Self::backtick_generic_name(inner),
            _ => None,
        }
    }

    /// Collect the names of all classes whose type satisfies `constraint` — i.e.
    /// classes equal to or inheriting from the constraint class. When `constraint`
    /// is `None` (an unconstrained generic), every known class name is returned.
    /// Used to drive string-literal completions for backtick-name generic params.
    ///
    /// Returns an empty `Vec` when a constraint is given but does not resolve to a
    /// class (e.g. a primitive or an undefined name), so the caller falls through.
    fn class_names_for_generic_constraint(&self, constraint: Option<&str>) -> Vec<String> {
        let bound = match constraint {
            Some(c) => match self.resolve_class_constraint(c) {
                Some(ValueType::Table(Some(idx))) => Some(idx),
                // Constraint specified but not a class — can't scope, bail.
                _ => return Vec::new(),
            },
            None => None,
        };

        let mut seen = HashSet::new();
        let mut names = Vec::new();
        for (name, &idx) in self.ir.classes.iter().chain(self.ir.ext.classes.iter()) {
            if let Some(bound_idx) = bound
                && !self.is_subclass_of(idx, bound_idx)
            {
                continue;
            }
            if seen.insert(name) {
                names.push(name.clone());
            }
        }
        names.sort_unstable();
        names
    }

    pub(super) fn resolve_type_of_expression_node(
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
        if let Some((_, _, expr_id, _, _)) = self.resolve_field_chain_at(tree, name_offset) {
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

    pub(super) fn collect_string_literals(vt: &ValueType) -> Vec<String> {
        let mut result = Vec::new();
        Self::collect_string_literals_inner(vt, &mut result);
        result
    }

    pub(super) fn collect_string_literals_inner(vt: &ValueType, out: &mut Vec<String>) {
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

    pub(super) fn annotation_completions(
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
        if let Some(items) = self.try_cast_name_completions(after_at, token) {
            return Some(items);
        }
        if let Some(items) = self.try_correlated_field_completions(after_at, token) {
            return Some(items);
        }
        if let Some(items) = Self::try_diagnostic_code_completions(after_at) {
            return Some(items);
        }
        if let Some(items) = self.try_type_completions(after_at) {
            return Some(items);
        }

        // Inside a ---@ annotation — never fall back to general scope completions.
        Some(Vec::new())
    }

    pub(super) fn try_tag_completions(&self, after_at: &str, token: &SyntaxToken, snippets: bool) -> Option<Vec<lsp_types::CompletionItem>> {
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
            ("cast",           "Cast a variable's type",                  F|S,     Some("cast ${1:name} ${2:type}")),
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
            ("returns-class-name", "Return value names receiver's class (narrows on ==)", F, None),
            ("flavor-narrows", "Flavor guard that narrows WoW API availability", F,  None),
            ("narrows-arg",    "In-place argument type narrowing",        F,         Some("narrows-arg ${1:N}")),
            ("creates-global", "Call creates a named global (e.g. CreateFrame)", F,   Some("creates-global ${1:N}")),
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

    pub(super) fn detect_annotation_context(&self, token: &SyntaxToken) -> AnnotationContext {
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
                            | "built-extends" | "type-narrows" | "returns-class-name" | "defclass" | "flavor-narrows"
                            | "narrows-arg" | "creates-global" | "requires" => {
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
    pub(super) fn is_annotation_block_above_function(&self, token: &SyntaxToken) -> bool {
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

    pub(super) fn try_param_name_completions(
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

    /// Offer local variable name completions after `@cast `.
    pub(super) fn try_cast_name_completions(
        &self,
        after_at: &str,
        token: &SyntaxToken,
    ) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        let rest = after_at.strip_prefix("cast")?;
        if !rest.starts_with(' ') && !rest.starts_with('\t') {
            return None;
        }
        let after_cast = rest.trim_start();

        // If there's already a space after the name, cursor is in type position — let
        // try_type_completions handle it.
        if after_cast.contains(' ') || after_cast.contains('\t') {
            return None;
        }

        let partial_name = after_cast;

        // Comments may land outside block_scopes ranges because the tree builder
        // excludes trivia from node range calculations. Walk backward to the previous
        // non-trivia token and use its start (safely inside the block scope).
        let scope_idx = {
            let mut tok = token.prev_token();
            let mut found = None;
            while let Some(t) = tok {
                match t.kind() {
                    SyntaxKind::Comment | SyntaxKind::Whitespace | SyntaxKind::Newline => {
                        tok = t.prev_token();
                    }
                    _ => {
                        found = Some(t.text_range().start());
                        break;
                    }
                }
            }
            let off = found.unwrap_or(token.text_range().start());
            self.scope_at_offset(off)?
        };

        // Walk the scope chain but skip scope 0 (globals) — @cast targets local variables.
        let mut seen: HashSet<&String> = HashSet::new();
        let mut items = Vec::new();
        let mut current_scope = Some(scope_idx);
        while let Some(si) = current_scope {
            if si.val() == 0 {
                break;
            }
            let scope = self.scope(si);
            for id in scope.symbols.keys() {
                if let SymbolIdentifier::Name(name) = id
                    && name.starts_with(partial_name) && seen.insert(name)
                {
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        ..CompletionItem::default()
                    });
                }
            }
            current_scope = scope.parent;
        }

        if items.is_empty() { None } else { Some(items) }
    }

    /// Offer field name completions after `@correlated `.
    /// Scans the annotation block for `@field` declarations and offers their names.
    pub(super) fn try_correlated_field_completions(
        &self,
        after_at: &str,
        token: &SyntaxToken,
    ) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        let rest = after_at.strip_prefix("correlated")?;
        if !rest.starts_with(' ') && !rest.starts_with('\t') {
            return None;
        }
        let after_correlated = rest.trim_start();

        // Parse already-listed fields (comma-separated) and extract the partial prefix
        // being typed (the part after the last comma).
        let parts: Vec<&str> = after_correlated.split(',').collect();
        let partial = parts.last().map(|s| s.trim()).unwrap_or("");
        let already_listed: HashSet<&str> = parts[..parts.len().saturating_sub(1)]
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let mut field_names = collect_field_names_in_direction(token.prev_token(), false);
        field_names.extend(collect_field_names_in_direction(token.next_token(), true));

        let mut seen = HashSet::new();
        let items: Vec<CompletionItem> = field_names
            .iter()
            .filter(|name| {
                name.starts_with(partial)
                    && !already_listed.contains(name.as_str())
                    && seen.insert((*name).clone())
            })
            .map(|name| CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::FIELD),
                ..CompletionItem::default()
            })
            .collect();

        if items.is_empty() { None } else { Some(items) }
    }

    /// Offer diagnostic code completions after `@diagnostic enable:` or `@diagnostic disable:`.
    fn try_diagnostic_code_completions(after_at: &str) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        let rest = after_at.strip_prefix("diagnostic")?;
        if !rest.starts_with(' ') && !rest.starts_with('\t') {
            return None;
        }
        let rest = rest.trim_start();

        // Must have enable/disable/disable-next-line followed by ':'
        // Check disable-next-line before disable to avoid partial match.
        let rest = rest.strip_prefix("enable")
            .or_else(|| rest.strip_prefix("disable-next-line"))
            .or_else(|| rest.strip_prefix("disable"))?;
        let rest = rest.strip_prefix(':')?;
        let rest = rest.trim_start();

        // Handle comma-separated codes: `@diagnostic disable: code1, co`
        // Split to find already-listed codes and the current partial prefix.
        let parts: Vec<&str> = rest.split(',').collect();
        let partial = parts.last().map(|s| s.trim()).unwrap_or("");
        let already_listed: HashSet<&str> = parts[..parts.len().saturating_sub(1)]
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let items: Vec<CompletionItem> = KNOWN_CODES
            .iter()
            .filter(|code| code.starts_with(partial) && !already_listed.contains(**code))
            .map(|code| CompletionItem {
                label: code.to_string(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                ..CompletionItem::default()
            })
            .collect();

        if items.is_empty() { None } else { Some(items) }
    }


    pub(super) fn find_function_params_below(
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
    pub(super) fn find_function_def_start_below(&self, comment_token: &SyntaxToken) -> Option<u32> {
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
    pub(super) fn annotation_block_has_function_tags(&self, token: &SyntaxToken) -> bool {
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
    pub(super) fn try_generate_annotations_completion(
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

    pub(super) fn try_type_completions(&self, after_at: &str) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        let type_region = self.extract_type_prefix_from_annotation(after_at)?;

        // The type region can be a compound type expression: unions (`A|B`),
        // generics (`table<K, V>`), intersections (`A & B`), or function/table
        // shapes. Completion targets the single type name being typed at the
        // cursor, so take the trailing run of type-name characters after the last
        // structural delimiter (`|`, `<`, `,`, `&`, `(`, `{`, `:`, whitespace…).
        let type_prefix = trailing_type_ident(type_region);

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

    pub(super) fn extract_type_prefix_from_annotation<'b>(&self, after_at: &'b str) -> Option<&'b str> {
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
                let rest = strip_optional_visibility(rest.trim_start());
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

        // @class ClassName: PARENT_TYPE
        if let Some(rest) = after_at.strip_prefix("class") {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let rest = rest.trim_start();
                // Skip optional (partial)/(exact) prefix
                let rest = if let Some(r) = rest.strip_prefix("(partial)") {
                    r.trim_start()
                } else if let Some(r) = rest.strip_prefix("(exact)") {
                    r.trim_start()
                } else {
                    rest
                };
                if let Some(colon_pos) = rest.find(':') {
                    let after_colon = rest[colon_pos + 1..].trim_start();
                    // Handle multiple parents — take after last comma
                    let after_last_comma = after_colon.rsplit(',').next().unwrap_or(after_colon).trim();
                    return Some(after_last_comma);
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
}
