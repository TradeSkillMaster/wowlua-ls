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

    /// Single-result go-to-definition (the primary definition). Convenience
    /// wrapper over [`Self::definitions_at`] for callers that only want one
    /// location (the CLI `test-query` and the integration-test harness).
    pub fn definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        self.definitions_at(tree, offset).into_iter().next()
    }

    /// All definition locations for the symbol/type at `offset`. When a global,
    /// `@class`, or `@alias` is defined in more than one file, every site is
    /// returned (primary first) so the editor can present a picker. Single-def
    /// cases return a one-element vector, matching the previous behavior.
    pub fn definitions_at(&self, tree: &SyntaxTree, offset: u32) -> Vec<DefinitionResult> {
        // Injected field carried cross-file by an inline `TableShape` member
        // (e.g. a factory's `frame.SetValue = …` read through `Frame & { … }`):
        // jump to its recorded definition site in the defining file. Checked
        // before the field-chain path, which can't resolve a shape-only field.
        if let Some(result) = self.shape_field_definition_at(offset) {
            return vec![result];
        }
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
                    return vec![DefinitionResult::Local(range)];
                }
            if let Some(result) = self.definition_for_expr(expr_id) {
                return self.field_definitions_with_alts(result, expr_id);
            }
            // Fall back to external field location (stubs / workspace @field annotations)
            if let Some(loc) = self.find_external_field_location(table_idx, &field_name) {
                return vec![DefinitionResult::External(loc.clone())];
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
                            return vec![DefinitionResult::External(loc.clone())];
                        }
                    }
                }
            }
        }
        // Don't let a same-named global shadow a field-position token (preceded by dot/colon).
        // Mirrors the same guard in hover_at(); _G.X (including indirect references) is
        // exempted so global-environment field access still works.
        if Self::is_field_position(tree, offset) && !self.is_g_dot_field(tree, offset) {
            return Vec::new();
        }
        // Table constructor field: definition is itself. Check before find_symbol_at
        // so that a same-named global doesn't shadow the field key.
        if self.find_constructor_field_at(tree, offset).is_some() {
            let text_size = TextSize::from(offset);
            if let TokenAtOffset::Single(t) | TokenAtOffset::Between(t, _) = SyntaxNode::new_root(tree).token_at_offset(text_size) {
                return vec![DefinitionResult::Local(t.text_range())];
            }
        }
        if let Some((symbol_idx, _, token_start)) = self.find_symbol_at(tree, offset) {
            if symbol_idx.is_external() {
                return self.external_symbol_definitions(symbol_idx);
            }
            let symbol = self.sym(symbol_idx);
            if let Some(version) = self.version_at_def_site(symbol, token_start)
                .or_else(|| symbol.versions.first())
            {
                return vec![DefinitionResult::Local(TextRange::new(
                    TextSize::from(version.def_node.start),
                    TextSize::from(version.def_node.end),
                ))];
            }
            return Vec::new();
        }
        // Try expression string go-to-definition
        if let Some(result) = self.expression_definition_at(tree, offset) {
            return vec![result];
        }
        // A `keyof X` string names a member of X — jump to it. Combined with any
        // event definition on the same token so `RegisterEvent("PLAYER_LOGIN")`
        // (typed `FrameEvent & keyof self`) lands on both the `self:PLAYER_LOGIN`
        // handler (listed first) and the game event.
        let mut string_defs = Vec::new();
        string_defs.extend(self.keyof_string_definition_at(tree, offset));
        string_defs.extend(self.event_string_definition_at(tree, offset));
        if !string_defs.is_empty() {
            return string_defs;
        }
        // Try annotation class/alias name go-to-definition (may be multi-file)
        let anno = self.annotation_name_definitions_at(tree, offset);
        if !anno.is_empty() {
            return anno;
        }
        Vec::new()
    }

    /// Go-to-definition for a field access whose receiver carries the field via
    /// an inline `TableShape` member (cross-file injected-field carrier). Mirrors
    /// [`Self::shape_field_hover_at`]: finds the `Expr::FieldAccess` whose
    /// field-name range covers `offset`, resolves the receiver type, and returns
    /// the field's carried cross-file definition location when a shape member
    /// declares one. `None` for ordinary class/record fields.
    fn shape_field_definition_at(&self, offset: u32) -> Option<DefinitionResult> {
        for expr in self.ir.exprs.iter() {
            let Expr::FieldAccess { table, field, field_range: Some((s, e)) } = expr else { continue };
            if offset < *s || offset >= *e {
                continue;
            }
            // Skip (not bail) on an unresolvable receiver, matching
            // `shape_field_hover_at`: another lowered `FieldAccess` may share this
            // offset and carry the shape.
            let Some(recv) = self.resolve_expr_type(*table).map(|t| t.into_strip_opaque()) else { continue };
            if let Some(loc) = recv.collect_shape_field_def(field) {
                return Some(DefinitionResult::External(loc.clone()));
            }
        }
        None
    }

    /// All definition sites for an external (global) symbol: the primary
    /// recorded location plus every other workspace file that defines a global
    /// of the same name. Deduplicated by `(path, start)`, primary first.
    fn external_symbol_definitions(&self, symbol_idx: SymbolIndex) -> Vec<DefinitionResult> {
        let primary = self.ir.ext.symbol_locations.get(&symbol_idx);
        let all = match &self.sym(symbol_idx).id {
            SymbolIdentifier::Name(name) => self.ir.ext.symbol_locations_for_name(name),
            _ => &[],
        };
        Self::merge_external_locations(primary, all)
    }

    /// Combine an optional primary location with a list of additional locations
    /// into definition results, deduplicating by `(path, start)` and keeping the
    /// primary (if any) first.
    fn merge_external_locations(
        primary: Option<&ExternalLocation>,
        all: &[ExternalLocation],
    ) -> Vec<DefinitionResult> {
        Self::merge_external_locations_excluding(primary, all, None)
    }

    /// Like [`Self::merge_external_locations`], but also excludes any external
    /// location whose `start` offset matches `exclude_start` (used to avoid
    /// duplicating a local result that was already pushed by the caller).
    fn merge_external_locations_excluding(
        primary: Option<&ExternalLocation>,
        all: &[ExternalLocation],
        exclude_start: Option<u32>,
    ) -> Vec<DefinitionResult> {
        let excluded = |loc: &ExternalLocation| {
            exclude_start.is_some_and(|s| loc.start == s)
        };
        let mut locs: Vec<ExternalLocation> = Vec::new();
        if let Some(p) = primary
            && !excluded(p)
        {
            locs.push(p.clone());
        }
        for loc in all {
            if excluded(loc) {
                continue;
            }
            if !locs.iter().any(|l| l.path == loc.path && l.start == loc.start) {
                locs.push(loc.clone());
            }
        }
        locs.into_iter().map(DefinitionResult::External).collect()
    }

    /// Navigate from a variable to its type's declaration (`textDocument/typeDefinition`).
    ///
    /// For a variable whose resolved type is a `@class`, jumps to the class declaration.
    /// For an `@alias (opaque)` type, jumps to the alias declaration.
    /// For union types, returns the first navigable class/alias member.
    /// Returns `None` for primitives and unresolvable types.
    pub fn type_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        self.type_definitions_at(tree, offset).into_iter().next()
    }

    /// All type-declaration locations for the variable/field at `offset`. When the
    /// resolved type is a `@class`/`@alias` declared in more than one file, every
    /// declaration site is returned (primary first). Single-declaration types
    /// return a one-element vector, matching the previous behavior.
    pub fn type_definitions_at(&self, tree: &SyntaxTree, offset: u32) -> Vec<DefinitionResult> {
        // Try field access first so a same-named global doesn't shadow a field result.
        // Invariant: when resolve_field_chain_at returns Some, the token is always at a
        // field position, so the is_field_position guard below would also return None.
        // We return early here to make that intent clear and prevent symbol lookup
        // from returning the container variable's type for a non-navigable field.
        if let Some((table_idx, field_name, expr_id, _, _)) = self.resolve_field_chain_at(tree, offset) {
            let resolved_type = self.resolve_expr_type(expr_id).or_else(|| {
                self.get_field(table_idx, &field_name)
                    .and_then(|fi| fi.annotation.clone())
            });
            return resolved_type
                .map(|vt| self.type_definitions_for_value(&vt))
                .unwrap_or_default();
        }
        if Self::is_field_position(tree, offset) && !self.is_g_dot_field(tree, offset) {
            return Vec::new();
        }
        if let Some((symbol_idx, _, token_start)) = self.find_symbol_at(tree, offset)
            && let Some(resolved) = self.symbol_resolved_type_at(symbol_idx, token_start)
        {
            return self.type_definitions_for_value(resolved);
        }
        Vec::new()
    }

    /// Map a resolved `ValueType` to all source locations of its class or alias
    /// declaration(s). For unions/intersections, returns the locations of the
    /// first navigable member (matching the previous single-result priority).
    pub(super) fn type_definitions_for_value(&self, vt: &ValueType) -> Vec<DefinitionResult> {
        match vt {
            ValueType::Table(Some(idx)) => match self.table(*idx).class_name.as_deref() {
                Some(class_name) => self.class_definitions_by_name(class_name),
                None => Vec::new(),
            },
            ValueType::OpaqueAlias(name, _) => self.alias_definitions_by_name(name),
            ValueType::Union(types) | ValueType::Intersection(types) => types
                .iter()
                .map(|t| self.type_definitions_for_value(t))
                .find(|defs| !defs.is_empty())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// All `@class` declarations for a name: the live local declaration (current
    /// file) first, then every external/workspace declaration of the same name.
    /// The first element matches the previous single-result priority (local then
    /// external).
    pub(super) fn class_definitions_by_name(&self, name: &str) -> Vec<DefinitionResult> {
        let mut out = Vec::new();
        let local_start = if let Some(&(start, end)) = self.ir.class_def_ranges.get(name) {
            out.push(DefinitionResult::Local(TextRange::new(
                TextSize::from(start),
                TextSize::from(end),
            )));
            Some(start)
        } else {
            None
        };
        out.extend(Self::merge_external_locations_excluding(
            self.ir.ext.class_locations.get(name),
            self.ir.ext.class_locations_for_name(name),
            local_start,
        ));
        out
    }

    /// All `@alias` declarations for a name. See [`Self::class_definitions_by_name`].
    pub(super) fn alias_definitions_by_name(&self, name: &str) -> Vec<DefinitionResult> {
        let mut out = Vec::new();
        let local_start = if let Some(&(start, end)) = self.ir.alias_def_ranges.get(name) {
            out.push(DefinitionResult::Local(TextRange::new(
                TextSize::from(start),
                TextSize::from(end),
            )));
            Some(start)
        } else {
            None
        };
        out.extend(Self::merge_external_locations_excluding(
            self.ir.ext.alias_locations.get(name),
            self.ir.ext.alias_locations_for_name(name),
            local_start,
        ));
        out
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

    /// Given the primary go-to-definition `result` for a method/function field
    /// reached through expr `expr_id`, append every additional external definition
    /// site recorded for the field's function — the built-in stub site plus any
    /// workspace `library` redefinition — deduplicated by `(path, start)`. Returns
    /// `[result]` unchanged when the field isn't a function or has no extra sites.
    /// Mirrors the multi-site behavior of globals/classes/aliases for methods.
    fn field_definitions_with_alts(&self, result: DefinitionResult, expr_id: ExprId) -> Vec<DefinitionResult> {
        let Expr::FunctionDef(func_idx) = self.expr(expr_id) else {
            return vec![result];
        };
        let extras = self.ir.ext.func_alt_locations_for(*func_idx);
        if extras.is_empty() {
            return vec![result];
        }
        let mut out = vec![result];
        for loc in extras {
            if !out.iter().any(|d| matches!(d, DefinitionResult::External(e) if e.path == loc.path && e.start == loc.start)) {
                out.push(DefinitionResult::External(loc.clone()));
            }
        }
        out
    }

    /// Go-to-definition on a class or alias name inside an annotation comment.
    /// Returns all declaration sites (class first, then alias) so a type defined
    /// across multiple files lists every site. The first element preserves the
    /// previous single-result priority (local class, local alias, external class,
    /// external alias).
    pub(super) fn annotation_name_definitions_at(&self, tree: &SyntaxTree, offset: u32) -> Vec<DefinitionResult> {
        let Some(word) = self.annotation_word_at(tree, offset) else { return Vec::new() };
        // A name resolves to a class or an alias, never both; check class first to
        // match the previous resolution order.
        let class_defs = self.class_definitions_by_name(&word);
        if !class_defs.is_empty() {
            return class_defs;
        }
        self.alias_definitions_by_name(&word)
    }
}
