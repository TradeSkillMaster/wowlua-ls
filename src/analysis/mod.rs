pub mod prescan;
pub mod build_ir;
pub mod lower_expression;
pub mod narrowing;
pub mod resolve;
pub mod resolve_call;
pub mod checks;
pub mod queries;
pub mod semantic_tokens;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::diagnostics::WowDiagnostic;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::{SyntaxTree, NodeId};
use crate::types::*;
use crate::pre_globals::PreResolvedGlobals;

// ── Scope-chain walking helpers ─────────────────────────────────────────────

pub(crate) fn ancestor_scopes(scopes: &[Scope], start: ScopeIndex) -> impl Iterator<Item = ScopeIndex> + '_ {
    let mut current = Some(start);
    std::iter::from_fn(move || {
        let si = current?;
        current = if si.val() < scopes.len() {
            scopes[si.val()].parent
        } else {
            None
        };
        Some(si)
    })
}

fn scope_set_contains(
    map: &HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    scopes: &[Scope],
    sym_idx: SymbolIndex,
    scope_idx: ScopeIndex,
) -> bool {
    ancestor_scopes(scopes, scope_idx)
        .any(|si| map.get(&si).is_some_and(|s| s.contains(&sym_idx)))
}

fn scope_map_get<'a, K: Eq + std::hash::Hash>(
    map: &'a HashMap<ScopeIndex, HashMap<K, ValueType>>,
    scopes: &[Scope],
    key: &K,
    scope_idx: ScopeIndex,
) -> Option<&'a ValueType> {
    ancestor_scopes(scopes, scope_idx)
        .find_map(|si| map.get(&si)?.get(key))
}

// ── Core IR database ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct Ir {
    pub(crate) framexml_enabled: bool,
    pub(crate) ext: Arc<PreResolvedGlobals>,
    pub(crate) scopes: Vec<Scope>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) functions: Vec<Function>,
    pub(crate) tables: Vec<TableInfo>,
    pub(crate) exprs: Vec<Expr>,
    pub(crate) block_scopes: Vec<(u32, u32, ScopeIndex)>,
    pub(crate) classes: HashMap<String, TableIndex>,
    pub(crate) aliases: HashMap<String, ValueType>,
    /// Raw annotation types for local aliases that resolve to Function(None).
    /// Used by materialize_fun_annotations() to recover function signatures from alias fields.
    pub(crate) alias_fun_types: HashMap<String, crate::annotations::AnnotationType>,
    /// Raw annotation types and type params for parameterized aliases (e.g. @alias Foo<K,V> V[]).
    pub(crate) parameterized_aliases: HashMap<String, (Vec<String>, crate::annotations::AnnotationType)>,
    /// Raw annotation types for aliases whose body is a tuple or union-of-tuples
    /// (new-style multi-return aliases, e.g. `@alias Result (true, T) | (false, string)`).
    /// Not stored in `aliases` because tuples don't have a single `ValueType`.
    /// Resolved at `@return Name` / `fun(): Name` use sites.
    pub(crate) tuple_form_aliases: HashMap<String, crate::annotations::AnnotationType>,
    pub(crate) string_literals: HashMap<ExprId, String>,
    pub(crate) number_literals: HashMap<ExprId, String>,
    pub(crate) table_ranges: HashMap<(u32, u32), TableIndex>,
    /// Per-file overlay: user-added fields on external tables (indices >= EXT_BASE).
    pub(crate) overlay_fields: HashMap<TableIndex, HashMap<String, FieldInfo>>,
    /// Bracket-keyed field pairs `[key_expr] = value_expr` from table constructors.
    /// Stored per-table for deferred `table<K, V>` type inference in Phase 2.
    pub(crate) bracket_key_fields: HashMap<TableIndex, Vec<(ExprId, ExprId)>>,
    /// Source ranges for local @class declarations (class name → (start, end) byte offsets).
    pub(crate) class_def_ranges: HashMap<String, (u32, u32)>,
    /// Source ranges for local @alias declarations (alias name → (start, end) byte offsets).
    pub(crate) alias_def_ranges: HashMap<String, (u32, u32)>,
    /// Monotonic counter for ordering scope and version creation. Used to prevent
    /// closure bodies from seeing variable versions created after the closure's scope.
    pub(crate) next_creation_order: u32,
    /// Table index for the `_G` global environment table. Field access on this table
    /// redirects to scope0 symbol lookup. Computed once at analysis construction.
    pub(crate) g_table_idx: Option<TableIndex>,
}

impl Ir {
    /// Check if a table index is the `_G` global environment table.
    #[inline]
    pub(crate) fn is_global_env(&self, table_idx: TableIndex) -> bool {
        self.g_table_idx == Some(table_idx)
    }

    /// Allocate the next creation_order value (monotonically increasing).
    pub(crate) fn next_order(&mut self) -> u32 {
        let order = self.next_creation_order;
        self.next_creation_order += 1;
        order
    }

    // Two-tier lookup: indices < EXT_BASE are local, >= EXT_BASE are external
    pub(crate) fn sym(&self, idx: SymbolIndex) -> &Symbol {
        if idx.is_external() {
            &self.ext.symbols[idx.ext_offset()]
        } else {
            &self.symbols[idx.val()]
        }
    }

    pub(crate) fn func(&self, idx: FunctionIndex) -> &Function {
        if idx.is_external() {
            &self.ext.functions[idx.ext_offset()]
        } else {
            &self.functions[idx.val()]
        }
    }

    pub(crate) fn expr(&self, idx: ExprId) -> &Expr {
        if idx.is_external() {
            &self.ext.exprs[idx.ext_offset()]
        } else {
            &self.exprs[idx.val()]
        }
    }

    pub(crate) fn table(&self, idx: TableIndex) -> &TableInfo {
        if idx.is_external() {
            &self.ext.tables[idx.ext_offset()]
        } else {
            &self.tables[idx.val()]
        }
    }

    pub(crate) fn get_symbol(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex) -> Option<SymbolIndex> {
        let mut scope_idx = Some(scope_idx);
        while let Some(si) = scope_idx {
            let scope_obj = if si.is_external() {
                self.ext.scopes.get(si.ext_offset())?
            } else {
                self.scopes.get(si.val())?
            };
            if let Some(&sym) = scope_obj.symbols.get(id) {
                return Some(sym);
            }
            // At scope 0 (global), also check external globals
            if si.val() == 0 {
                if let Some(&sym) = self.ext.scope0_symbols.get(id) {
                    return Some(sym);
                }
                if self.framexml_enabled
                    && let Some(&sym) = self.ext.framexml_scope0_symbols.get(id) {
                        return Some(sym);
                    }
            }
            scope_idx = scope_obj.parent;
        }
        None
    }

    pub(crate) fn push_expr(&mut self, expr: Expr) -> ExprId {
        self.exprs.push(expr);
        ExprId(self.exprs.len() - 1)
    }

    /// A table is "anonymous-empty" when it carries no user-visible information
    /// beyond being `table` — no class, no declared/inferred fields, no
    /// key/value map types, no parents or metatables. Multiple such indices
    /// produced by separate `{}` literals are display- and semantically
    /// equivalent and can be collapsed in a union.
    pub(crate) fn is_anonymous_empty_table(&self, idx: TableIndex) -> bool {
        let t = self.table(idx);
        t.class_name.is_none()
            && t.fields.is_empty()
            && t.array_fields.is_empty()
            && t.parent_classes.is_empty()
            && t.key_type.is_none()
            && t.value_type.is_none()
            && t.metatable.is_none()
            && t.metatable_index.is_none()
            && t.call_func.is_none()
            && t.built_table.is_none()
            && !t.is_enum
    }

