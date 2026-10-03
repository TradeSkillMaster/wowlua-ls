use super::*;

impl AnalysisResult {
    /// Return all reference ranges for a file-local symbol at `offset`, including
    /// the declaration. Returns `None` for external symbols, fields, and scope-0
    /// globals that have cross-file counterparts (those should use full rename).
    pub fn linked_editing_ranges_at(&self, tree: &SyntaxTree, offset: u32) -> Option<Vec<TextRange>> {
        let (symbol_idx, name, token_start) = self.find_symbol_at(tree, offset)?;
        // `find_symbol_at` also binds a field name (`t.key`) or constructor key
        // (`{ key = 1 }`) that merely shares a local's name.
        let token = SyntaxNode::new_root(tree).token_at_offset(TextSize::from(token_start)).right_biased()?;
        if symbol_idx.is_external()
            || Self::is_field_position(tree, token_start)
            || Self::is_constructor_key(&token)
        {
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

    /// Validate that the symbol at offset can be renamed. Returns (token_range, current_name).
    /// Rejects external symbols (WoW API stubs) and external table fields.
    pub fn prepare_rename_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(TextRange, String)> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).right_biased()?;

        if token.kind() == SyntaxKind::Name || token.kind() == SyntaxKind::Parameter {
            let name = token.text().to_string();
            // A table-constructor key is a field even when a same-named symbol is in scope.
            if let Some(&table_idx) = self.constructor_key_owners(token).first() {
                return (!table_idx.is_external()).then(|| (token.text_range(), name));
            }
            // Try symbol first
            if let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, offset) {
                if symbol_idx.is_external() {
                    return None;
                }
                return Some((token.text_range(), name));
            }
            // Try field
            if let Some((table_idx, _, _, _, _)) = self.resolve_field_chain_at(tree, offset) {
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
        // Try `@field name` in a class annotation
        if let Some((_, name, range)) = self.find_field_in_annotation_at(tree, offset) {
            return Some((range, name));
        }

        None
    }
}
