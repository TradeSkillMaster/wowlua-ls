use super::*;

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

impl AnalysisResult {
    /// Resolve the cross-file identity of the symbol or field at `offset`.
    /// Returns a `ReferenceTarget` whose index (symbol_idx / table_idx) is stable across
    /// any `AnalysisResult` built from the same `PreResolvedGlobals` when the index is
    /// `>= EXT_BASE`. Local-to-file identities (`idx < EXT_BASE`) are only meaningful
    /// to `self` and shouldn't be used for cross-file search.
    pub fn reference_target_at(&self, tree: &SyntaxTree, offset: u32) -> Option<ReferenceTarget> {
        if let Some((symbol_idx, name, _)) = self.find_symbol_at(tree, offset) {
            Some(ReferenceTarget::Symbol { idx: symbol_idx, name })
        } else if let Some((table_idx, field_name, _, _, _)) = self.resolve_field_chain_at(tree, offset) {
            Some(ReferenceTarget::Field { table_idx, field_name })
        } else if let Some((sym_idx, name, _)) = self.find_param_in_annotation_at(tree, offset) {
            Some(ReferenceTarget::Symbol { idx: sym_idx, name })
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
    pub(super) fn is_in_own_local_init(&self, tree: &SyntaxTree, symbol_idx: SymbolIndex, token: &SyntaxToken<'_>, name: &str) -> bool {
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
    pub(super) fn is_local_assign_statement(&self, tree: &SyntaxTree, def_start: u32) -> bool {
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
                            } else if !resolved.is_external()
                                && !symbol_idx.is_external()
                                && self.is_in_own_local_init(tree, resolved, &token, name)
                                && self.get_symbol_excluding(
                                    &SymbolIdentifier::Name(name.clone()),
                                    scope_idx,
                                    resolved,
                                ) == Some(symbol_idx)
                            {
                                // The token is in the RHS of `local shadow = <token> ...`
                                // that shadows the target symbol. In Lua, `local x = x + 1`
                                // means the RHS `x` refers to the outer `x`. The standard
                                // get_symbol finds the shadow (resolved) rather than the
                                // outer symbol; get_symbol_excluding verifies the outer
                                // symbol is actually the target.
                                true
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

                // Include @param annotation name ranges for parameter symbols.
                // Parameters always live in a function body scope (never scope 0),
                // so skip the O(F) scan for non-parameter symbols.
                if !symbol_idx.is_external()
                    && self.sym(symbol_idx).scope_idx != ScopeIndex(0) {
                    for (fi, func) in self.ir.functions.iter().enumerate() {
                        if func.args.contains(&symbol_idx) {
                            for (pname, range) in self.param_annotation_name_ranges(tree, FunctionIndex(fi)) {
                                if pname == *name {
                                    results.push(range);
                                }
                            }
                            break;
                        }
                    }
                    // Re-sort after adding annotation ranges
                    results.sort_by_key(|r| (r.start(), r.end()));
                    results.dedup();
                }

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
                // Field reference: find all Name tokens that resolve to the same table+field
                // via resolve_field_chain_for_token, which correctly handles method calls on
                // function call results, intermediate field chains, and colon-syntax definitions.
                // The accept logic also handles cross-file class_name matching (local @class
                // tables promoted to their EXT_BASE+ counterpart) and inherited fields (the
                // resolved table may be a parent class of the target, or vice versa).
                let mut results = Vec::new();
                for token in SyntaxNode::new_root(tree).descendants_with_tokens().filter_map(|it| it.into_token()) {
                    if token.kind() != SyntaxKind::Name || token.text() != field_name.as_str() {
                        continue;
                    }
                    if let Some((resolved_table, _, _, _, _)) = self.resolve_field_chain_for_token(token) {
                        let accept = if resolved_table == table_idx {
                            true
                        } else if table_idx.is_external() && !resolved_table.is_external() {
                            // Cross-file field search: the file that declares `@class X` keeps a
                            // local table for it with `class_name = "X"`; fields defined on that
                            // local (e.g. `function X:Method() end`) should be matched for an
                            // external `X` target too.
                            let ext_for_local = self.table(resolved_table).class_name.as_ref()
                                .and_then(|n| self.ir.ext.classes.get(n).copied());
                            ext_for_local == Some(table_idx)
                        } else if self.tables_share_field_owner(table_idx, resolved_table) {
                            // Inherited field: the target may be a child class while the resolved
                            // table is the parent that owns the field (or vice versa). Check if
                            // one is an ancestor of the other via parent_classes chains.
                            true
                        } else {
                            false
                        };
                        if accept {
                            results.push(token.text_range());
                        }
                    }
                }

                // Also find string literals passed to keyof-constrained generic
                // parameters (e.g. `CallMethod(obj, "fieldName")` with
                // `@generic Obj, K: keyof Obj`).
                for cr in self.ir.call_resolutions.values() {
                    if cr.generic_subs.is_empty() { continue; }
                    let func = self.func(cr.func_idx);
                    // Pre-build map from generic name → keyof target name.
                    let keyof_targets: Vec<(&str, &str)> = func.generic_constraints_raw.iter()
                        .filter_map(|(n, c)| c.as_deref()
                            .and_then(crate::annotations::parse_keyof_constraint)
                            .map(|ref_name| (n.as_str(), ref_name)))
                        .collect();
                    if keyof_targets.is_empty() { continue; }
                    for (gen_name, bound_type, arg_range) in &cr.generic_subs {
                        let Some(ref_name) = keyof_targets.iter()
                            .find(|(n, _)| *n == gen_name.as_str())
                            .map(|(_, r)| *r) else { continue };
                        let ValueType::String(Some(key)) = bound_type else { continue };
                        if key != field_name { continue; }
                        if let Some(ref_table_idx) = cr.resolve_keyof_target(ref_name) {
                            let accept = ref_table_idx == table_idx
                                || (table_idx.is_external() && !ref_table_idx.is_external()
                                    && self.table(ref_table_idx).class_name.as_ref()
                                        .and_then(|n| self.ir.ext.classes.get(n).copied())
                                        == Some(table_idx))
                                || self.tables_share_field_owner(table_idx, ref_table_idx);
                            if accept
                                && let Some(&(start, end)) = arg_range.as_ref()
                            {
                                // Trim string delimiters so the range covers only the
                                // content (e.g. `Activate` not `"Activate"`).  This is
                                // critical for rename, which replaces the range text.
                                let content_len = key.len() as u32;
                                let total_len = end - start;
                                let delim = (total_len.saturating_sub(content_len)) / 2;
                                results.push(TextRange::new(
                                    TextSize::from(start + delim),
                                    TextSize::from(end - delim),
                                ));
                            }
                        }
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

    /// Parse a `---@param name ...` comment token and extract the param name with its
    /// byte range relative to the comment text start. Returns `(name, start, end)`.
    /// The `?` suffix on optional params is excluded. Skips `self` and `...` params.
    pub(super) fn extract_param_from_comment(text: &str) -> Option<(&str, usize, usize)> {
        if !text.starts_with("---") {
            return None;
        }
        let stripped = text.trim_start_matches('-');
        let prefix_len = text.len() - stripped.len();
        let trimmed = stripped.trim_start();
        let ws_before = stripped.len() - trimmed.len();
        let rest = trimmed.strip_prefix("@param")?;
        let rest_trimmed = rest.trim_start();
        let ws_after_tag = rest.len() - rest_trimmed.len();
        let name_with_q = rest_trimmed.split(char::is_whitespace).next()?;
        let name = name_with_q.trim_end_matches('?');
        if name.is_empty() || name == "..." || name == "self" {
            return None;
        }
        let name_start = prefix_len + ws_before + "@param".len() + ws_after_tag;
        Some((name, name_start, name_start + name.len()))
    }

    /// For a given function, find the byte ranges of each `@param` name in the
    /// preceding annotation comments. Returns `(param_name, TextRange)` pairs.
    pub(super) fn param_annotation_name_ranges(
        &self,
        tree: &SyntaxTree,
        func_idx: FunctionIndex,
    ) -> Vec<(String, TextRange)> {
        let func = self.func(func_idx);
        let def_start = func.def_node.start;
        let Some(start_token) = SyntaxNode::new_root(tree)
            .token_at_offset(TextSize::from(def_start))
            .right_biased()
        else {
            return Vec::new();
        };
        // Walk backward from the function's first token through preceding comments
        let mut results = Vec::new();
        let mut tok = start_token.prev_token();
        while let Some(token) = tok {
            let kind = token.kind();
            if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
                tok = token.prev_token();
                continue;
            }
            if kind == SyntaxKind::Comment {
                let text = token.text();
                if text.starts_with("---") {
                    if let Some((name, ns, ne)) = Self::extract_param_from_comment(text) {
                        let token_start = u32::from(token.text_range().start());
                        let range = TextRange::new(
                            TextSize::from(token_start + ns as u32),
                            TextSize::from(token_start + ne as u32),
                        );
                        results.push((name.to_string(), range));
                    }
                    tok = token.prev_token();
                    continue;
                }
            }
            break;
        }
        results
    }

    /// If the cursor offset falls inside a `@param` name within a comment preceding
    /// a function, return the parameter's symbol index, name, and the TextRange of the
    /// name in the comment. This enables rename-from-annotation.
    pub(crate) fn find_param_in_annotation_at(
        &self,
        tree: &SyntaxTree,
        offset: u32,
    ) -> Option<(SymbolIndex, String, TextRange)> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).right_biased()?;
        if token.kind() != SyntaxKind::Comment {
            return None;
        }
        let (name, ns, ne) = Self::extract_param_from_comment(token.text())?;
        let token_start = u32::from(token.text_range().start());
        let abs_start = token_start + ns as u32;
        let abs_end = token_start + ne as u32;
        // Check cursor is within the name range
        if offset < abs_start || offset >= abs_end {
            return None;
        }
        let name_range = TextRange::new(TextSize::from(abs_start), TextSize::from(abs_end));

        // Walk forward from this comment to find the function it annotates
        let mut forward = token.next_token();
        while let Some(t) = forward {
            let k = t.kind();
            if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline || k == SyntaxKind::Comment {
                forward = t.next_token();
                continue;
            }
            // Found a non-trivia token — walk up to find FunctionDefinition
            let mut node = t.parent();
            while let Some(n) = node {
                if n.kind() == SyntaxKind::FunctionDefinition {
                    let r = n.text_range();
                    let fn_start = u32::from(r.start());
                    // Find the function in ir.functions by def_node.start
                    for func in self.ir.functions.iter() {
                        if func.def_node.start == fn_start {
                            // Find matching param symbol
                            for &sym_idx in &func.args {
                                if let SymbolIdentifier::Name(ref sym_name) = self.sym(sym_idx).id
                                    && sym_name == name {
                                        return Some((sym_idx, name.to_string(), name_range));
                                }
                            }
                            return None;
                        }
                    }
                    return None;
                }
                node = n.parent();
            }
            return None;
        }
        None
    }

    pub(super) fn def_name_token_offset(&self, tree: &SyntaxTree, def_start: u32, def_end: u32, name: &str) -> Option<u32> {
        // Binary-search into the flat token array (sorted by byte offset)
        // instead of walking the entire tree from the root. O(log N + k)
        // where k is the number of tokens in the def_start..def_end range.
        let tokens = &tree.tokens;
        debug_assert!(tokens.len() <= u32::MAX as usize, "token count exceeds u32 — TokenId would overflow");
        let start_idx = tokens.partition_point(|t| t.start < def_start);
        for (i, t) in tokens[start_idx..].iter().enumerate() {
            if t.start > def_end { break; }
            if t.kind == SyntaxKind::Name && tree.token_text(TokenId((start_idx + i) as u32)) == name {
                return Some(t.start);
            }
        }
        None
    }
}