    /// Collapse structurally-equivalent `Table(Some(_))` members in a `Union`.
    /// Separate `{}` literals across branches produce distinct `TableIndex`
    /// values but render identically as `table`, so multiple such members
    /// collapse to a single representative. Class tables with the same
    /// `class_name` also collapse to the first occurrence. Non-empty
    /// anonymous tables (with declared fields / key-value types / parents
    /// / metatables) are left as-is — their shapes may genuinely differ even
    /// when indices are distinct, and structural comparison is out of scope
    /// here. Applied after `ValueType::make_union` at union-producing sites
    /// (branch merge, function return aggregation, binary op resolve).
    pub(crate) fn dedupe_union_tables(&self, vt: ValueType) -> ValueType {
        let ValueType::Union(members) = vt else { return vt };
        let mut result: Vec<ValueType> = Vec::with_capacity(members.len());
        let mut seen_anon = false;
        let mut seen_class_names: Vec<String> = Vec::new();
        for m in members {
            match &m {
                ValueType::Table(Some(idx)) => {
                    if let Some(cn) = self.table(*idx).class_name.clone() {
                        if seen_class_names.iter().any(|n| n == &cn) {
                            continue;
                        }
                        seen_class_names.push(cn);
                        result.push(m);
                    } else if self.is_anonymous_empty_table(*idx) {
                        if !seen_anon {
                            seen_anon = true;
                            result.push(m);
                        }
                    } else if !result.contains(&m) {
                        result.push(m);
                    }
                }
                _ => {
                    if !result.contains(&m) {
                        result.push(m);
                    }
                }
            }
        }
        ValueType::make_union(result)
    }

