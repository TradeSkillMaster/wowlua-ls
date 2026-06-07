use super::*;

impl AnalysisResult {
    /// Returns `true` when `token` sits inside the `ExpressionList` (RHS) of
    /// the specific `LocalAssignStatement` whose byte range matches `def_node`.
    /// Stops the walk at function boundaries so that
    /// `local f = function() f() end` still resolves the recursive `f`.
    pub(super) fn is_in_defining_local_assign_rhs(token: &SyntaxToken<'_>, def_node: &DefNode) -> bool {
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
        if let Some((table_idx, field_name, expr_id, _, _)) = self.resolve_field_chain_at(tree, offset) {
            // Check the field's local definition range first (from @field annotation,
            // table constructor, or field assignment site). This prevents jumping to an
            // unrelated external file when the field is defined in the current file.
            // Guard: get_field() walks parent classes and metatables, so `fi` could
            // originate from a different table; the external check prevents using an
            // overlay field on an external table as a local definition.
            if let Some(fi) = self.get_field(table_idx, &field_name)
                && let Some((start, end)) = fi.def_range
                && !table_idx.is_external() {
                    let range = TextRange::new(
                        TextSize::from(start),
                        TextSize::from(end),
                    );
                    return Some(DefinitionResult::Local(range));
                }
            if let Some(result) = self.definition_for_expr(expr_id) {
                return Some(result);
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
        if let Some((table_idx, field_name, expr_id, _, _)) = self.resolve_field_chain_at(tree, offset) {
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
    pub(super) fn type_definition_for_value(&self, vt: &ValueType) -> Option<DefinitionResult> {
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
    pub(super) fn class_definition_by_name(&self, name: &str) -> Option<DefinitionResult> {
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
    pub(super) fn alias_definition_by_name(&self, name: &str) -> Option<DefinitionResult> {
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

    pub(super) fn definition_for_expr(&self, expr_id: ExprId) -> Option<DefinitionResult> {
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

    /// Go-to-definition on a class or alias name inside an annotation comment.
    pub(super) fn annotation_name_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
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
}
