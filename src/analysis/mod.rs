pub mod prescan;
pub mod build_ir;
pub mod resolve;
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
        if idx >= EXT_BASE {
            &self.ext.symbols[idx - EXT_BASE]
        } else {
            &self.symbols[idx]
        }
    }

    pub(crate) fn func(&self, idx: FunctionIndex) -> &Function {
        if idx >= EXT_BASE {
            &self.ext.functions[idx - EXT_BASE]
        } else {
            &self.functions[idx]
        }
    }

    pub(crate) fn expr(&self, idx: ExprId) -> &Expr {
        if idx >= EXT_BASE {
            &self.ext.exprs[idx - EXT_BASE]
        } else {
            &self.exprs[idx]
        }
    }

    pub(crate) fn table(&self, idx: TableIndex) -> &TableInfo {
        if idx >= EXT_BASE {
            &self.ext.tables[idx - EXT_BASE]
        } else {
            &self.tables[idx]
        }
    }

    pub(crate) fn get_symbol(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex) -> Option<SymbolIndex> {
        let mut scope_idx = Some(scope_idx);
        while let Some(si) = scope_idx {
            let scope_obj = if si >= EXT_BASE {
                self.ext.scopes.get(si - EXT_BASE)?
            } else {
                self.scopes.get(si)?
            };
            if let Some(&sym) = scope_obj.symbols.get(id) {
                return Some(sym);
            }
            // At scope 0 (global), also check external globals
            if si == 0 {
                if let Some(&sym) = self.ext.scope0_symbols.get(id) {
                    return Some(sym);
                }
                if self.framexml_enabled {
                    if let Some(&sym) = self.ext.framexml_scope0_symbols.get(id) {
                        return Some(sym);
                    }
                }
            }
            scope_idx = scope_obj.parent;
        }
        None
    }

    pub(crate) fn push_expr(&mut self, expr: Expr) -> ExprId {
        self.exprs.push(expr);
        self.exprs.len() - 1
    }

    /// Create a new symbol version whose type_source is `StripNil(previous_version)`.
    /// Returns the new version index, or `None` if the symbol is external.
    pub(crate) fn push_strip_nil_version(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<usize> {
        if sym_idx >= EXT_BASE { return None; }
        let prev_ver = self.version_for_scope(sym_idx, scope_idx);
        let prev_ref = self.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
        let stripped = self.push_expr(Expr::StripNil(prev_ref));
        let node = self.symbols[sym_idx].versions[prev_ver].def_node;
        let order = self.next_order();
        let new_ver = self.symbols[sym_idx].versions.len();
        self.symbols[sym_idx].versions.push(SymbolVersion {
            def_node: node,
            type_source: Some(stripped),
            resolved_type: None,
            type_args: Vec::new(),
            created_in_scope: scope_idx,
            creation_order: order,
        });
        Some(new_ver)
    }

    /// Create a new symbol version whose type_source is `OverloadNarrow(previous_version)`.
    /// Returns the new version index, or `None` if the symbol is external.
    pub(crate) fn push_overload_narrow_version(
        &mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex,
        func_expr: ExprId, ret_index: usize, narrowed: Vec<(usize, NarrowKind)>,
    ) -> Option<usize> {
        if sym_idx >= EXT_BASE { return None; }
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
        let node = self.symbols[sym_idx].versions[prev_ver].def_node;
        let order = self.next_order();
        let new_ver = self.symbols[sym_idx].versions.len();
        self.symbols[sym_idx].versions.push(SymbolVersion {
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
        self.scopes.len() - 1
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
        if let Some(&existing_symbol) = self.scopes[scope_idx].symbols.get(&id) {
            if existing_symbol < EXT_BASE {
                self.symbols.get_mut(existing_symbol).unwrap().versions.push(version);
                return existing_symbol;
            }
        }
        {
            self.symbols.push(Symbol {
                id: id.clone(),
                scope_idx,
                versions: vec![version],
            });
            let symbol_idx = self.symbols.len() - 1;
            let current_scope = self.scopes.get_mut(scope_idx).unwrap();
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
            if s >= EXT_BASE { break; }
            if let Some(&existing_symbol) = self.scopes[s].symbols.get(&id) {
                if existing_symbol < EXT_BASE {
                    self.symbols.get_mut(existing_symbol).unwrap().versions.push(version);
                    return existing_symbol;
                }
            }
            si = self.scopes[s].parent;
        }
        // No existing local found — create a new symbol (implicit global).
        self.symbols.push(Symbol {
            id: id.clone(),
            scope_idx,
            versions: vec![version],
        });
        let symbol_idx = self.symbols.len() - 1;
        let current_scope = self.scopes.get_mut(scope_idx).unwrap();
        current_scope.symbols.insert(id, symbol_idx);
        symbol_idx
    }

    pub(super) fn set_type_source(&mut self, symbol_idx: SymbolIndex, expr_id: ExprId) {
        let symbol = &mut self.symbols[symbol_idx];
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
        if table_idx >= EXT_BASE {
            if let Some(fields) = self.overlay_fields.get(&table_idx) {
                if let Some(fi) = fields.get(field_name) {
                    return Some(fi);
                }
            }
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
        let mut current = self.scopes.get(reference_scope).and_then(|s| s.parent);
        while let Some(s) = current {
            if s == version_scope { return true; }
            if s >= EXT_BASE { break; }
            current = self.scopes[s].parent;
        }
        // Check if version_scope is a descendant of reference_scope
        let mut current = self.scopes.get(version_scope).and_then(|s| s.parent);
        while let Some(s) = current {
            if s == reference_scope { return true; }
            if s >= EXT_BASE { break; }
            current = self.scopes[s].parent;
        }
        false
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
        if sym_idx >= EXT_BASE {
            return self.ext.symbols[sym_idx - EXT_BASE].versions.len() - 1;
        }
        let scope_order = self.scopes[scope_idx].creation_order;
        let sym = &self.symbols[sym_idx];
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

    /// Check if `ancestor` is a strict ancestor of `descendant`.
    fn is_ancestor_scope(&self, ancestor: ScopeIndex, descendant: ScopeIndex) -> bool {
        let mut current = self.scopes.get(descendant).and_then(|s| s.parent);
        while let Some(s) = current {
            if s == ancestor { return true; }
            if s >= EXT_BASE { break; }
            current = self.scopes[s].parent;
        }
        false
    }

    /// Find the latest version of a symbol that was created in `scope_idx` or an ancestor scope.
    /// Unlike `version_for_scope`, this does NOT consider versions from descendant (child) scopes.
    /// Used by BranchMerge to find the pre-branch version without picking up child scope assignments.
    pub(crate) fn version_for_scope_ancestors_only(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> usize {
        if sym_idx >= EXT_BASE {
            return self.ext.symbols[sym_idx - EXT_BASE].versions.len() - 1;
        }
        let sym = &self.symbols[sym_idx];
        for (i, ver) in sym.versions.iter().enumerate().rev() {
            let vs = ver.created_in_scope;
            if vs == scope_idx { return i; }
            // Check if vs is an ancestor of scope_idx
            let mut current = self.scopes.get(scope_idx).and_then(|s| s.parent);
            while let Some(s) = current {
                if s == vs { return i; }
                if s >= EXT_BASE { break; }
                current = self.scopes[s].parent;
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
                    if let Some(ValueType::Function(Some(idx))) = &ver.resolved_type {
                        if *idx == func_idx { return Some(name.clone()); }
                    }
                }
            }
        }
        for sym in &self.ext.symbols {
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
            if n.kind() == SyntaxKind::FunctionDefinition {
                if let Some(func_def) = FunctionDefinition::cast(n) {
                    if let Some(ident) = func_def.identifier() {
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
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(narrowed) = self.narrowed_symbols.get(&si) {
                if narrowed.contains(&sym_idx) {
                    return true;
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        false
    }

    pub(crate) fn is_symbol_falsy_narrowed(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(narrowed) = self.falsy_narrowed_symbols.get(&si) {
                if narrowed.contains(&sym_idx) {
                    return true;
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        false
    }

    pub(crate) fn get_type_narrowing(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(narrowed) = self.type_narrowed_symbols.get(&si) {
                if let Some(vt) = narrowed.get(&sym_idx) {
                    return Some(vt);
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        None
    }

    pub(crate) fn get_type_filtering(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(filtered) = self.type_filtered_symbols.get(&si) {
                if let Some(vt) = filtered.get(&sym_idx) {
                    return Some(vt);
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        None
    }

    pub(crate) fn get_type_stripping(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(stripped) = self.type_stripped_symbols.get(&si) {
                if let Some(vt) = stripped.get(&sym_idx) {
                    return Some(vt);
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        None
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
    /// Multi-return sibling groups for return-only overload narrowing.
    /// Maps each symbol to the full list of (ret_index, SymbolIndex) for all siblings (including itself).
    pub(crate) multi_return_siblings: HashMap<SymbolIndex, Vec<(usize, SymbolIndex)>>,
    /// Deferred sibling narrowings for cross-file FieldAccess calls where the function
    /// can't be resolved at build time. Each entry is (func_expr_id, siblings, scope_idx, narrowed_info).
    /// narrowed_info is Vec<(ret_index, NarrowKind)> for siblings narrowed at build time.
    /// Processed during the resolve fixpoint loop once the function type is available.
    pub(crate) deferred_sibling_narrowings: Vec<(ExprId, Vec<(usize, SymbolIndex)>, ScopeIndex, Vec<(usize, NarrowKind)>)>,
    /// Deferred class-equality narrowings from `x == EXPR` / `x ~= EXPR` where EXPR
    /// can't be classified at build time. Each entry: (sym_idx, expr_id, scope_idx).
    /// Processed in resolve: if EXPR's type is a class table, narrow sym_idx to that class
    /// and propagate to multi-return siblings.
    pub(crate) deferred_class_eq_narrowings: Vec<(SymbolIndex, ExprId, ScopeIndex)>,
    /// Groups of local variables that are always assigned together in if/elseif branches.
    /// When one is narrowed via nil guard, others should be narrowed too.
    pub(crate) correlated_locals: Vec<Vec<SymbolIndex>>,
    /// Callee ExprIds guarded by `and` field guards (e.g. `self._func and self._func()`).
    /// These are suppressed from need-check-nil call diagnostics in resolve.
    pub(crate) and_guarded_call_exprs: HashSet<ExprId>,
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
    /// Set once `infer_backward_param_types()` has run during resolve_types().
    /// Prevents the inference pass from running every outer fixpoint iteration.
    pub(crate) backward_inference_done: bool,
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
            allowed_read_globals, allowed_write_globals, 0, true,
        )
    }

    /// Like `new_with_tree` but accepts the project's declared flavor mask and
    /// a flag to enable/disable backward param-type inference.
    pub fn new_with_tree_and_flavors(
        tree: &'a SyntaxTree,
        pre_globals: Arc<PreResolvedGlobals>,
        framexml_enabled: bool,
        allowed_read_globals: HashSet<String>,
        allowed_write_globals: HashSet<String>,
        project_flavors: u8,
        backward_param_types: bool,
    ) -> Analysis<'a> {
        // Compute _G table index from PreResolvedGlobals for field-to-global redirect
        let g_table_idx = pre_globals.scope0_symbols
            .get(&SymbolIdentifier::Name("_G".to_string()))
            .and_then(|&sym_idx| {
                let sym = &pre_globals.symbols[sym_idx - EXT_BASE];
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
            },
            referenced_symbols: HashSet::new(),
            symbol_type_annotations: HashMap::new(),
            functions_with_returns: HashSet::new(),
            resolving_exprs: HashSet::new(),
            resolve_depth: 0,
            resolved_expr_cache: HashMap::new(),
            builder_call_memo: HashMap::new(),
            multi_return_siblings: HashMap::new(),
            deferred_sibling_narrowings: Vec::new(),
            deferred_class_eq_narrowings: Vec::new(),
            correlated_locals: Vec::new(),
            and_guarded_call_exprs: HashSet::new(),
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
            backward_inference_done: false,
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
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(narrowed) = self.narrowed_symbols.get(&si) {
                if narrowed.contains(&sym_idx) {
                    return true;
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        false
    }

    /// Check if a symbol was narrowed via a truthiness guard (strip both nil and false).
    pub(crate) fn is_symbol_falsy_narrowed(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(narrowed) = self.falsy_narrowed_symbols.get(&si) {
                if narrowed.contains(&sym_idx) {
                    return true;
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        false
    }

    pub(crate) fn get_type_narrowing(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(narrowed) = self.type_narrowed_symbols.get(&si) {
                if let Some(vt) = narrowed.get(&sym_idx) {
                    return Some(vt);
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        None
    }

    /// Look up a field-chain type narrowing (e.g. from boolean discrimination on `self.x.y:Method()`).
    pub(crate) fn get_field_type_narrowing(&self, sym_idx: SymbolIndex, chain: &[String], scope_idx: ScopeIndex) -> Option<&ValueType> {
        let key = (sym_idx, chain.to_vec());
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(narrowed) = self.type_narrowed_fields.get(&si) {
                if let Some(vt) = narrowed.get(&key) {
                    return Some(vt);
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        None
    }

    /// Look up a field-chain type stripping (inverse type() guard: strips a specific type from the union).
    pub(crate) fn get_field_type_stripping(&self, sym_idx: SymbolIndex, chain: &[String], scope_idx: ScopeIndex) -> Option<&ValueType> {
        let key = (sym_idx, chain.to_vec());
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(stripped) = self.type_stripped_fields.get(&si) {
                if let Some(vt) = stripped.get(&key) {
                    return Some(vt);
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        None
    }

    /// Like `get_type_narrowing` but for type() guard filter-narrowing.
    pub(crate) fn get_type_filtering(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(filtered) = self.type_filtered_symbols.get(&si) {
                if let Some(vt) = filtered.get(&sym_idx) {
                    return Some(vt);
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        None
    }

    pub(crate) fn get_type_stripping(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(stripped) = self.type_stripped_symbols.get(&si) {
                if let Some(vt) = stripped.get(&sym_idx) {
                    return Some(vt);
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        None
    }

    /// Check if a symbol's narrowing was overridden by a reassignment
    /// in the given scope or any ancestor scope.
    pub(crate) fn is_narrowing_overridden(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(set) = self.narrowing_overridden.get(&si) {
                if set.contains(&sym_idx) {
                    return true;
                }
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        false
    }

    /// Check whether a field chain (e.g. `["_state", "subMenu"]` on symbol `self`) is narrowed.
    /// Returns true if the exact chain or any prefix of it is narrowed in the scope hierarchy.
    pub(crate) fn is_field_chain_narrowed(&self, sym_idx: SymbolIndex, fields: &[String], scope_idx: ScopeIndex) -> bool {
        Self::check_field_set(&self.narrowed_fields, sym_idx, fields, scope_idx, &self.ir.scopes)
    }

    /// Check whether a field chain was narrowed via a truthiness guard (strip both nil and false).
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
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(narrowed) = map.get(&si) {
                // Check exact match
                let key = (sym_idx, fields.to_vec());
                if narrowed.contains(&key) {
                    return true;
                }
                // Check if any prefix of the chain is narrowed (e.g. narrowing `self._state`
                // also covers `self._state.subMenu`)
                for len in 1..fields.len() {
                    let prefix = (sym_idx, fields[..len].to_vec());
                    if narrowed.contains(&prefix) {
                        return true;
                    }
                }
            }
            if si < scopes.len() {
                current = scopes[si].parent;
            } else {
                break;
            }
        }
        false
    }

    /// Look up the active flavor mask at `scope_idx` by walking ancestor
    /// scopes for the first explicit override; falls back to the project's
    /// declared flavors. Returns 0 when flavor filtering is disabled.
    pub(crate) fn active_flavors_at(&self, scope_idx: ScopeIndex) -> u8 {
        if self.project_flavors == 0 { return 0; }
        let mut current = Some(scope_idx);
        while let Some(si) = current {
            if let Some(&mask) = self.scope_flavors.get(&si) {
                return mask;
            }
            if si < self.ir.scopes.len() {
                current = self.ir.scopes[si].parent;
            } else {
                break;
            }
        }
        self.project_flavors
    }

    /// Narrow the active flavor set in `scope_idx` to the intersection of
    /// `new_mask` with whatever is already active. Used by flavor guards.
    pub(crate) fn narrow_scope_flavors(&mut self, scope_idx: ScopeIndex, new_mask: u8) {
        if self.project_flavors == 0 { return; }
        let parent_scope = if scope_idx < self.ir.scopes.len() {
            self.ir.scopes[scope_idx].parent.unwrap_or(scope_idx)
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
        let parent_scope = if scope_idx < self.ir.scopes.len() {
            self.ir.scopes[scope_idx].parent.unwrap_or(scope_idx)
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
        }
    }
}