    /// Create a new symbol version whose type_source is `StripNil(previous_version)`.
    /// Returns the new version index, or `None` if the symbol is external.
    pub(crate) fn push_strip_nil_version(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<usize> {
        if sym_idx.is_external() { return None; }
        let prev_ver = self.version_for_scope(sym_idx, scope_idx);
        let prev_ref = self.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
        let stripped = self.push_expr(Expr::StripNil(prev_ref));
        let node = self.symbols[sym_idx.val()].versions[prev_ver].def_node;
        let order = self.next_order();
        let new_ver = self.symbols[sym_idx.val()].versions.len();
        self.symbols[sym_idx.val()].versions.push(SymbolVersion {
            def_node: node,
            type_source: Some(stripped),
            resolved_type: None,
            type_args: Vec::new(),
            created_in_scope: scope_idx,
            creation_order: order,
        });
        Some(new_ver)
    }

    /// Push a new symbol version whose type is identical to `base_ver` — used
    /// to "restore" a symbol after a scoped narrowing (e.g. the RHS of `and`)
    /// so later lookups via `version_for_scope` see the un-narrowed type.
    /// No-op for external symbols.
    pub(crate) fn push_alias_version(
        &mut self, sym_idx: SymbolIndex, base_ver: usize, scope_idx: ScopeIndex,
    ) {
        if sym_idx.is_external() { return; }
        let node = self.symbols[sym_idx.val()].versions[base_ver].def_node;
        let ref_expr = self.push_expr(Expr::SymbolRef(sym_idx, base_ver));
        let order = self.next_order();
        self.symbols[sym_idx.val()].versions.push(SymbolVersion {
            def_node: node,
            type_source: Some(ref_expr),
            resolved_type: None,
            type_args: Vec::new(),
            created_in_scope: scope_idx,
            creation_order: order,
        });
    }

    /// Create a new symbol version whose type_source is `OverloadNarrow(previous_version)`.
    /// Returns the new version index, or `None` if the symbol is external.
    pub(crate) fn push_overload_narrow_version(
        &mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex,
        func_expr: ExprId, ret_index: usize, narrowed: Vec<(usize, NarrowKind)>,
    ) -> Option<usize> {
        if sym_idx.is_external() { return None; }
        // Ancestors-only so that a narrowing version from a sibling branch scope
        // doesn't become the base for an outer-scope narrowing (which would chain
        // narrowings across branches and produce empty types when they disagree).
        let prev_ver = self.version_for_scope_ancestors_only(sym_idx, scope_idx);
        let prev_ref = self.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
        let narrow_expr = self.push_expr(Expr::OverloadNarrow {
            inner: prev_ref,
            func_expr,
            ret_index,
            narrowed,
        });
        let node = self.symbols[sym_idx.val()].versions[prev_ver].def_node;
        let order = self.next_order();
        let new_ver = self.symbols[sym_idx.val()].versions.len();
        self.symbols[sym_idx.val()].versions.push(SymbolVersion {
            def_node: node,
            type_source: Some(narrow_expr),
            resolved_type: None,
            type_args: Vec::new(),
            created_in_scope: scope_idx,
            creation_order: order,
        });
        Some(new_ver)
    }

    pub(super) fn insert_scope(&mut self, parent: Option<ScopeIndex>) -> ScopeIndex {
        let order = self.next_order();
        self.scopes.push(Scope {
            parent,
            symbols: HashMap::new(),
            creation_order: order,
        });
        ScopeIndex(self.scopes.len() - 1)
    }

    pub(super) fn insert_symbol(&mut self, id: SymbolIdentifier, scope_idx: ScopeIndex, node: DefNode) -> SymbolIndex {
        let order = self.next_order();
        let version = SymbolVersion {
            def_node: node,
            type_source: None,
            resolved_type: None,
            type_args: Vec::new(),
            created_in_scope: scope_idx,
            creation_order: order,
        };
        // Only add a version to existing symbols in the SAME scope (reassignment tracking).
        // Do NOT walk the parent scope chain — that would add versions to outer-scope
        // variables instead of shadowing them (e.g. function params with same name as outer locals).
        if let Some(&existing_symbol) = self.scopes[scope_idx.val()].symbols.get(&id)
            && !existing_symbol.is_external() {
                self.symbols.get_mut(existing_symbol.val()).unwrap().versions.push(version);
                return existing_symbol;
            }
        {
            self.symbols.push(Symbol {
                id: id.clone(),
                scope_idx,
                versions: vec![version],
            });
            let symbol_idx = SymbolIndex(self.symbols.len() - 1);
            let current_scope = self.scopes.get_mut(scope_idx.val()).unwrap();
            current_scope.symbols.insert(id, symbol_idx);
            symbol_idx
        }
    }

    /// Like `insert_symbol`, but walks the parent scope chain to find an existing symbol
    /// to version. Used for plain assignments (`x = expr`) where we want to add a version
    /// to the outer-scope variable rather than creating a new shadow symbol.
    pub(super) fn insert_or_version_symbol(&mut self, id: SymbolIdentifier, scope_idx: ScopeIndex, node: DefNode) -> SymbolIndex {
        let order = self.next_order();
        let version = SymbolVersion {
            def_node: node,
            type_source: None,
            resolved_type: None,
            type_args: Vec::new(),
            created_in_scope: scope_idx,
            creation_order: order,
        };
        // Walk the scope chain to find an existing local symbol to add a version to.
        let mut si = Some(scope_idx);
        while let Some(s) = si {
            if s.is_external() { break; }
            if let Some(&existing_symbol) = self.scopes[s.val()].symbols.get(&id)
                && !existing_symbol.is_external() {
                    self.symbols.get_mut(existing_symbol.val()).unwrap().versions.push(version);
                    return existing_symbol;
                }
            si = self.scopes[s.val()].parent;
        }
        // No existing local found — create a new symbol (implicit global).
        self.symbols.push(Symbol {
            id: id.clone(),
            scope_idx,
            versions: vec![version],
        });
        let symbol_idx = SymbolIndex(self.symbols.len() - 1);
        let current_scope = self.scopes.get_mut(scope_idx.val()).unwrap();
        current_scope.symbols.insert(id, symbol_idx);
        symbol_idx
    }

    pub(super) fn set_type_source(&mut self, symbol_idx: SymbolIndex, expr_id: ExprId) {
        let symbol = &mut self.symbols[symbol_idx.val()];
        let version = symbol.versions.last_mut().expect("symbol must have at least one version");
        version.type_source = Some(expr_id);
    }

    pub(super) fn find_table_for_symbol(&self, root_name: &str, scope_idx: ScopeIndex) -> Option<TableIndex> {
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name.to_string()), scope_idx)?;
        let ver_idx = self.version_for_scope(symbol_idx, scope_idx);
        let type_source = self.sym(symbol_idx).versions[ver_idx].type_source?;
        self.find_table_index(type_source)
    }

    pub(super) fn find_table_index(&self, expr_id: ExprId) -> Option<TableIndex> {
        match self.expr(expr_id) {
            Expr::TableConstructor(idx) => Some(*idx),
            Expr::Literal(ValueType::Table(Some(idx))) => Some(*idx),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let sym_idx = *sym_idx;
                let ver_idx = *ver_idx;
                let type_source = self.sym(sym_idx).versions[ver_idx].type_source?;
                self.find_table_index(type_source)
            }
            Expr::Grouped(inner)
            | Expr::StripNil(inner)
            | Expr::StripFalsy(inner) => self.find_table_index(*inner),
            _ => None,
        }
    }

    pub(crate) fn find_root_symbol(&self, expr_id: ExprId) -> Option<SymbolIndex> {
        match self.expr(expr_id) {
            Expr::SymbolRef(sym_idx, _) => Some(*sym_idx),
            Expr::FieldAccess { table, .. } => self.find_root_symbol(*table),
            Expr::Grouped(inner) => self.find_root_symbol(*inner),
            Expr::StripNil(inner) | Expr::StripFalsy(inner) => self.find_root_symbol(*inner),
            Expr::BranchMerge(exprs) => exprs.first().and_then(|e| self.find_root_symbol(*e)),
            _ => None,
        }
    }

    /// Extract the full field chain from a nested FieldAccess expression.
    /// E.g. `FieldAccess(FieldAccess(SymRef(self), "_state"), "x")` → `(self, ["_state", "x"])`
    pub(crate) fn extract_field_chain(&self, expr_id: ExprId) -> Option<(SymbolIndex, Vec<String>)> {
        let mut fields = Vec::new();
        let mut current = expr_id;
        loop {
            match self.expr(current) {
                Expr::FieldAccess { table, field, .. } => {
                    fields.push(field.clone());
                    current = *table;
                }
                Expr::SymbolRef(sym_idx, _) => {
                    fields.reverse();
                    return Some((*sym_idx, fields));
                }
                Expr::Grouped(inner) |
                Expr::StripNil(inner) |
                Expr::StripFalsy(inner) => {
                    current = *inner;
                }
                _ => return None,
            }
        }
    }

    // ── Overlay-aware field lookups ──────────────────────────────────────────

    /// Look up a field on a table, checking per-file overlay first for external tables,
    /// then walking parent_classes for inherited fields, then metatable __index chain.
    pub(crate) fn get_field(&self, table_idx: TableIndex, field_name: &str) -> Option<&FieldInfo> {
        if let Some(fi) = self.get_field_direct(table_idx, field_name) {
            return Some(fi);
        }
        // Walk metatable __index chain with cycle detection
        let mut visited = HashSet::new();
        self.get_field_via_metatable(table_idx, field_name, &mut visited)
    }

    /// Direct field lookup: overlay → own fields → parent_classes. No metatable fallback.
    fn get_field_direct(&self, table_idx: TableIndex, field_name: &str) -> Option<&FieldInfo> {
        if table_idx.is_external()
            && let Some(fields) = self.overlay_fields.get(&table_idx)
                && let Some(fi) = fields.get(field_name) {
                    return Some(fi);
                }
        if let Some(fi) = self.table(table_idx).fields.get(field_name) {
            return Some(fi);
        }
        for &parent_idx in &self.table(table_idx).parent_classes {
            if let Some(fi) = self.table(parent_idx).fields.get(field_name) {
                return Some(fi);
            }
        }
        None
    }

    /// Walk the metatable __index chain to find a field, with cycle detection.
    fn get_field_via_metatable(&self, table_idx: TableIndex, field_name: &str, visited: &mut HashSet<TableIndex>) -> Option<&FieldInfo> {
        if !visited.insert(table_idx) { return None; }
        let index_idx = self.table(table_idx).metatable_index?;
        // Check __index table's own fields + parents
        if let Some(fi) = self.get_field_direct(index_idx, field_name) {
            return Some(fi);
        }
        // Recurse into __index table's own metatable chain
        self.get_field_via_metatable(index_idx, field_name, visited)
    }

    /// Check if a field exists on a table (base, overlay, or inherited).
    pub(crate) fn has_field(&self, table_idx: TableIndex, field_name: &str) -> bool {
        self.get_field(table_idx, field_name).is_some()
    }

    /// Check if a table or any of its parents has the given accessor.
    pub(crate) fn has_accessor(&self, table_idx: TableIndex, name: &str) -> bool {
        if self.table(table_idx).accessors.contains_key(name) {
            return true;
        }
        for &parent_idx in &self.table(table_idx).parent_classes {
            if self.table(parent_idx).accessors.contains_key(name) {
                return true;
            }
        }
        false
    }

    /// Get accessor visibility from a table or its parents.
    pub(crate) fn get_accessor(&self, table_idx: TableIndex, name: &str) -> Option<crate::annotations::Visibility> {
        if let Some(&vis) = self.table(table_idx).accessors.get(name) {
            return Some(vis);
        }
        for &parent_idx in &self.table(table_idx).parent_classes {
            if let Some(&vis) = self.table(parent_idx).accessors.get(name) {
                return Some(vis);
            }
        }
        None
    }

    /// Check whether a version created in `version_scope` is visible from `reference_scope`.
    /// A version is visible if its scope is an ancestor, descendant, or equal to the reference scope.
    /// Versions from sibling scopes (neither ancestor nor descendant) are NOT visible.
    pub(crate) fn is_scope_visible_from(&self, version_scope: ScopeIndex, reference_scope: ScopeIndex) -> bool {
        if version_scope == reference_scope { return true; }
        // Check if version_scope is an ancestor of reference_scope
        if ancestor_scopes(&self.scopes, reference_scope).skip(1).any(|s| s == version_scope) {
            return true;
        }
        // Check if version_scope is a descendant of reference_scope
        ancestor_scopes(&self.scopes, version_scope).skip(1).any(|s| s == reference_scope)
    }

    /// Find the latest version of a symbol that is visible from `scope_idx`.
    /// A version is visible if its scope is an ancestor, descendant, or equal to `scope_idx`.
    ///
    /// For versions created in an **ancestor** scope, an additional temporal check
    /// ensures the version was created **before** the querying scope. This prevents
    /// closure bodies (whose scope was created before an enclosing assignment) from
    /// seeing variable versions created by that assignment.
    pub(crate) fn version_for_scope(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> usize {
        // External symbols always have a single version; no branch filtering needed
        if sym_idx.is_external() {
            return self.ext.symbols[sym_idx.ext_offset()].versions.len() - 1;
        }
        let scope_order = self.scopes[scope_idx.val()].creation_order;
        let sym = &self.symbols[sym_idx.val()];
        for (i, ver) in sym.versions.iter().enumerate().rev() {
            if ver.created_in_scope == scope_idx {
                // Same scope: always visible
                return i;
            }
            if self.is_scope_visible_from(ver.created_in_scope, scope_idx) {
                // Ancestor or descendant scope: check temporal ordering.
                // Only skip if the version was created in a strict ancestor and
                // was created AFTER the querying scope (i.e. the querying scope
                // is a closure whose scope existed before this version).
                if ver.creation_order > scope_order && self.is_ancestor_scope(ver.created_in_scope, scope_idx) {
                    continue;
                }
                return i;
            }
        }
        // Fallback: always return version 0 (original definition)
        0
    }

    fn is_ancestor_scope(&self, ancestor: ScopeIndex, descendant: ScopeIndex) -> bool {
        ancestor_scopes(&self.scopes, descendant).skip(1).any(|s| s == ancestor)
    }

    /// Find the latest version of a symbol that was created in `scope_idx` or an ancestor scope.
    /// Unlike `version_for_scope`, this does NOT consider versions from descendant (child) scopes.
    /// Used by BranchMerge to find the pre-branch version without picking up child scope assignments.
    pub(crate) fn version_for_scope_ancestors_only(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> usize {
        if sym_idx.is_external() {
            return self.ext.symbols[sym_idx.ext_offset()].versions.len() - 1;
        }
        let sym = &self.symbols[sym_idx.val()];
        for (i, ver) in sym.versions.iter().enumerate().rev() {
            if ancestor_scopes(&self.scopes, scope_idx).any(|s| s == ver.created_in_scope) {
                return i;
            }
        }
        0
    }

    /// Insert a field into the overlay for an external table.
    pub(crate) fn insert_overlay_field(&mut self, table_idx: TableIndex, field_name: String, field_info: FieldInfo) {
        self.overlay_fields.entry(table_idx).or_default().insert(field_name, field_info);
    }

    /// Get a mutable reference to an overlay field.
    pub(crate) fn get_overlay_field_mut(&mut self, table_idx: TableIndex, field_name: &str) -> Option<&mut FieldInfo> {
        self.overlay_fields.get_mut(&table_idx)?.get_mut(field_name)
    }

    // ── Methods shared by Analysis and AnalysisResult ────────────────────────

    pub(crate) fn scope_at_offset(&self, offset: impl Into<u32>) -> Option<ScopeIndex> {
        let off: u32 = offset.into();
        let mut best: Option<(u32, ScopeIndex)> = None; // (length, scope)
        for &(start, end, scope_idx) in &self.block_scopes {
            if start <= off && off < end {
                let len = end - start;
                match best {
                    None => best = Some((len, scope_idx)),
                    Some((best_len, _)) if len < best_len => {
                        best = Some((len, scope_idx));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, idx)| idx)
    }

    pub(crate) fn function_name(&self, func_idx: FunctionIndex) -> Option<String> {
        for sym in &self.symbols {
            if let SymbolIdentifier::Name(name) = &sym.id {
                for ver in &sym.versions {
                    if let Some(ValueType::Function(Some(idx))) = &ver.resolved_type
                        && *idx == func_idx { return Some(name.clone()); }
                }
            }
        }
        for sym in &self.ext.symbols {
            if let SymbolIdentifier::Name(name) = &sym.id {
                for ver in &sym.versions {
                    if let Some(ValueType::Function(Some(idx))) = &ver.resolved_type
                        && *idx == func_idx { return Some(name.clone()); }
                }
            }
        }
        None
    }

    /// Check if two table indices refer to the same class (possibly across local/external).
    pub(crate) fn same_class(&self, a: TableIndex, b: TableIndex) -> bool {
        if a == b { return true; }
        let a_name = self.table(a).class_name.as_deref();
        let b_name = self.table(b).class_name.as_deref();
        a_name.is_some() && a_name == b_name
    }

    /// Check if `child_idx` is the same class as or inherits from `parent_idx`.
    pub(crate) fn is_subclass_of(&self, child_idx: TableIndex, parent_idx: TableIndex) -> bool {
        let mut visited = HashSet::new();
        self.is_subclass_of_inner(child_idx, parent_idx, &mut visited)
    }

    fn is_subclass_of_inner(&self, child_idx: TableIndex, parent_idx: TableIndex, visited: &mut HashSet<TableIndex>) -> bool {
        if self.same_class(child_idx, parent_idx) { return true; }
        if !visited.insert(child_idx) { return false; }
        for &p in &self.table(child_idx).parent_classes {
            if self.is_subclass_of_inner(p, parent_idx, visited) { return true; }
        }
        false
    }

    /// Find the class table index of the nearest enclosing method.
    /// Walks up the AST from `node` to find `function Foo:Bar()` or
    /// `function Foo.bar()` / `function Foo.__accessor.bar()` and resolves `Foo`.
    pub(crate) fn find_enclosing_class(&self, node: &SyntaxNode<'_>) -> Option<TableIndex> {
        use crate::ast::{AstNode, FunctionDefinition};
        use crate::syntax::SyntaxKind;
        use crate::syntax::TextSize;

        let mut current = node.parent();
        while let Some(n) = current {
            if n.kind() == SyntaxKind::FunctionDefinition
                && let Some(func_def) = FunctionDefinition::cast(n)
                    && let Some(ident) = func_def.identifier() {
                        let names = ident.names();
                        if names.len() >= 2 {
                            let first_name_token = ident.syntax().children_with_tokens()
                                .filter_map(|it| it.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name)?;
                            let offset = TextSize::from(u32::from(first_name_token.text_range().start()));
                            let scope_idx = self.scope_at_offset(offset)?;
                            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                            let ver = self.sym(sym_idx).versions.last()?;
                            if let Some(ValueType::Table(Some(idx))) = &ver.resolved_type {
                                return Some(*idx);
                            }
                        }
                    }
            current = n.parent();
        }
        None
    }
}

// ── Stored analysis output for LSP queries ───────────────────────────────────

/// Stored analysis output for LSP queries. No lifetime — can be persisted in Document.
/// Contains only the fields that query methods actually read.
pub struct AnalysisResult {
    pub(crate) ir: Ir,
    pub(crate) diagnostics: Vec<WowDiagnostic>,
    pub(crate) is_meta: bool,
    pub(crate) symbol_version_at: HashMap<u32, usize>,
    pub(crate) resolved_expr_cache: HashMap<ExprId, Option<ValueType>>,
    pub(crate) narrowed_symbols: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) falsy_narrowed_symbols: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) type_narrowed_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    pub(crate) type_filtered_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    pub(crate) type_stripped_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    pub(crate) call_type_args: HashMap<ExprId, Vec<ValueType>>,
    pub(crate) field_type_args_cache: HashMap<(TableIndex, String), Vec<ValueType>>,
}

impl AnalysisResult {
    // ── Delegators for two-tier lookups ──────────────────────────────────────

    #[inline] pub(crate) fn sym(&self, idx: SymbolIndex) -> &Symbol { self.ir.sym(idx) }
    #[inline] pub(crate) fn func(&self, idx: FunctionIndex) -> &Function { self.ir.func(idx) }
    #[inline] pub(crate) fn expr(&self, idx: ExprId) -> &Expr { self.ir.expr(idx) }
    #[inline] pub(crate) fn table(&self, idx: TableIndex) -> &TableInfo { self.ir.table(idx) }
    #[inline] pub(crate) fn get_symbol(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex) -> Option<SymbolIndex> { self.ir.get_symbol(id, scope_idx) }
    #[inline] pub(crate) fn get_field(&self, table_idx: TableIndex, field_name: &str) -> Option<&FieldInfo> { self.ir.get_field(table_idx, field_name) }
    #[inline] pub(crate) fn scope_at_offset(&self, offset: impl Into<u32>) -> Option<ScopeIndex> { self.ir.scope_at_offset(offset) }
    #[inline] pub(crate) fn same_class(&self, a: TableIndex, b: TableIndex) -> bool { self.ir.same_class(a, b) }
    #[inline] pub(crate) fn is_subclass_of(&self, child_idx: TableIndex, parent_idx: TableIndex) -> bool { self.ir.is_subclass_of(child_idx, parent_idx) }
    #[inline] pub(crate) fn find_enclosing_class(&self, node: &SyntaxNode<'_>) -> Option<TableIndex> { self.ir.find_enclosing_class(node) }

    pub fn is_meta(&self) -> bool {
        self.is_meta
    }

    pub fn diagnostics(&self) -> &[WowDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn is_symbol_narrowed(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        scope_set_contains(&self.narrowed_symbols, &self.ir.scopes, sym_idx, scope_idx)
    }

    pub(crate) fn is_symbol_falsy_narrowed(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        scope_set_contains(&self.falsy_narrowed_symbols, &self.ir.scopes, sym_idx, scope_idx)
    }

    pub(crate) fn get_type_narrowing(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        scope_map_get(&self.type_narrowed_symbols, &self.ir.scopes, &sym_idx, scope_idx)
    }

    pub(crate) fn get_type_filtering(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        scope_map_get(&self.type_filtered_symbols, &self.ir.scopes, &sym_idx, scope_idx)
    }

    pub(crate) fn get_type_stripping(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        scope_map_get(&self.type_stripped_symbols, &self.ir.scopes, &sym_idx, scope_idx)
    }

}

// ── Deferred checks (written during build_ir, consumed during checks) ────────

#[derive(Debug)]
pub(crate) struct DeferredChecks {
    pub(crate) return_type_checks: Vec<ReturnTypeCheck>,
    pub(crate) field_type_checks: Vec<FieldTypeCheck>,
    pub(crate) assign_type_checks: Vec<AssignTypeCheck>,
    pub(crate) unresolved_globals: Vec<UnresolvedGlobal>,
    pub(crate) created_globals: Vec<CreatedGlobal>,
    pub(crate) nil_check_sites: Vec<NilCheckSite>,
    pub(crate) field_assignment_sites: Vec<FieldAssignmentSite>,
    pub(crate) missing_fields_checks: Vec<MissingFieldsCheck>,
    pub(crate) call_exprs: Vec<ExprId>,
    pub(crate) local_defs: Vec<LocalDef>,
    pub(crate) grouped_return_checks: Vec<GroupedReturnCheck>,
    pub(crate) undefined_field_checks: Vec<UndefinedFieldCheck>,
    pub(crate) deep_field_injections: Vec<DeepFieldInjection>,
    pub(crate) deferred_field_assignments: Vec<DeferredFieldAssignment>,
    pub(crate) redefined_local_checks: Vec<RedefinedLocalCheck>,
    pub(crate) return_count_checks: Vec<ReturnCountCheck>,
    pub(crate) inject_field_checks: Vec<InjectFieldCheck>,
    pub(crate) discard_returns_checks: Vec<DiscardReturnsCheck>,
    pub(crate) wrong_flavor_api_checks: Vec<WrongFlavorApiCheck>,
    pub(crate) annotation_validation_checks: Vec<AnnotationValidationCheck>,
}

/// Pending refinement of a single synthesized return-only overload slot.
/// Placeholder `ValueType::Any` is emitted at build time for non-literal
/// return positions; at resolve time, each still-unresolved `candidate` is
/// retried and — once it resolves — its type is folded into `resolved` and
/// the candidate is dropped. The slot is updated every time `resolved` grows,
/// so a candidate that never resolves (e.g. an unannotated extern with no
/// inferable return) doesn't block the contributions of its siblings.
#[derive(Debug, Clone)]
pub(crate) struct SynthOverloadRefinement {
    pub(crate) function_idx: FunctionIndex,
    pub(crate) overload_idx: usize,
    pub(crate) ret_pos: usize,
    /// Candidate ExprIds not yet resolved. Drained as they resolve.
    pub(crate) candidates: Vec<ExprId>,
    /// Already-resolved types (deduped), carried across fixpoint iterations.
    pub(crate) resolved: Vec<ValueType>,
}

// ── Main struct ──────────────────────────────────────────────────────────────

pub struct Analysis<'a> {
    pub(crate) tree: &'a SyntaxTree,
    pub(crate) ir: Ir,
    pub(crate) deferred: DeferredChecks,
    // Metadata (written during build_ir, read during resolve+checks)
    pub(crate) defclass_vars: HashMap<String, TableIndex>,
    // ── Narrowing tracking maps ──────────────────────────────────────────────
    // Convention: each map's name describes what the guard STRIPPED to produce the
    // narrowing, not what the value now is. See CLAUDE.md (*Return-only overloads*).
    //   narrowed_symbols       — nil stripped (e.g. `x ~= nil`)
    //   falsy_narrowed_symbols — nil AND false stripped (e.g. `if x then`); subset of `narrowed_symbols`
    //   truthy_narrowed_symbols — truthy stripped → value IS `nil | false` (e.g. `if not x` / else of `if x`)
    //   class_narrowed_symbols — equated to a class value (e.g. `x == ERROR.MAX`); value IS that class
    pub(crate) narrowed_symbols: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) falsy_narrowed_symbols: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) truthy_narrowed_symbols: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) class_narrowed_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, String>>,
    pub(crate) narrowed_fields: HashMap<ScopeIndex, HashSet<(SymbolIndex, Vec<String>)>>,
    pub(crate) falsy_narrowed_fields: HashMap<ScopeIndex, HashSet<(SymbolIndex, Vec<String>)>>,
    pub(crate) type_narrowed_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    /// Like `type_narrowed_symbols` but for field chains (e.g. `self._state.field`).
    /// Used for literal boolean return discrimination on field-chain method calls.
    pub(crate) type_narrowed_fields: HashMap<ScopeIndex, HashMap<(SymbolIndex, Vec<String>), ValueType>>,
    /// Like `type_stripped_symbols` but for field chains.
    /// Used for inverse type() guard on fields: `if type(obj.f) == "table" then` → else-branch strips table.
    pub(crate) type_stripped_fields: HashMap<ScopeIndex, HashMap<(SymbolIndex, Vec<String>), ValueType>>,
    /// Like `type_narrowed_symbols` but filters the union to keep matching types
    /// instead of replacing with a bare type. Used for type() guard then-branches
    /// to preserve specific types like `string[]` when narrowing by "table".
    pub(crate) type_filtered_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    pub(crate) type_stripped_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    pub(crate) type_of_aliases: HashMap<SymbolIndex, SymbolIndex>,
    pub(crate) symbol_version_at: HashMap<u32, usize>, // token start offset → version_idx used at that point
    /// For each symbol, the SymbolRef sites (expression id + token offset) where it's referenced.
    /// Used by resolve-time narrowing to retroactively update `SymbolRef(_, _)` expressions
    /// and `symbol_version_at` entries whose scope lies within a newly-pushed narrowing
    /// version's scope subtree — so deferred narrowing propagates to pre-lowered references.
    pub(crate) sym_ref_sites: HashMap<SymbolIndex, Vec<(ExprId, u32)>>,
    /// Cache for lazily-materialized type-narrowing versions.
    /// Maps (reference_scope, symbol) → version index pushed for that narrowing.
    pub(super) type_narrows_version_cache: HashMap<(ScopeIndex, SymbolIndex), usize>,
    /// Symbols whose type-narrowing was overridden by a reassignment in a given scope.
    /// Checked (with scope-chain walk) to skip stale narrowing after assignment.
    pub(crate) narrowing_overridden: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) referenced_symbols: HashSet<SymbolIndex>,
    pub(crate) symbol_type_annotations: HashMap<SymbolIndex, ValueType>,
    pub(crate) functions_with_returns: HashSet<FunctionIndex>,
    pub(crate) resolving_exprs: HashSet<ExprId>,
    pub(crate) resolve_depth: usize,
    pub(crate) resolved_expr_cache: HashMap<ExprId, Option<ValueType>>,
    /// Memoizes the table index produced by `@builds-field` / `@built-name`
    /// operations at a given FunctionCall expression. Survives cache clears
    /// so that re-resolving the builder chain (after @built-name class
    /// discovery triggers a fixpoint restart) reuses the same tables
    /// instead of cloning fresh ones each iteration.
    pub(crate) builder_call_memo: HashMap<ExprId, TableIndex>,
    /// Substituted type_args for generic function calls whose return annotation
    /// is `Parameterized("ClassName", [...])`. Populated during call resolution
    /// when generic inference succeeds. Consumed by `get_expr_type_args` to
    /// carry type arguments from a call's return to the assigned receiver, so
    /// that subsequent method calls on that receiver can re-substitute T.
    pub(crate) call_type_args: HashMap<ExprId, Vec<ValueType>>,
    /// Cache of materialized field type args (Gap 1). Keyed by (enclosing
    /// table, field name); value is the resolved type-argument list.
    /// Populated lazily in `get_expr_type_args`'s FieldAccess branch so that
    /// repeated method calls on the same `@field foo X<fun(...)>` don't
    /// re-materialize a fresh `Function(Some(idx))` per call site. Transient
    /// (per-Analysis), not serialized — dies with IR rebuild.
    pub(crate) field_type_args_cache: HashMap<(TableIndex, String), Vec<ValueType>>,
    /// Multi-return sibling groups for return-only overload narrowing.
    /// Maps each symbol to the full list of (ret_index, SymbolIndex) for all siblings (including itself).
    pub(crate) multi_return_siblings: HashMap<SymbolIndex, Vec<(usize, SymbolIndex)>>,
    /// Deferred sibling narrowings for cross-file FieldAccess calls where the function
    /// can't be resolved at build time.
    /// Processed during the resolve fixpoint loop once the function type is available.
    pub(crate) deferred_sibling_narrowings: Vec<DeferredSiblingNarrowing>,
    /// Deferred class-equality narrowings from `x == EXPR` / `x ~= EXPR` where EXPR
    /// can't be classified at build time. Each entry: (sym_idx, expr_id, scope_idx).
    /// Processed in resolve: if EXPR's type is a class table, narrow sym_idx to that class
    /// and propagate to multi-return siblings.
    pub(crate) deferred_class_eq_narrowings: Vec<(SymbolIndex, ExprId, ScopeIndex)>,
    /// Groups of local variables that are always assigned together in if/elseif branches.
    /// When one is narrowed via nil guard, others should be narrowed too.
    pub(crate) correlated_locals: Vec<Vec<SymbolIndex>>,
    /// Asymmetric narrowing: when the key symbol is narrowed non-nil, every derived
    /// symbol in the value list is also narrowed. Populated from `x = x or y`
    /// assignments — if `y` is known non-nil, `x` (just assigned `x or y`) is too.
    /// One-directional: narrowing `x` does NOT imply anything about `y`.
    pub(crate) or_coalesce_derivations: HashMap<SymbolIndex, Vec<SymbolIndex>>,
    /// Callee ExprIds guarded by `and` field guards (e.g. `self._func and self._func()`).
    /// These are suppressed from need-check-nil call diagnostics in resolve.
    pub(crate) and_guarded_call_exprs: HashSet<ExprId>,
    /// ExprIds lowered inside a conditionally-reached region of a function body —
    /// specifically the RHS of short-circuit `and`/`or`, and the body of
    /// if/elseif/else/while/repeat/for blocks. Used by backward param-type
    /// inference to downgrade baseline hints (which drive inference) to
    /// narrowing-only hints (which can only tighten an existing baseline) when
    /// the contributing expression may not execute on a given function call.
    /// Populated once during `build_ir` (an AST-level property) and read-only
    /// thereafter — no clearing between fixpoint iterations.
    pub(crate) conditionally_reached_exprs: HashSet<ExprId>,
    /// Pending refinements for synthesized return-only overloads. Each entry
    /// points at one `overloads[overload_idx].returns[ret_pos]` slot that was
    /// emitted as `ValueType::Any` at build time because the return expression
    /// was not a literal. During resolve, the candidate expressions are
    /// resolved and their union replaces the placeholder. Entries are retained
    /// until every candidate resolves, and removed once the slot is refined.
    pub(crate) synth_return_overload_refinements: Vec<SynthOverloadRefinement>,
    // Tracks whether we are currently inside a function during build_ir (None = file scope)
    pub(super) current_func_id: Option<FunctionIndex>,
    // Pending function bodies from inline function expressions (used during build_ir)
    pub(super) pending_blocks: Vec<(NodeId, ScopeIndex, Option<FunctionIndex>)>,
    // Config
    pub(crate) allowed_read_globals: HashSet<String>,
    pub(crate) allowed_write_globals: HashSet<String>,
    /// Declared target flavors for the project (see `crate::flavor`). Zero
    /// means flavor filtering is disabled (backward-compat).
    pub(crate) project_flavors: u8,
    /// Per-scope override of the active flavor set. Scopes without an entry
    /// inherit from their parent (walked at lookup time).
    pub(crate) scope_flavors: HashMap<ScopeIndex, u8>,
    pub(crate) backward_param_types: bool,
    /// When true, functions without `@return` annotations whose return statements
    /// match a clear all-set-or-all-nil pattern get synthesized return-only
    /// overloads (so call sites get sibling narrowing). Off by default.
    pub(crate) correlated_return_overloads: bool,
    pub(crate) implicit_protected_prefix: bool,
    /// Functions detected as inherited constructors (e.g. `__init` on a class
    /// that declares `@constructor __init`) but not explicitly `@constructor`.
    /// Used by post-resolution `constructor_return` diagnostic check.
    pub(crate) inherited_constructors: HashSet<FunctionIndex>,
    /// Maps function index → owning class name for methods defined with colon
    /// syntax on a `@class` table. Used by post-resolution `builds_field_not_self`
    /// and `return_self_class_name` checks.
    pub(crate) function_owner_class: HashMap<FunctionIndex, String>,
    // Output
    pub(crate) diagnostics: Vec<WowDiagnostic>,
    pub(crate) is_meta: bool,
    /// Set when a safety limit is hit during resolution (iteration cap, table cap, depth cap).
    pub(crate) safety_limit_hit: Option<String>,
}

