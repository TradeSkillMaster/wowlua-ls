use super::*;

/// Context for an expression string argument at a given offset.
pub(super) struct ExpressionStringContext {
    /// Table indices whose fields are the expression's variables.
    table_idxs: Vec<TableIndex>,
    /// Byte offset in the file where the string content starts (after opening delimiter).
    content_start: u32,
    /// The raw expression string content (without delimiters).
    content: String,
}

impl AnalysisResult {
    pub(super) fn resolve_event_string_at<'a>(&'a self, tree: &'a SyntaxTree, offset: u32) -> Option<(&'a str, &'a str, &'a crate::pre_globals::EventPayload)> {
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

    pub(super) fn event_string_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        let (_, event_name, payload) = self.resolve_event_string_at(tree, offset)
            .or_else(|| self.resolve_event_string_in_comparison(tree, offset))?;
        let type_str = Self::format_event_payload(event_name, payload);
        Some(HoverResult { type_str, doc: payload.documentation.clone() })
    }

    pub(super) fn event_string_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        let result = self.resolve_event_string_at(tree, offset)
            .or_else(|| self.resolve_event_string_in_comparison(tree, offset));
        let (event_type_name, event_name, _) = result?;
        let loc = self.ir.ext.event_locations.get(event_type_name)?.get(event_name)?;
        Some(DefinitionResult::External(loc.clone()))
    }

    /// Resolve an event string in an equality comparison like `event == "ADDON_LOADED"`.
    pub(super) fn resolve_event_string_in_comparison<'a>(&'a self, tree: &'a SyntaxTree, offset: u32) -> Option<(&'a str, &'a str, &'a crate::pre_globals::EventPayload)> {
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

    pub(super) fn format_event_payload(event_name: &str, payload: &crate::pre_globals::EventPayload) -> String {
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

    /// Check whether the token at `offset` is a string literal passed to an
    /// `expression<C, R>` parameter, and return the context if so.
    pub(super) fn resolve_expression_context_at(&self, tree: &SyntaxTree, offset: u32) -> Option<ExpressionStringContext> {
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
    pub(super) fn expression_word_at(&self, ctx: &ExpressionStringContext, offset: u32) -> Option<(String, u32, u32)> {
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
    pub(super) fn expression_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
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
    pub(super) fn expression_completions_at(&self, tree: &SyntaxTree, offset: u32) -> Option<Vec<lsp_types::CompletionItem>> {
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
    pub(super) fn collect_expression_fields(&self, table_idx: TableIndex, seen: &mut HashSet<String>, out: &mut Vec<(String, String)>) {
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
    pub(super) fn expression_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
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
}
