use super::*;

impl AnalysisResult {
    pub(super) fn is_field_position(tree: &SyntaxTree, offset: u32) -> bool {
        let text_size = TextSize::from(offset);
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(_, right) => right,
            TokenAtOffset::None => return false,
        };
        if token.kind() != SyntaxKind::Name { return false; }
        if let Some(parent) = token.parent() {
            // The take_while keeps non-token children (child nodes) in the iterator
            // unconditionally: the only identifier-like parents that contain Names are
            // shapes like NameRef / DotAccess / MethodCall, whose child nodes are
            // receivers/argument lists that always come BEFORE the field-name Name we
            // care about. Stopping iteration on them would miss a leading `:`/`.` that
            // sits between the child node and the Name. Passing them through is safe
            // because the `.any` only matches Dot/Colon tokens.
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
    pub(super) fn is_g_dot_field(&self, tree: &SyntaxTree, offset: u32) -> bool {
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

    pub fn find_symbol_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(SymbolIndex, String, u32)> {
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

    /// Resolve the type of a symbol at the given token offset, selecting the correct
    /// symbol version for redefined locals, params, and external symbols.
    ///
    /// This is the version-tracking logic shared by `type_definition_at` and `hover_at`.
    pub fn symbol_resolved_type_at(&self, symbol_idx: SymbolIndex, token_start: u32) -> Option<&ValueType> {
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

    /// Search for an external field location across the table hierarchy
    /// (own fields → class_name redirect → addon namespace → parent classes → metatable chain).
    pub(super) fn find_external_field_location(&self, table_idx: TableIndex, field_name: &str) -> Option<&ExternalLocation> {
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

    /// Extract the identifier word at the given byte offset if it falls inside an annotation comment.
    /// Supports both `---` line comments and `--[[...]]` / `--[=[...]=]` block comments
    /// that contain `@`-prefixed annotation content (e.g. `@as`, `@cast`, `@type`).
    pub(super) fn annotation_word_at(&self, tree: &SyntaxTree, offset: u32) -> Option<String> {
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
            let inner = crate::analysis::block_comment_inner(tok_text)?;
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

    pub(super) fn extract_table_idx(resolved: &ValueType) -> Option<TableIndex> {
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
    pub(super) fn extract_all_table_indices(resolved: &ValueType) -> Vec<TableIndex> {
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

    /// Resolve a bare-variable expression node typed as a constrained type variable
    /// (`@generic T: C` + `@param x \`T\``) to its class constraint `C`.
    /// Counterpart of `backtick_type_var_constraint` in resolve_call.rs (which
    /// operates on ExprIds rather than SyntaxNodes).
    pub(super) fn resolve_type_var_constraint_at_expr(&self, expr_node: &SyntaxNode) -> Option<ValueType> {
        let names: Vec<_> = expr_node.descendants_with_tokens()
            .filter_map(|c| c.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();
        if names.len() != 1 {
            return None;
        }
        let start = names[0].text_range().start();
        let scope_idx = self.scope_at_offset(start)?;
        let sym = self.get_symbol(&SymbolIdentifier::Name(names[0].text().to_string()), scope_idx)?;
        let tv_name = match self.symbol_resolved_type_at(sym, u32::from(start)) {
            Some(ValueType::TypeVariable(n)) => n.clone(),
            _ => return None,
        };
        self.ir.type_var_class_constraint_for_param(sym, &tv_name)
    }

    /// Find a field by name across multiple tables and their parent classes.
    /// Returns the owning table index and the field's expr id.
    pub(super) fn find_field_in_tables(&self, table_indices: &[TableIndex], field_name: &str) -> Option<(TableIndex, ExprId)> {
        self.find_all_fields_in_tables(table_indices, field_name).into_iter().next()
    }

    /// Collect all (table_idx, expr_id) pairs for a field across multiple tables
    /// and their parent classes. Checks direct fields first, then parent classes.
    /// Each table appears at most once (deduped). Used by `find_field_in_tables`
    /// (first match) and hover (all matches for union receiver display).
    pub(super) fn find_all_fields_in_tables(&self, table_indices: &[TableIndex], field_name: &str) -> Vec<(TableIndex, ExprId)> {
        let mut results = Vec::new();
        let mut seen_tables: HashSet<TableIndex> = HashSet::new();
        // Direct fields first
        for &idx in table_indices {
            if let Some(fi) = self.get_field(idx, field_name)
                && seen_tables.insert(idx)
            {
                results.push((idx, fi.expr));
            }
        }
        // Then parent classes
        for &idx in table_indices {
            for &parent_idx in &self.table(idx).parent_classes.clone() {
                if let Some(fi) = self.get_field(parent_idx, field_name)
                    && seen_tables.insert(parent_idx)
                {
                    results.push((parent_idx, fi.expr));
                }
            }
        }
        results
    }

    /// Resolve a dot/colon chain at offset, returning (owning_table_idx, field_name, field_expr_id, access_kind).
    /// Byte range of the `Name` token at `offset`, matching the `field_range`
    /// stored on method/field `FieldAccess` exprs during lowering. Used to look
    /// up `method_decl_subs` for hover type-variable substitution.
    pub(super) fn method_name_range_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(u32, u32)> {
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

    /// Resolve a field access chain at the given offset.
    /// Returns (table_idx, field_name, expr_id, access_kind, all_receiver_tables).
    /// The 5th element contains all table indices from the receiver's type. It is
    /// empty only for access patterns with no typed receiver (dot chains, funcall
    /// chains, `_G` redirects). Single-class colon receivers get a one-element Vec
    /// containing just `table_idx`; union receivers get one entry per union member.
    pub fn resolve_field_chain_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(TableIndex, String, ExprId, FieldAccessKind, Vec<TableIndex>)> {
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
        self.resolve_field_chain_for_token(token)
    }

    /// Resolve a field access chain for a pre-found Name token.
    /// Same as `resolve_field_chain_at` but avoids redundant O(log n) tree traversal
    /// when the caller already has the token (e.g. from a `descendants_with_tokens` walk).
    pub(super) fn resolve_field_chain_for_token(&self, token: SyntaxToken<'_>) -> Option<(TableIndex, String, ExprId, FieldAccessKind, Vec<TableIndex>)> {
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        let text_size = token.text_range().start();
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
                    return Some((table_idx, method_name, expr_id, FieldAccessKind::Colon, table_indices));
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
                return Some((table_idx, field_name, expr_id, access, table_indices));
            }
            // Check _G.field redirect
            for &idx in &table_indices {
                if let Some((ti, fn_, ei, ak)) = self.resolve_g_env_field(idx, &field_name, access) {
                    return Some((ti, fn_, ei, ak, Vec::new()));
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
                                    return Some((table_idx, field_name, fi.expr, access, Vec::new()));
                                }
                                // Check parent classes
                                for &parent_idx in &self.table(table_idx).parent_classes.clone() {
                                    if let Some(fi) = self.table(parent_idx).fields.get(&field_name) {
                                        return Some((parent_idx, field_name, fi.expr, access, Vec::new()));
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
                                return Some((table_idx, field_name, fi.expr, access, Vec::new()));
                            }
                            for &parent_idx in &self.table(table_idx).parent_classes.clone() {
                                if let Some(fi) = self.table(parent_idx).fields.get(&field_name) {
                                    return Some((parent_idx, field_name, fi.expr, access, Vec::new()));
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
            return Some((table_idx, field_name, fi.expr, access, Vec::new()));
        }
        for &parent_idx in &self.table(table_idx).parent_classes.clone() {
            if let Some(fi) = self.get_field(parent_idx, &field_name) {
                return Some((parent_idx, field_name, fi.expr, access, Vec::new()));
            }
        }
        self.resolve_g_env_field(table_idx, &field_name, access)
            .map(|(ti, fn_, ei, ak)| (ti, fn_, ei, ak, Vec::new()))
    }

    /// When `table_idx` is the global environment (`_G`), look up `field_name` as a
    /// scope-0 symbol and return its `type_source` expression. Used as a fallback in
    /// `resolve_field_chain_at` after normal field/parent-class lookup fails.
    pub(super) fn resolve_g_env_field(&self, table_idx: TableIndex, field_name: &str, access: FieldAccessKind) -> Option<(TableIndex, String, ExprId, FieldAccessKind)> {
        if !self.ir.is_global_env(table_idx) { return None; }
        let sym_id = SymbolIdentifier::Name(field_name.to_string());
        if let Some(si) = self.ir.scope0_global_symbol(&sym_id)
            && let Some(source) = self.sym(si).versions.last().and_then(|v| v.type_source) {
                return Some((table_idx, field_name.to_string(), source, access));
            }
        None
    }

    /// Walk one step in a field chain, falling back to global-symbol resolution when
    /// the current table is the `_G` environment. Returns the next table index.
    pub(super) fn resolve_field_or_g_env(&self, idx: TableIndex, name: &str) -> Option<TableIndex> {
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

    /// Check whether two table indices refer to tables connected by inheritance.
    /// Returns `true` if `a` is an ancestor of `b` or `b` is an ancestor of `a`.
    /// Used by find-references to match inherited fields: `resolve_field_chain_at`
    /// returns the parent class that owns a field, but the target may be the child
    /// class (or vice versa).
    pub(super) fn tables_share_field_owner(&self, a: TableIndex, b: TableIndex) -> bool {
        // Check if a is an ancestor of b
        let is_ancestor = |ancestor: TableIndex, descendant: TableIndex| -> bool {
            for &parent_idx in &self.table(descendant).parent_classes {
                if parent_idx == ancestor { return true; }
            }
            // For cross-file: local class → ext class via class_name
            if ancestor.is_external() && !descendant.is_external()
                && let Some(cn) = &self.table(descendant).class_name
                && let Some(ext_idx) = self.ir.ext.classes.get(cn).copied()
            {
                for &parent_idx in &self.table(ext_idx).parent_classes {
                    if parent_idx == ancestor { return true; }
                }
            }
            false
        };
        is_ancestor(a, b) || is_ancestor(b, a)
    }

    /// Detect whether the separator before a Name token in an Identifier is a colon or dot.
    pub(super) fn detect_access_before_token(parent: &SyntaxNode, token: &SyntaxToken) -> FieldAccessKind {
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
    pub(super) fn resolve_method_call_return_table(&self, receiver_table: TableIndex, method_name: &str, call_node: &SyntaxNode) -> Option<TableIndex> {
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
    pub(super) fn resolve_func_return_table(&self, func_idx: FunctionIndex, call_node: &SyntaxNode) -> Option<TableIndex> {
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
    pub(super) fn resolve_backtick_generic_return(&self, func_idx: FunctionIndex, call_node: &SyntaxNode) -> Option<TableIndex> {
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
        // Direct string-literal argument → resolve it as a class name.
        if let Some(string_token) = target_expr.descendants_with_tokens()
            .find_map(|child| {
                if let NodeOrToken::Token(t) = child
                    && t.kind() == SyntaxKind::String { return Some(t); }
                None
            })
        {
            let class_name = string_token.text().trim_matches(|c| c == '"' || c == '\'').to_string();
            // Skip primitive type names — they don't resolve to class tables
            if crate::annotations::resolve_primitive_type_name(&class_name).is_some() {
                return None;
            }
            return self.ir.classes.get(&class_name).copied()
                .or_else(|| self.ir.ext.classes.get(&class_name).copied());
        }
        if let Some(constraint) = self.resolve_type_var_constraint_at_expr(target_expr) {
            return Self::extract_table_idx(&constraint);
        }
        None
    }

    /// Check if a table has @constructor (own or inherited from parent classes).
    pub(super) fn has_constructor(&self, table_idx: TableIndex) -> bool {
        if !self.table(table_idx).constructors.is_empty() {
            return true;
        }
        self.table(table_idx).parent_classes.clone().iter()
            .any(|&p| !self.table(p).constructors.is_empty())
    }

    /// Resolve a FunctionCall syntax node to the table its return type represents.
    /// Handles colon method calls, dot-calls, and chained combinations.
    pub(super) fn resolve_funcall_node_to_table(&self, node: &SyntaxNode, scope_offset: TextSize) -> Option<TableIndex> {
        // Special-case: select(2, ...) → addon namespace table, but only at
        // file scope where `...` is WoW's (addonName, addonTable) vararg.
        if let Some(expr) = Expression::cast(*node)
            && let Some(2) = crate::annotations::is_select_varargs(&expr)
        {
            let inside_function = node.ancestors()
                .any(|a| a.kind() == SyntaxKind::FunctionDefinition);
            if !inside_function {
                return self.ir.addon_table_idx();
            }
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
            }
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
            }
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
    pub(super) fn resolve_identifier_to_table(&self, node: &SyntaxNode, scope_offset: TextSize) -> Option<TableIndex> {
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
    pub(super) fn resolve_identifier_to_type(&self, node: &SyntaxNode, scope_offset: TextSize) -> Option<ValueType> {
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
        // Select the version active at the root token's position. Without this,
        // a symbol with later narrowed/cast/merge versions (e.g. `task` cast to
        // a subclass inside a subsequent if-branch) would pick `versions.last()`
        // and resolve the receiver as a union, causing hover/completion to show
        // duplicate signatures from each union member's parent chain.
        //
        // Narrowing keeps `scope_idx` (from the deeper-in-chain `scope_offset`)
        // because narrowing is scope-keyed, not position-keyed, and for a dot/
        // colon chain in a single statement the root token and the deeper
        // tokens always sit in the same scope. Falls back to
        // `symbol_resolved_type_at`, which uses `symbol_version_at` to pick
        // the version active at the root token; that map is populated by
        // `lower_expression` for every Name and Parameter token use, so
        // receiver root tokens (NameRef in a DotAccess/MethodCall) always
        // have an entry.
        let root_token_start: u32 = root_names[0].text_range().start().into();
        let mut current_type = self.get_type_narrowing(symbol_idx, scope_idx)
            .cloned()
            .or_else(|| self.symbol_resolved_type_at(symbol_idx, root_token_start).cloned())?;

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
    pub(super) fn resolve_receiver_to_all_tables(&self, parent: &SyntaxNode, scope_offset: TextSize) -> Vec<TableIndex> {
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
    pub(super) fn resolve_literal_receiver_type(node: &SyntaxNode) -> Option<ValueType> {
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
    pub fn find_constructor_field_at(&self, tree: &SyntaxTree, offset: u32) -> Option<(String, FieldInfo)> {
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

    /// Find the version whose `def_node` range contains `token_start`.
    /// Used for redefined locals where multiple versions share the same SymbolIndex
    /// but each has a distinct `def_node` from its own `local` statement.
    /// Returns the first matching version because narrowing/merge versions copy the
    /// same `def_node` — we want the original declaration version, not a narrowed one.
    pub(super) fn version_at_def_site<'a>(&self, symbol: &'a Symbol, token_start: u32) -> Option<&'a SymbolVersion> {
        symbol.versions.iter().find(|v| {
            v.def_node.start <= token_start && token_start < v.def_node.end
        })
    }

}