impl<'a> Analysis<'a> {
    /// Create a new Analysis from a pre-parsed tree.
    pub fn new_with_tree(
        tree: &'a SyntaxTree,
        pre_globals: Arc<PreResolvedGlobals>,
        framexml_enabled: bool,
        allowed_read_globals: HashSet<String>,
        allowed_write_globals: HashSet<String>,
    ) -> Analysis<'a> {
        Self::new_with_tree_and_flavors(
            tree, pre_globals, framexml_enabled,
            allowed_read_globals, allowed_write_globals, 0, true, true, false,
        )
    }

    /// Like `new_with_tree` but accepts the project's declared flavor mask and
    /// flags to enable/disable inference passes.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_tree_and_flavors(
        tree: &'a SyntaxTree,
        pre_globals: Arc<PreResolvedGlobals>,
        framexml_enabled: bool,
        allowed_read_globals: HashSet<String>,
        allowed_write_globals: HashSet<String>,
        project_flavors: u8,
        backward_param_types: bool,
        correlated_return_overloads: bool,
        implicit_protected_prefix: bool,
    ) -> Analysis<'a> {
        // Compute _G table index from PreResolvedGlobals for field-to-global redirect
        let g_table_idx = pre_globals.scope0_symbols
            .get(&SymbolIdentifier::Name("_G".to_string()))
            .and_then(|&sym_idx| {
                let sym = &pre_globals.symbols[sym_idx.ext_offset()];
                match sym.versions.last()?.resolved_type.as_ref()? {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                }
            });

        let mut analysis = Analysis {
            tree,
            ir: Ir {
                framexml_enabled,
                ext: pre_globals,
                scopes: Vec::new(),
                symbols: Vec::new(),
                functions: Vec::new(),
                tables: Vec::new(),
                exprs: Vec::new(),
                block_scopes: Vec::new(),
                classes: HashMap::new(),
                aliases: HashMap::new(),
                alias_fun_types: HashMap::new(),
                parameterized_aliases: HashMap::new(),
                tuple_form_aliases: HashMap::new(),
                string_literals: HashMap::new(),
                number_literals: HashMap::new(),
                table_ranges: HashMap::new(),
                overlay_fields: HashMap::new(),
                bracket_key_fields: HashMap::new(),
                class_def_ranges: HashMap::new(),
                alias_def_ranges: HashMap::new(),
                next_creation_order: 0,
                g_table_idx,
            },
            deferred: DeferredChecks {
                return_type_checks: Vec::new(),
                field_type_checks: Vec::new(),
                assign_type_checks: Vec::new(),
                unresolved_globals: Vec::new(),
                created_globals: Vec::new(),
                nil_check_sites: Vec::new(),
                field_assignment_sites: Vec::new(),
                missing_fields_checks: Vec::new(),
                call_exprs: Vec::new(),
                local_defs: Vec::new(),
                grouped_return_checks: Vec::new(),
                undefined_field_checks: Vec::new(),
                deep_field_injections: Vec::new(),
                deferred_field_assignments: Vec::new(),
                redefined_local_checks: Vec::new(),
                return_count_checks: Vec::new(),
                inject_field_checks: Vec::new(),
                discard_returns_checks: Vec::new(),
                wrong_flavor_api_checks: Vec::new(),
                annotation_validation_checks: Vec::new(),
            },
            referenced_symbols: HashSet::new(),
            symbol_type_annotations: HashMap::new(),
            functions_with_returns: HashSet::new(),
            resolving_exprs: HashSet::new(),
            resolve_depth: 0,
            resolved_expr_cache: HashMap::new(),
            builder_call_memo: HashMap::new(),
            call_type_args: HashMap::new(),
            field_type_args_cache: HashMap::new(),
            multi_return_siblings: HashMap::new(),
            deferred_sibling_narrowings: Vec::new(),
            deferred_class_eq_narrowings: Vec::new(),
            correlated_locals: Vec::new(),
            or_coalesce_derivations: HashMap::new(),
            and_guarded_call_exprs: HashSet::new(),
            conditionally_reached_exprs: HashSet::new(),
            synth_return_overload_refinements: Vec::new(),
            defclass_vars: HashMap::new(),
            narrowed_symbols: HashMap::new(),
            falsy_narrowed_symbols: HashMap::new(),
            truthy_narrowed_symbols: HashMap::new(),
            class_narrowed_symbols: HashMap::new(),
            narrowed_fields: HashMap::new(),
            falsy_narrowed_fields: HashMap::new(),
            type_narrowed_symbols: HashMap::new(),
            type_narrowed_fields: HashMap::new(),
            type_stripped_fields: HashMap::new(),
            type_filtered_symbols: HashMap::new(),
            type_stripped_symbols: HashMap::new(),
            type_of_aliases: HashMap::new(),
            symbol_version_at: HashMap::new(),
            sym_ref_sites: HashMap::new(),
            type_narrows_version_cache: HashMap::new(),
            narrowing_overridden: HashMap::new(),
            current_func_id: None,
            pending_blocks: Vec::new(),
            allowed_read_globals,
            allowed_write_globals,
            project_flavors,
            scope_flavors: HashMap::new(),
            backward_param_types,
            correlated_return_overloads,
            implicit_protected_prefix,
            inherited_constructors: HashSet::new(),
            function_owner_class: HashMap::new(),
            diagnostics: Vec::new(),
            is_meta: false,
            safety_limit_hit: None,
        };
        crate::diagnostics::trailing_space::check(&mut analysis.diagnostics, tree.source());
        analysis.prescan_classes_and_aliases();
        analysis.prescan_defclass_calls();
        analysis.build_ir();
        analysis.materialize_fun_annotations();
        analysis.inject_preresolved();
        analysis
    }

    /// Get the root SyntaxNode for tree traversal.
    pub(crate) fn root(&self) -> SyntaxNode<'a> {
        SyntaxNode::new_root(self.tree)
    }

    // ── Delegators for two-tier lookups (zero call-site changes needed) ──────

    #[inline] pub(crate) fn sym(&self, idx: SymbolIndex) -> &Symbol { self.ir.sym(idx) }
    #[inline] pub(crate) fn func(&self, idx: FunctionIndex) -> &Function { self.ir.func(idx) }
    #[inline] pub(crate) fn expr(&self, idx: ExprId) -> &Expr { self.ir.expr(idx) }
    #[inline] pub(crate) fn table(&self, idx: TableIndex) -> &TableInfo { self.ir.table(idx) }
    #[inline] pub(crate) fn get_symbol(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex) -> Option<SymbolIndex> { self.ir.get_symbol(id, scope_idx) }
    #[inline] pub(crate) fn get_field(&self, table_idx: TableIndex, field_name: &str) -> Option<&FieldInfo> { self.ir.get_field(table_idx, field_name) }
    #[inline] pub(crate) fn scope_at_offset(&self, offset: impl Into<u32>) -> Option<ScopeIndex> { self.ir.scope_at_offset(offset) }
    #[inline] pub(crate) fn function_name(&self, func_idx: FunctionIndex) -> Option<String> { self.ir.function_name(func_idx) }
    #[inline] pub(crate) fn same_class(&self, a: TableIndex, b: TableIndex) -> bool { self.ir.same_class(a, b) }
    #[inline] pub(crate) fn is_subclass_of(&self, child_idx: TableIndex, parent_idx: TableIndex) -> bool { self.ir.is_subclass_of(child_idx, parent_idx) }
    #[inline] pub(crate) fn find_enclosing_class(&self, node: &SyntaxNode<'_>) -> Option<TableIndex> { self.ir.find_enclosing_class(node) }

    /// Whether `inject-field` on `class_name.field_name` should be suppressed.
    /// Writes to `_G.<known-global>` — directly or via a local alias of `_G` —
    /// are semantically plain global assignments, not field injection on a
    /// class. Uses the same lookup as `undefined-global` so stub, FrameXML,
    /// workspace-defined, and allowed-globals names are all covered.
    pub(crate) fn suppress_inject_field_on_g(&self, class_name: &str, field_name: &str, scope_idx: ScopeIndex) -> bool {
        if class_name != "_G" { return false; }
        if self.allowed_read_globals.contains(field_name)
            || self.allowed_write_globals.contains(field_name) {
            return true;
        }
        self.ir.get_symbol(&SymbolIdentifier::Name(field_name.to_string()), scope_idx).is_some()
    }

    pub fn dump(&self) {
        println!("Symbols:");
        for symbol in self.ir.symbols.iter() {
            println!("    {:?} (scope_idx: {:?}):", &symbol.id, &symbol.scope_idx);
            for version in &symbol.versions {
                println!("        def: {:?}, source: {:?}, resolved: {:?}",
                    version.def_node, version.type_source, version.resolved_type);
            }
        }
        println!("Functions:");
        for (i, func) in self.ir.functions.iter().enumerate() {
            println!("    [{}] {:?}", i, func);
        }
        println!("Tables:");
        for (i, table) in self.ir.tables.iter().enumerate() {
            let class_label = table.class_name.as_deref().unwrap_or("");
            println!("    [{}] {} fields: {:?}", i, class_label, table.fields.keys().collect::<Vec<_>>());
        }
        if !self.ir.classes.is_empty() {
            println!("Classes:");
            for (name, table_idx) in &self.ir.classes {
                println!("    {} -> table[{}]", name, table_idx);
            }
        }
        if !self.ir.aliases.is_empty() {
            println!("Aliases:");
            for (name, vt) in &self.ir.aliases {
                println!("    {} -> {:?}", name, vt);
            }
        }
    }

    pub(crate) fn is_symbol_narrowed(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        scope_set_contains(&self.narrowed_symbols, &self.ir.scopes, sym_idx, scope_idx)
    }

    pub(crate) fn is_symbol_falsy_narrowed(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        scope_set_contains(&self.falsy_narrowed_symbols, &self.ir.scopes, sym_idx, scope_idx)
    }

    pub(crate) fn get_type_narrowing(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        scope_map_get(&self.type_narrowed_symbols, &self.ir.scopes, &sym_idx, scope_idx)
    }

    pub(crate) fn get_field_type_narrowing(&self, sym_idx: SymbolIndex, chain: &[String], scope_idx: ScopeIndex) -> Option<&ValueType> {
        let key = (sym_idx, chain.to_vec());
        scope_map_get(&self.type_narrowed_fields, &self.ir.scopes, &key, scope_idx)
    }

    pub(crate) fn get_field_type_stripping(&self, sym_idx: SymbolIndex, chain: &[String], scope_idx: ScopeIndex) -> Option<&ValueType> {
        let key = (sym_idx, chain.to_vec());
        scope_map_get(&self.type_stripped_fields, &self.ir.scopes, &key, scope_idx)
    }

    pub(crate) fn get_type_filtering(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        scope_map_get(&self.type_filtered_symbols, &self.ir.scopes, &sym_idx, scope_idx)
    }

    pub(crate) fn get_type_stripping(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        scope_map_get(&self.type_stripped_symbols, &self.ir.scopes, &sym_idx, scope_idx)
    }

    pub(crate) fn is_narrowing_overridden(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        scope_set_contains(&self.narrowing_overridden, &self.ir.scopes, sym_idx, scope_idx)
    }

    pub(crate) fn is_field_chain_narrowed(&self, sym_idx: SymbolIndex, fields: &[String], scope_idx: ScopeIndex) -> bool {
        Self::check_field_set(&self.narrowed_fields, sym_idx, fields, scope_idx, &self.ir.scopes)
    }

    pub(crate) fn is_field_falsy_narrowed(&self, sym_idx: SymbolIndex, fields: &[String], scope_idx: ScopeIndex) -> bool {
        Self::check_field_set(&self.falsy_narrowed_fields, sym_idx, fields, scope_idx, &self.ir.scopes)
    }

    fn check_field_set(
        map: &HashMap<ScopeIndex, HashSet<(SymbolIndex, Vec<String>)>>,
        sym_idx: SymbolIndex,
        fields: &[String],
        scope_idx: ScopeIndex,
        scopes: &[Scope],
    ) -> bool {
        ancestor_scopes(scopes, scope_idx).any(|si| {
            let Some(narrowed) = map.get(&si) else { return false };
            let key = (sym_idx, fields.to_vec());
            if narrowed.contains(&key) {
                return true;
            }
            (1..fields.len()).any(|len| {
                let prefix = (sym_idx, fields[..len].to_vec());
                narrowed.contains(&prefix)
            })
        })
    }

    /// Look up the active flavor mask at `scope_idx` by walking ancestor
    /// scopes for the first explicit override; falls back to the project's
    /// declared flavors. Returns 0 when flavor filtering is disabled.
    pub(crate) fn active_flavors_at(&self, scope_idx: ScopeIndex) -> u8 {
        if self.project_flavors == 0 { return 0; }
        ancestor_scopes(&self.ir.scopes, scope_idx)
            .find_map(|si| self.scope_flavors.get(&si).copied())
            .unwrap_or(self.project_flavors)
    }

    /// Narrow the active flavor set in `scope_idx` to the intersection of
    /// `new_mask` with whatever is already active. Used by flavor guards.
    pub(crate) fn narrow_scope_flavors(&mut self, scope_idx: ScopeIndex, new_mask: u8) {
        if self.project_flavors == 0 { return; }
        let parent_scope = if scope_idx.val() < self.ir.scopes.len() {
            self.ir.scopes[scope_idx.val()].parent.unwrap_or(scope_idx)
        } else {
            scope_idx
        };
        let parent_mask = self.active_flavors_at(parent_scope);
        let effective = parent_mask & new_mask;
        self.scope_flavors.insert(scope_idx, effective);
    }

    /// Set the active flavor set in `scope_idx` to `parent_mask & !exclude_mask`
    /// — used for else-branches of flavor comparisons.
    pub(crate) fn exclude_scope_flavors(&mut self, scope_idx: ScopeIndex, exclude_mask: u8) {
        if self.project_flavors == 0 { return; }
        let parent_scope = if scope_idx.val() < self.ir.scopes.len() {
            self.ir.scopes[scope_idx.val()].parent.unwrap_or(scope_idx)
        } else {
            scope_idx
        };
        let parent_mask = self.active_flavors_at(parent_scope);
        let effective = parent_mask & !exclude_mask;
        self.scope_flavors.insert(scope_idx, effective);
    }

    /// Consume this Analysis and produce an AnalysisResult for LSP queries.
    pub fn into_result(self) -> AnalysisResult {
        AnalysisResult {
            ir: self.ir,
            diagnostics: self.diagnostics,
            is_meta: self.is_meta,
            symbol_version_at: self.symbol_version_at,
            resolved_expr_cache: self.resolved_expr_cache,
            narrowed_symbols: self.narrowed_symbols,
            falsy_narrowed_symbols: self.falsy_narrowed_symbols,
            type_narrowed_symbols: self.type_narrowed_symbols,
            type_filtered_symbols: self.type_filtered_symbols,
            type_stripped_symbols: self.type_stripped_symbols,
            call_type_args: self.call_type_args,
            field_type_args_cache: self.field_type_args_cache,
        }
    }
}
