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

use crate::ast::Block;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::{SyntaxTree, NodeId};
use crate::types::*;
use crate::config::AllowedGlobals;
use crate::pre_globals::PreResolvedGlobals;

// ── Call-site self_offset ───────────────────────────────────────────────────

pub(crate) fn call_self_offset(
    is_metamethod_call_func: bool,
    is_other_call_func: bool,
    is_constructor: bool,
    is_method_call: bool,
    has_self: bool,
    has_args: bool,
) -> usize {
    if (is_metamethod_call_func && has_args)
        || (is_other_call_func && has_self)
        || (is_constructor && has_self)
        || (is_method_call && (has_self || has_args)) { 1 } else { 0 }
}

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
    /// Bracket-indexed access sites for `nil-index` diagnostic.
    /// Each entry is (key_expr_id, key_start, key_end) covering both reads and writes.
    pub(crate) bracket_index_sites: Vec<(ExprId, u32, u32)>,
    /// Binary-op sites for `invalid-op` diagnostic.
    /// Each entry is (binary_op_expr_id, start, end) covering arithmetic and concatenation ops.
    pub(crate) binary_op_sites: Vec<(ExprId, u32, u32)>,
    /// Source ranges for local @class declarations (class name → (start, end) byte offsets).
    pub(crate) class_def_ranges: HashMap<String, (u32, u32)>,
    /// Maps @class annotation byte offset → TableIndex for positional disambiguation
    /// when multiple `@class` declarations share the same name in one file.
    pub(crate) class_table_by_offset: HashMap<u32, TableIndex>,
    /// Symbols annotated with `@class` (class definitions). Field assignments on these
    /// symbols define class fields, not inject foreign fields — `inject-field` skips them.
    pub(crate) class_def_symbols: HashSet<SymbolIndex>,
    /// Source ranges for local @alias declarations (alias name → (start, end) byte offsets).
    pub(crate) alias_def_ranges: HashMap<String, (u32, u32)>,
    /// Monotonic counter for ordering scope and version creation. Used to prevent
    /// closure bodies from seeing variable versions created after the closure's scope.
    pub(crate) next_creation_order: u32,
    /// Table index for the `_G` global environment table. Field access on this table
    /// redirects to scope0 symbol lookup. Computed once at analysis construction.
    pub(crate) g_table_idx: Option<TableIndex>,
    pub(crate) field_assignments: Vec<FieldAssignment>,
    pub(crate) call_resolutions: HashMap<ExprId, CallResolution>,
    pub(crate) and_guarded_call_exprs: HashSet<ExprId>,
    pub(crate) and_guarded_flavor_exprs: HashMap<ExprId, u8>,
    pub(crate) and_guarded_nil_check_exprs: HashSet<ExprId>,
    pub(crate) assign_nil_check_bases: Vec<(ExprId, u32, u32)>,
    pub(crate) symbol_type_annotations: HashMap<SymbolIndex, ValueType>,
    /// Scope in which each VarArgs expression was created (for event-param narrowing).
    pub(crate) varargs_scope: HashMap<ExprId, ScopeIndex>,
    /// Display alias for event type parameters. When a symbol's type is `String(None)`
    /// but originated from an event type alias (e.g. `FrameEvent`, `WowEvent`),
    /// this stores the alias name so hover shows `WowEvent` instead of `string`.
    /// Key is (SymbolIndex, version_index).
    pub(crate) event_type_display: HashMap<(SymbolIndex, usize), String>,
    /// Per-file override for the addon namespace table. When set (multi-addon
    /// workspace), this file uses its own addon's table instead of the global
    /// `ext.addon_table_idx`. Set via `AnalysisConfig::addon_table_override`.
    pub(crate) addon_table_override: Option<TableIndex>,
    /// Maps string-literal ExprIds to their `expression<C, R>` context.
    /// Populated during call resolution when a string arg matches an
    /// `expression<C, R>` parameter annotation.
    pub(crate) expression_args: HashMap<ExprId, ExpressionArg>,
}

/// Metadata for a string literal argument annotated as `expression<C, R>`.
#[derive(Debug, Clone)]
pub(crate) struct ExpressionArg {
    /// Table indices whose fields are the expression's variables.
    /// Multiple indices when C is an intersection type (e.g. `expression<State & Builtins>`).
    pub table_idxs: Vec<TableIndex>,
    /// Optional expected return type from the `R` parameter.
    pub return_type: Option<ValueType>,
    /// Source range `(start, end)` of the string literal in the file.
    pub str_range: (u32, u32),
}

impl Ir {
    /// Get the addon namespace table index for this file. Prefers the per-file
    /// override (set when multi-addon workspace isolation is active), falling
    /// back to the global addon table from `PreResolvedGlobals`.
    #[inline]
    pub(crate) fn addon_table_idx(&self) -> Option<TableIndex> {
        self.addon_table_override.or(self.ext.addon_table_idx)
    }

    /// Check if a table index represents the `_G` global environment table.
    /// Matches both the external `_G` symbol's table and per-file `@class _G`
    /// overlay tables that shadow it.
    #[inline]
    pub(crate) fn is_global_env(&self, table_idx: TableIndex) -> bool {
        self.g_table_idx == Some(table_idx)
            || self.table(table_idx).class_name.as_deref() == Some("_G")
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
        self.get_symbol_impl(id, scope_idx, None)
    }

    /// Like `get_symbol` but skips a specific symbol index. Used when the
    /// query position is on the RHS of a `local x = x` statement so the
    /// freshly-defined local is bypassed in favor of the outer/global binding.
    /// `exclude` is always a local symbol (never external), so the external
    /// scope0 fallback paths don't need the check.
    pub(crate) fn get_symbol_excluding(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex, exclude: SymbolIndex) -> Option<SymbolIndex> {
        self.get_symbol_impl(id, scope_idx, Some(exclude))
    }

    fn get_symbol_impl(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex, exclude: Option<SymbolIndex>) -> Option<SymbolIndex> {
        let mut scope_idx = Some(scope_idx);
        while let Some(si) = scope_idx {
            let scope_obj = if si.is_external() {
                self.ext.scopes.get(si.ext_offset())?
            } else {
                self.scopes.get(si.val())?
            };
            if let Some(&sym) = scope_obj.symbols.get(id)
                && exclude != Some(sym) {
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
            && !t.enum_kind.is_enum()
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
            original_type_source: None,
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
            original_type_source: None,
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
            original_type_source: None,
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
            original_type_source: None,
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
                flavor_guard: 0,
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
            original_type_source: None,
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
        // In Lua, assignment without `local` always creates a global, regardless
        // of nesting depth, so register the symbol in file scope (scope 0).
        let global_scope = ScopeIndex(0);
        self.symbols.push(Symbol {
            id: id.clone(),
            scope_idx: global_scope,
            versions: vec![version],
            flavor_guard: 0,
        });
        let symbol_idx = SymbolIndex(self.symbols.len() - 1);
        self.scopes.get_mut(global_scope.val()).unwrap().symbols.insert(id, symbol_idx);
        symbol_idx
    }

    pub(super) fn set_type_source(&mut self, symbol_idx: SymbolIndex, expr_id: ExprId) {
        let symbol = &mut self.symbols[symbol_idx.val()];
        let version = symbol.versions.last_mut().expect("symbol must have at least one version");
        if version.type_source.is_some() && version.original_type_source.is_none() {
            version.original_type_source = version.type_source;
        }
        version.type_source = Some(expr_id);
    }

    /// Resolve `@class` from preceding annotations or an inline `---@class` comment.
    /// Returns `(class_name, class_table_idx)` using offset-based disambiguation
    /// when multiple `@class` declarations share the same name.
    pub(super) fn resolve_class_annotation(
        &self,
        class: &Option<String>,
        class_comment_start: Option<u32>,
        assign_syntax: crate::syntax::SyntaxNode<'_>,
    ) -> Option<(String, TableIndex)> {
        let (name, offset) = if let Some(name) = class {
            (name.clone(), class_comment_start)
        } else if let Some((name, offset)) = crate::annotations::extract_inline_class_with_offset(assign_syntax) {
            (name, Some(offset))
        } else {
            return None;
        };
        let table_idx = offset
            .and_then(|off| self.class_table_by_offset.get(&off).copied())
            .or_else(|| self.classes.get(&name).copied())?;
        Some((name, table_idx))
    }

    pub(super) fn find_table_for_symbol(&self, root_name: &str, scope_idx: ScopeIndex) -> Option<TableIndex> {
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name.to_string()), scope_idx)?;
        let ver_idx = self.version_for_scope(symbol_idx, scope_idx);
        let ver = &self.sym(symbol_idx).versions[ver_idx];
        if let Some(type_source) = ver.type_source {
            self.find_table_index(type_source)
        } else {
            // External symbols may not have type_source but have resolved_type
            match &ver.resolved_type {
                Some(ValueType::Table(Some(idx))) => Some(*idx),
                _ => None,
            }
        }
    }

    pub(super) fn find_table_index(&self, expr_id: ExprId) -> Option<TableIndex> {
        match self.expr(expr_id) {
            Expr::TableConstructor(idx) => Some(*idx),
            Expr::Literal(ValueType::Table(Some(idx))) => Some(*idx),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let sym_idx = *sym_idx;
                let ver_idx = *ver_idx;
                let ver = &self.sym(sym_idx).versions[ver_idx];
                if let Some(type_source) = ver.type_source {
                    self.find_table_index(type_source)
                } else {
                    // External symbols may not have type_source but have resolved_type
                    match &ver.resolved_type {
                        Some(ValueType::Table(Some(idx))) => Some(*idx),
                        _ => None,
                    }
                }
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
                Expr::BracketIndex { table, literal_key: Some(key), .. } => {
                    fields.push(key.clone());
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
    pub(crate) fn resolve_constructor_func(&self, table_idx: TableIndex) -> Option<FunctionIndex> {
        let ctor_name = self.table(table_idx).constructors.iter().next().cloned()
            .or_else(|| {
                self.table(table_idx).parent_classes.clone().iter()
                    .find_map(|&p| self.table(p).constructors.iter().next().cloned())
            })?;
        let field = self.get_field(table_idx, &ctor_name)
            .or_else(|| self.table(table_idx).parent_classes.clone().iter()
                .find_map(|&p| self.get_field(p, &ctor_name)))?;
        if let Expr::FunctionDef(fi) = self.expr(field.expr) {
            Some(*fi)
        } else {
            None
        }
    }

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

    /// Check if a table or any of its ancestors has the given accessor.
    pub(crate) fn has_accessor(&self, table_idx: TableIndex, name: &str) -> bool {
        self.get_accessor(table_idx, name).is_some()
    }

    /// Get accessor visibility from a table or its ancestors (recursive).
    pub(crate) fn get_accessor(&self, table_idx: TableIndex, name: &str) -> Option<crate::annotations::Visibility> {
        let mut visited = HashSet::new();
        self.get_accessor_recursive(table_idx, name, &mut visited)
    }

    fn get_accessor_recursive(&self, table_idx: TableIndex, name: &str, visited: &mut HashSet<TableIndex>) -> Option<crate::annotations::Visibility> {
        if !visited.insert(table_idx) {
            return None;
        }
        if let Some(&vis) = self.table(table_idx).accessors.get(name) {
            return Some(vis);
        }
        for &parent_idx in &self.table(table_idx).parent_classes {
            if let Some(vis) = self.get_accessor_recursive(parent_idx, name, visited) {
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
                    Some((best_len, _)) if len <= best_len => {
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

    /// Check annotation type names for undefined types. Pure read-only check
    /// usable from both Analysis (build phase) and AnalysisResult (check phase).
    pub(crate) fn check_annotation_type_names(
        &self,
        at: &crate::annotations::AnnotationType,
        generics: &[(String, Option<String>)],
        start: usize,
        end: usize,
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
    ) {
        use crate::annotations::AnnotationType;
        match at {
            AnnotationType::Simple(name) => {
                if generics.iter().any(|(g, _)| g == name) { return; }
                if generics.iter().any(|(_, c)| c.as_deref() == Some(name.as_str())) { return; }
                match name.as_str() {
                    "nil" | "boolean" | "bool" | "number" | "integer"
                    | "string" | "table" | "function" | "fun" | "any"
                    | "unknown" | "self" | "void" | "true" | "false"
                    | "built" | "..." | "userdata" | "thread" => return,
                    _ => {}
                }
                if name.starts_with("fun(") { return; }
                if let Some(parent) = name.strip_prefix("built:") {
                    self.check_annotation_type_names(
                        &AnnotationType::Simple(parent.to_string()),
                        generics, start, end, diags,
                    );
                    return;
                }
                if (name.starts_with('"') && name.ends_with('"'))
                    || (name.starts_with('\'') && name.ends_with('\''))
                { return; }
                if name.bytes().all(|b| b.is_ascii_digit()) && !name.is_empty() { return; }
                if self.classes.contains_key(name.as_str()) { return; }
                if self.aliases.contains_key(name.as_str()) { return; }
                if self.parameterized_aliases.contains_key(name.as_str()) { return; }
                if self.tuple_form_aliases.contains_key(name.as_str()) { return; }
                if self.ext.classes.contains_key(name.as_str()) { return; }
                if self.ext.aliases.contains_key(name.as_str()) { return; }
                if self.ext.parameterized_aliases.contains_key(name.as_str()) { return; }
                if self.ext.tuple_form_aliases.contains_key(name.as_str()) { return; }
                // Comma in type name = malformed `@return`; the malformed-annotation pass handles it.
                if name.contains(',') { return; }
                crate::diagnostics::UNDEFINED_DOC_NAME.emit(
                    diags,
                    format!("undefined type '{}'", name),
                    start,
                    end,
                );
            }
            AnnotationType::Union(parts) | AnnotationType::Intersection(parts) => {
                for p in parts {
                    self.check_annotation_type_names(p, generics, start, end, diags);
                }
            }
            AnnotationType::Array(inner)
            | AnnotationType::Backtick(inner)
            | AnnotationType::NonNil(inner)
            | AnnotationType::VarArgs(inner) => {
                self.check_annotation_type_names(inner, generics, start, end, diags);
            }
            AnnotationType::Parameterized(base, args) => {
                // expression<C, R> is a built-in type; skip the base name check
                // and only validate the type arguments (class name and return type).
                if base == "expression" {
                    for arg in args {
                        self.check_annotation_type_names(arg, generics, start, end, diags);
                    }
                    return;
                }
                if base == "params" || base == "returns" {
                    // params<F> requires exactly 1 generic arg.
                    // returns<F> or returns<F, offset_param> requires 1-2 args;
                    // first must be a declared @generic, second (if present) is a param name.
                    let first_ok = args.first()
                        .is_some_and(|a| matches!(a, AnnotationType::Simple(name) if generics.iter().any(|(g, _)| g == name)));
                    let shape_ok = if base == "returns" {
                        first_ok && (args.len() == 1 || (args.len() == 2 && matches!(&args[1], AnnotationType::Simple(_))))
                    } else {
                        first_ok && args.len() == 1
                    };
                    if !shape_ok {
                        let msg = if base == "returns" {
                            format!("{}<...> projection expects a declared @generic as first arg and an optional param name as second", base)
                        } else {
                            format!("{}<...> projection expects exactly one type-argument that names a declared @generic", base)
                        };
                        crate::diagnostics::MALFORMED_ANNOTATION.emit(
                            diags,
                            msg,
                            start,
                            end,
                        );
                    }
                    return;
                }
                self.check_annotation_type_names(
                    &AnnotationType::Simple(base.clone()), generics, start, end, diags,
                );
                for arg in args {
                    self.check_annotation_type_names(arg, generics, start, end, diags);
                }
            }
            AnnotationType::Fun(params, returns, _) => {
                for p in params {
                    self.check_annotation_type_names(&p.typ, generics, start, end, diags);
                }
                for r in returns {
                    self.check_annotation_type_names(r, generics, start, end, diags);
                }
            }
            AnnotationType::TableLiteral(fields) => {
                for (_, ft) in fields {
                    self.check_annotation_type_names(ft, generics, start, end, diags);
                }
            }
            AnnotationType::Tuple(positions, _) => {
                for p in positions {
                    self.check_annotation_type_names(&p.typ, generics, start, end, diags);
                }
            }
        }
    }
}

// ── Stored analysis output for LSP queries ───────────────────────────────────

/// Stored analysis output for LSP queries. No lifetime — can be persisted in Document.
/// Contains only the fields that query methods actually read.
pub struct AnalysisResult {
    pub(crate) ir: Ir,
    pub(crate) is_meta: bool,
    pub(crate) symbol_version_at: HashMap<u32, usize>,
    pub(crate) resolved_expr_cache: Vec<Option<ValueType>>,
    pub(crate) narrowed_symbols: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) falsy_narrowed_symbols: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) type_narrowed_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    pub(crate) type_filtered_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    pub(crate) type_stripped_symbols: HashMap<ScopeIndex, HashMap<SymbolIndex, ValueType>>,
    pub(crate) call_type_args: HashMap<ExprId, Vec<ValueType>>,
    pub(crate) field_type_args_cache: HashMap<(TableIndex, String), Vec<ValueType>>,
    pub(crate) referenced_symbols: HashSet<SymbolIndex>,
    pub(crate) inherited_constructors: HashSet<FunctionIndex>,
    pub(crate) function_owner_class: HashMap<FunctionIndex, String>,
    pub(crate) allowed_read_globals: AllowedGlobals,
    pub(crate) allowed_write_globals: AllowedGlobals,
    pub(crate) allow_slash_commands: bool,
    pub(crate) defclass_vars: HashMap<String, TableIndex>,
    pub(crate) safety_limit_hit: Option<String>,
    pub(crate) narrowed_fields: HashMap<ScopeIndex, HashSet<(SymbolIndex, Vec<String>)>>,
    pub(crate) type_narrowed_fields: HashMap<ScopeIndex, HashMap<(SymbolIndex, Vec<String>), ValueType>>,
    pub(crate) narrowing_overridden: HashMap<ScopeIndex, HashMap<SymbolIndex, u32>>,
    pub(crate) explicit_globals: HashSet<String>,
    pub(crate) scope_flavors: HashMap<ScopeIndex, u8>,
    pub(crate) project_flavors: u8,
    pub(crate) event_vararg_types: HashMap<ScopeIndex, Vec<ValueType>>,
    pub(crate) vararg_user_annotated_fns: HashSet<FunctionIndex>,
    /// Diagnostic codes declared by loaded plugins (suppresses `unknown-diag-code`).
    pub plugin_diag_codes: Vec<String>,
}

impl AnalysisResult {
    // ── Delegators for two-tier lookups ──────────────────────────────────────

    #[inline] pub(crate) fn sym(&self, idx: SymbolIndex) -> &Symbol { self.ir.sym(idx) }
    #[inline] pub(crate) fn func(&self, idx: FunctionIndex) -> &Function { self.ir.func(idx) }
    #[inline] pub(crate) fn expr(&self, idx: ExprId) -> &Expr { self.ir.expr(idx) }
    #[inline] pub(crate) fn table(&self, idx: TableIndex) -> &TableInfo { self.ir.table(idx) }
    #[inline] pub(crate) fn get_symbol(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex) -> Option<SymbolIndex> { self.ir.get_symbol(id, scope_idx) }
    #[inline] pub(crate) fn get_symbol_excluding(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex, exclude: SymbolIndex) -> Option<SymbolIndex> { self.ir.get_symbol_excluding(id, scope_idx, exclude) }
    #[inline] pub(crate) fn get_field(&self, table_idx: TableIndex, field_name: &str) -> Option<&FieldInfo> { self.ir.get_field(table_idx, field_name) }
    #[inline] pub(crate) fn scope_at_offset(&self, offset: impl Into<u32>) -> Option<ScopeIndex> { self.ir.scope_at_offset(offset) }
    #[inline] pub(crate) fn same_class(&self, a: TableIndex, b: TableIndex) -> bool { self.ir.same_class(a, b) }
    #[inline] pub(crate) fn is_subclass_of(&self, child_idx: TableIndex, parent_idx: TableIndex) -> bool { self.ir.is_subclass_of(child_idx, parent_idx) }
    #[inline] pub(crate) fn find_enclosing_class(&self, node: &SyntaxNode<'_>) -> Option<TableIndex> { self.ir.find_enclosing_class(node) }
    #[inline] pub(crate) fn function_name(&self, func_idx: FunctionIndex) -> Option<String> { self.ir.function_name(func_idx) }

    pub fn is_meta(&self) -> bool {
        self.is_meta
    }

    #[inline] pub(crate) fn resolve_constructor_func(&self, table_idx: TableIndex) -> Option<FunctionIndex> { self.ir.resolve_constructor_func(table_idx) }

    pub(crate) fn resolve_class_constraint(&self, constraint_str: &str) -> Option<ValueType> {
        let parsed = crate::annotations::parse_type(constraint_str);
        self.resolve_annotation_type_simple(&parsed)
    }

    fn resolve_annotation_type_simple(&self, at: &crate::annotations::AnnotationType) -> Option<ValueType> {
        match at {
            crate::annotations::AnnotationType::Simple(name) => {
                match name.as_str() {
                    "number" | "integer" => Some(ValueType::Number),
                    "string" => Some(ValueType::String(None)),
                    "boolean" => Some(ValueType::Boolean(None)),
                    "table" => Some(ValueType::Table(None)),
                    "function" => Some(ValueType::Function(None)),
                    "any" => Some(ValueType::Any),
                    "nil" => Some(ValueType::Nil),
                    _ => {
                        let table_idx = self.ir.classes.get(name.as_str())
                            .or_else(|| self.ir.ext.classes.get(name.as_str()))
                            .copied()?;
                        Some(ValueType::Table(Some(table_idx)))
                    }
                }
            }
            crate::annotations::AnnotationType::Union(members) => {
                let resolved: Vec<ValueType> = members.iter()
                    .filter_map(|m| self.resolve_annotation_type_simple(m))
                    .collect();
                if resolved.len() != members.len() { return None; }
                Some(ValueType::Union(resolved))
            }
            _ => None,
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

    pub(crate) fn get_type_filtering(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        scope_map_get(&self.type_filtered_symbols, &self.ir.scopes, &sym_idx, scope_idx)
    }

    pub(crate) fn get_type_stripping(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<&ValueType> {
        scope_map_get(&self.type_stripped_symbols, &self.ir.scopes, &sym_idx, scope_idx)
    }

    pub(crate) fn get_field_type_narrowing(&self, sym_idx: SymbolIndex, chain: &[String], scope_idx: ScopeIndex) -> Option<&ValueType> {
        let key = (sym_idx, chain.to_vec());
        scope_map_get(&self.type_narrowed_fields, &self.ir.scopes, &key, scope_idx)
    }

    pub(crate) fn is_field_chain_narrowed(&self, sym_idx: SymbolIndex, fields: &[String], scope_idx: ScopeIndex) -> bool {
        Self::check_field_set(&self.narrowed_fields, sym_idx, fields, scope_idx, &self.ir.scopes)
    }

    /// Position-aware override check: returns true only if the override was set at or before `at_offset`.
    /// However, if any scope between the override and the querying scope (exclusive of
    /// the override scope, inclusive of the querying scope) has a fresh narrowing entry
    /// for the symbol, the override doesn't apply — the new guard re-establishes
    /// narrowing after the reassignment.
    pub(crate) fn is_narrowing_overridden_at(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex, at_offset: u32) -> bool {
        let override_scope = ancestor_scopes(&self.ir.scopes, scope_idx)
            .find(|si| self.narrowing_overridden.get(si)
                .and_then(|m| m.get(&sym_idx))
                .is_some_and(|&off| off <= at_offset));
        let Some(override_scope) = override_scope else { return false; };
        if override_scope == scope_idx { return true; }
        // Override is in a strict ancestor. Check if any scope between here and the
        // override has a fresh narrowing entry (e.g. from a new `if x then` or
        // `type(x) == "t"` guard).
        for si in ancestor_scopes(&self.ir.scopes, scope_idx) {
            if si == override_scope { break; }
            if self.narrowed_symbols.get(&si).is_some_and(|s| s.contains(&sym_idx))
                || self.falsy_narrowed_symbols.get(&si).is_some_and(|s| s.contains(&sym_idx))
                || self.type_narrowed_symbols.get(&si).is_some_and(|m| m.contains_key(&sym_idx))
                || self.type_narrowed_fields.get(&si).is_some_and(|m| m.keys().any(|(s, _)| *s == sym_idx)) {
                return false;
            }
        }
        true
    }

    pub(crate) fn active_flavors_at(&self, scope_idx: ScopeIndex) -> u8 {
        if self.project_flavors == 0 { return 0; }
        ancestor_scopes(&self.ir.scopes, scope_idx)
            .find_map(|si| self.scope_flavors.get(&si).copied())
            .unwrap_or(self.project_flavors)
    }

    pub(crate) fn suppress_inject_field_on_g(&self, class_name: &str, field_name: &str, scope_idx: ScopeIndex) -> bool {
        if class_name != "_G" { return false; }
        if self.allowed_read_globals.contains(field_name)
            || self.allowed_write_globals.contains(field_name) {
            return true;
        }
        self.ir.get_symbol(&SymbolIdentifier::Name(field_name.to_string()), scope_idx).is_some()
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
    pub(crate) deep_field_injections: Vec<DeepFieldInjection>,
    pub(crate) deferred_field_assignments: Vec<DeferredFieldAssignment>,
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
    /// Maps to the byte offset of the reassignment node.
    pub(crate) narrowing_overridden: HashMap<ScopeIndex, HashMap<SymbolIndex, u32>>,
    pub(crate) referenced_symbols: HashSet<SymbolIndex>,
    pub(crate) functions_with_returns: HashSet<FunctionIndex>,
    /// Dense cycle-detection bitmap for `resolve_expr`, indexed by `ExprId.val()`.
    /// Local expressions only (< EXT_BASE); external ones resolve via fast paths.
    pub(crate) resolving_exprs: Vec<bool>,
    pub(crate) resolve_depth: usize,
    pub(crate) resolve_work_count: usize,
    /// Dense cache for resolved expression types, indexed by `ExprId.val()`.
    /// Only caches local expressions (< EXT_BASE); external expressions resolve
    /// through fast paths (Literal, FunctionDef) and skip the cache.
    /// `None` = not yet cached; `Some(vt)` = cached resolved type.
    pub(crate) resolved_expr_cache: Vec<Option<ValueType>>,
    /// Set by `substitute_generics_deep` when a projection (returns<F>/params<F>)
    /// can't be resolved because the bound F's return type isn't available yet.
    /// Signals to the caller that the result is incomplete and should be retried.
    pub(crate) projection_deferred: bool,
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
    /// Generic substitutions computed at each call site (keyed by func_expr).
    /// Used by `resolve_overload_narrow` to substitute implicit generics
    /// (pass-through param TypeVariables) during sibling narrowing.
    pub(crate) call_site_generic_subs: HashMap<ExprId, HashMap<String, ValueType>>,
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
    /// Deferred event-param narrowings: (event_sym, string_literal, target_scope).
    /// Stored during build_ir when `event == "STRING"` is detected, processed during
    /// resolve after event_params has been propagated from overload contextual typing.
    pub(crate) deferred_event_narrowings: Vec<(SymbolIndex, String, ScopeIndex)>,
    /// Groups of local variables that are always assigned together in if/elseif branches.
    /// When one is narrowed via nil guard, others should be narrowed too.
    pub(crate) correlated_locals: Vec<Vec<SymbolIndex>>,
    /// Asymmetric narrowing: when the key symbol is narrowed non-nil, every derived
    /// symbol in the value list is also narrowed. Populated from `x = x or y`
    /// assignments — if `y` is known non-nil, `x` (just assigned `x or y`) is too.
    /// One-directional: narrowing `x` does NOT imply anything about `y`.
    pub(crate) or_coalesce_derivations: HashMap<SymbolIndex, Vec<SymbolIndex>>,
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
    pub(crate) allowed_read_globals: AllowedGlobals,
    pub(crate) allowed_write_globals: AllowedGlobals,
    pub(crate) allow_slash_commands: bool,
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
    pub(crate) explicit_globals: HashSet<String>,
    pub(crate) implicit_protected_prefix: bool,
    /// Functions detected as inherited constructors (e.g. `__init` on a class
    /// that declares `@constructor __init`) but not explicitly `@constructor`.
    /// Used by post-resolution `constructor_return` diagnostic check.
    pub(crate) inherited_constructors: HashSet<FunctionIndex>,
    /// Maps function index → owning class name for methods defined with colon
    /// syntax on a `@class` table. Used by post-resolution `builds_field_not_self`
    /// and `return_self_class_name` checks.
    pub(crate) function_owner_class: HashMap<FunctionIndex, String>,
    pub(crate) is_meta: bool,
    /// Set when a safety limit is hit during resolution (iteration cap, table cap, depth cap).
    pub(crate) safety_limit_hit: Option<String>,
    /// Event-param narrowing: when an event param is narrowed to a string literal,
    /// per-position vararg types from the event payload are stored here.
    pub(crate) event_vararg_types: HashMap<ScopeIndex, Vec<ValueType>>,
    /// Functions whose `vararg_annotation` came from a user-written `@param ...`.
    /// Used to suppress redundant inlay hints (contextual propagation should show hints,
    /// but user annotations shouldn't be duplicated).
    pub(crate) vararg_user_annotated_fns: HashSet<FunctionIndex>,
}

/// Per-file analysis configuration bundling project-level settings.
pub struct AnalysisConfig {
    pub framexml_enabled: bool,
    pub allowed_read_globals: AllowedGlobals,
    pub allowed_write_globals: AllowedGlobals,
    pub allow_slash_commands: bool,
    pub project_flavors: u8,
    pub backward_param_types: bool,
    pub correlated_return_overloads: bool,
    pub implicit_protected_prefix: bool,
    /// Per-file addon namespace table override for multi-addon workspaces.
    /// When set, this file sees only its own addon's namespace table.
    pub addon_table_override: Option<TableIndex>,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            framexml_enabled: true,
            allowed_read_globals: AllowedGlobals::default(),
            allowed_write_globals: AllowedGlobals::default(),
            allow_slash_commands: true,
            project_flavors: 0,
            backward_param_types: true,
            correlated_return_overloads: true,
            implicit_protected_prefix: false,
            addon_table_override: None,
        }
    }
}

impl<'a> Analysis<'a> {
    /// Create a new Analysis from a pre-parsed tree.
    pub fn new_with_tree(
        tree: &'a SyntaxTree,
        pre_globals: Arc<PreResolvedGlobals>,
        config: AnalysisConfig,
    ) -> Analysis<'a> {
        let AnalysisConfig {
            framexml_enabled,
            allowed_read_globals,
            allowed_write_globals,
            allow_slash_commands,
            project_flavors,
            backward_param_types,
            correlated_return_overloads,
            implicit_protected_prefix,
            addon_table_override,
        } = config;

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
                bracket_index_sites: Vec::new(),
                binary_op_sites: Vec::new(),
                class_def_ranges: HashMap::new(),
                class_table_by_offset: HashMap::new(),
                class_def_symbols: HashSet::new(),
                alias_def_ranges: HashMap::new(),
                next_creation_order: 0,
                g_table_idx,
                field_assignments: Vec::new(),
                call_resolutions: HashMap::new(),
                and_guarded_call_exprs: HashSet::new(),
                and_guarded_flavor_exprs: HashMap::new(),
                and_guarded_nil_check_exprs: HashSet::new(),
                assign_nil_check_bases: Vec::new(),
                symbol_type_annotations: HashMap::new(),
                varargs_scope: HashMap::new(),
                event_type_display: HashMap::new(),
                addon_table_override,
                expression_args: HashMap::new(),
            },
            deep_field_injections: Vec::new(),
            deferred_field_assignments: Vec::new(),
            referenced_symbols: HashSet::new(),
            functions_with_returns: HashSet::new(),
            resolving_exprs: Vec::new(),
            resolve_depth: 0,
            resolve_work_count: 0,
            resolved_expr_cache: Vec::new(),
            projection_deferred: false,
            builder_call_memo: HashMap::new(),
            call_type_args: HashMap::new(),
            call_site_generic_subs: HashMap::new(),
            field_type_args_cache: HashMap::new(),
            multi_return_siblings: HashMap::new(),
            deferred_sibling_narrowings: Vec::new(),
            deferred_class_eq_narrowings: Vec::new(),
            deferred_event_narrowings: Vec::new(),
            correlated_locals: Vec::new(),
            or_coalesce_derivations: HashMap::new(),
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
            allow_slash_commands,
            project_flavors,
            scope_flavors: HashMap::new(),
            backward_param_types,
            correlated_return_overloads,
            explicit_globals: HashSet::new(),
            implicit_protected_prefix,
            inherited_constructors: HashSet::new(),
            function_owner_class: HashMap::new(),
            is_meta: false,
            safety_limit_hit: None,
            event_vararg_types: HashMap::new(),
            vararg_user_annotated_fns: HashSet::new(),
        };
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

    // ── Forwarding stubs for methods now living on AnalysisResult ────────────
    // These are called by resolve.rs, resolve_call.rs, build_ir.rs, and narrowing.rs
    // during the mutable Analysis phase.

    pub(super) fn is_table_subtype(&self, actual: &ValueType, expected: &ValueType) -> bool {
        is_table_subtype_impl(&self.ir, &self.resolved_expr_cache[..], actual, expected)
    }

    pub(super) fn type_involves_type_variable(&self, vt: &ValueType) -> bool {
        type_involves_type_variable_impl(&self.ir, vt)
    }

    pub(super) fn class_has_field(&self, table_idx: TableIndex, field_name: &str) -> bool {
        class_has_field_impl(&self.ir, table_idx, field_name)
    }

    pub(super) fn block_always_exits(block: &Block) -> bool {
        AnalysisResult::block_always_exits(block)
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

    pub(crate) fn is_narrowing_overridden(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> bool {
        let override_scope = ancestor_scopes(&self.ir.scopes, scope_idx)
            .find(|si| self.narrowing_overridden.get(si).is_some_and(|m| m.contains_key(&sym_idx)));
        let Some(override_scope) = override_scope else { return false; };
        if override_scope == scope_idx { return true; }
        // Override is in a strict ancestor. Check if any scope between here and the
        // override has a fresh narrowing entry (e.g. from a new `if x then` or
        // `type(x) == "t"` guard).
        for si in ancestor_scopes(&self.ir.scopes, scope_idx) {
            if si == override_scope { break; }
            if self.narrowed_symbols.get(&si).is_some_and(|s| s.contains(&sym_idx))
                || self.falsy_narrowed_symbols.get(&si).is_some_and(|s| s.contains(&sym_idx))
                || self.type_narrowed_symbols.get(&si).is_some_and(|m| m.contains_key(&sym_idx))
                || self.type_narrowed_fields.get(&si).is_some_and(|m| m.keys().any(|(s, _)| *s == sym_idx)) {
                return false;
            }
        }
        true
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

    pub fn into_result(self) -> AnalysisResult {
        AnalysisResult {
            ir: self.ir,
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
            referenced_symbols: self.referenced_symbols,
            inherited_constructors: self.inherited_constructors,
            function_owner_class: self.function_owner_class,
            allowed_read_globals: self.allowed_read_globals,
            allowed_write_globals: self.allowed_write_globals,
            allow_slash_commands: self.allow_slash_commands,
            defclass_vars: self.defclass_vars,
            safety_limit_hit: self.safety_limit_hit,
            narrowed_fields: self.narrowed_fields,
            type_narrowed_fields: self.type_narrowed_fields,
            narrowing_overridden: self.narrowing_overridden,
            explicit_globals: self.explicit_globals,
            scope_flavors: self.scope_flavors,
            project_flavors: self.project_flavors,
            event_vararg_types: self.event_vararg_types,
            vararg_user_annotated_fns: self.vararg_user_annotated_fns,
            plugin_diag_codes: Vec::new(),
        }
    }
}

// ── Free functions shared by Analysis and AnalysisResult ──────────────────────
// These implement the core logic for subtype checking, type-variable detection,
// and class field lookup. Both `Analysis` (mutable phase) and `AnalysisResult`
// (immutable diagnostic/query phase) delegate to these.

pub(crate) fn type_involves_type_variable_impl(ir: &Ir, vt: &ValueType) -> bool {
    match vt {
        ValueType::TypeVariable(_) => true,
        ValueType::Table(Some(idx)) => {
            let table = ir.table(*idx);
            table.value_type.as_ref().is_some_and(|v| type_involves_type_variable_impl(ir, v))
                || table.key_type.as_ref().is_some_and(|k| type_involves_type_variable_impl(ir, k))
        }
        ValueType::Union(types) => types.iter().any(|t| type_involves_type_variable_impl(ir, t)),
        _ => false,
    }
}

pub(crate) fn class_has_field_impl(ir: &Ir, table_idx: TableIndex, field_name: &str) -> bool {
    let mut to_check = vec![table_idx];
    let mut visited = std::collections::HashSet::new();
    while let Some(idx) = to_check.pop() {
        if !visited.insert(idx) { continue; }
        let table = ir.table(idx);
        if table.fields.contains_key(field_name) { return true; }
        if let Some(bt) = table.built_table
            && ir.table(bt).fields.contains_key(field_name) { return true; }
        to_check.extend_from_slice(&table.parent_classes);
    }
    false
}

pub(crate) fn is_table_subtype_impl(
    ir: &Ir,
    resolved_expr_cache: &[Option<ValueType>],
    actual: &ValueType,
    expected: &ValueType,
) -> bool {
    match (actual, expected) {
        // Opaque aliases: different names are never assignable
        (ValueType::OpaqueAlias(a, _), ValueType::OpaqueAlias(b, _)) if a != b => false,
        // Unwrap opaques and delegate to inner type
        (ValueType::OpaqueAlias(_, inner), exp) => is_table_subtype_impl(ir, resolved_expr_cache, inner, exp),
        (act, ValueType::OpaqueAlias(_, inner)) => is_table_subtype_impl(ir, resolved_expr_cache, act, inner),
        // Number enum <-> number: @enum types with numeric values are integers at runtime
        (ValueType::Table(Some(a)), ValueType::Number) if ir.table(*a).enum_kind == EnumKind::Number => true,
        (ValueType::Number, ValueType::Table(Some(b))) if ir.table(*b).enum_kind == EnumKind::Number => true,
        // String enum <-> string: @enum types with string values are strings at runtime.
        // Uses String(None) which matches any string; string literals currently always
        // lower to String(None) in lower_expression.rs so this covers all cases.
        (ValueType::Table(Some(a)), ValueType::String(None)) if ir.table(*a).enum_kind == EnumKind::String => true,
        (ValueType::String(None), ValueType::Table(Some(b))) if ir.table(*b).enum_kind == EnumKind::String => true,
        (ValueType::Table(Some(a)), ValueType::Table(Some(b))) => {
            if ir.is_subclass_of(*a, *b) { return true; }
            let at = ir.table(*a);
            let bt = ir.table(*b);
            // Enum-like value-type compatibility: when the expected type is a class
            // with @field [string] V (directly or inherited), actual type V is
            // considered assignable. This is an intentional loosening for the common
            // enum pattern where @param x MyEnum accepts MyEnum.MEMBER (which
            // resolves to EnumValue). It also loosens container patterns like
            // @class Pool with @field [string] Widget — a Widget would be accepted
            // where a Pool is expected — but this is acceptable since the pattern
            // is almost exclusively used for enum-like types in practice.
            // Uses is_subclass_of (not recursive is_table_subtype_impl) to avoid
            // infinite recursion from circular value_type chains.
            if bt.class_name.is_some() {
                let vt = bt.value_type.as_ref().or_else(|| {
                    bt.parent_classes.iter()
                        .find_map(|&p| ir.table(p).value_type.as_ref())
                });
                if let Some(vt) = vt
                    && (actual.is_assignable_to(vt)
                        || matches!((actual, vt), (ValueType::Table(Some(a_idx)), ValueType::Table(Some(v_idx))) if ir.is_subclass_of(*a_idx, *v_idx)))
                {
                    return true;
                }
            }
            if at.class_name.is_none() && bt.class_name.is_some() && !at.fields.is_empty()
                && fields_structurally_match_impl(ir, resolved_expr_cache, *a, *b) {
                    return true;
                }
            if at.class_name.is_some() && bt.class_name.is_none()
                && let (Some(bk), Some(bv)) = (&bt.key_type, &bt.value_type)
            {
                if matches!(bk, ValueType::TypeVariable(_)) || matches!(bv, ValueType::TypeVariable(_)) {
                    return true;
                }
                let (ak, av) = if at.key_type.is_some() {
                    (at.key_type.clone(), at.value_type.clone())
                } else if !at.fields.is_empty() {
                    let field_types: Vec<ValueType> = at.fields.values()
                        .filter_map(|f| f.annotation.clone().or_else(|| {
                            match ir.expr(f.expr) {
                                Expr::Literal(vt) => Some(vt.clone()),
                                Expr::FunctionDef(idx) => Some(ValueType::Function(Some(*idx))),
                                Expr::TableConstructor(idx) => Some(ValueType::Table(Some(*idx))),
                                _ => resolved_expr_cache.get(f.expr.val()).and_then(|v| v.clone()),
                            }
                        }))
                        .collect();
                    (Some(ValueType::String(None)), Analysis::union_of(field_types))
                } else {
                    (None, None)
                };
                if let (Some(ak), Some(av)) = (&ak, &av) {
                    return (ak.is_assignable_to(bk) || is_table_subtype_impl(ir, resolved_expr_cache, ak, bk))
                        && (av.is_assignable_to(bv) || is_table_subtype_impl(ir, resolved_expr_cache, av, bv));
                }
            }
            if at.class_name.is_none() && bt.class_name.is_none() {
                if bt.key_type.is_none() && bt.value_type.is_none()
                    && !bt.fields.values().any(|f| f.annotation_type_raw.is_some()) {
                    return true;
                }
                if at.key_type.is_none() && at.value_type.is_none()
                    && at.array_fields.is_empty()
                    && bt.key_type.is_some()
                {
                    return true;
                }
                let (ak, av) = if at.key_type.is_some() {
                    (at.key_type.clone(), at.value_type.clone())
                } else if !at.array_fields.is_empty() {
                    let mut types: Vec<ValueType> = Vec::new();
                    let mut resolved_count = 0usize;
                    for &field_expr in &at.array_fields {
                        let vt = match ir.expr(field_expr) {
                            Expr::Literal(vt) => Some(vt.clone()),
                            _ => resolved_expr_cache.get(field_expr.val())
                                .and_then(|v| v.clone()),
                        };
                        if let Some(vt) = vt {
                            resolved_count += 1;
                            if !types.contains(&vt) {
                                types.push(vt);
                            }
                        }
                    }
                    if resolved_count < at.array_fields.len() {
                        return true;
                    }
                    (Some(ValueType::Number), Analysis::union_of(types))
                } else {
                    (None, None)
                };
                if let (Some(ak), Some(av), Some(bk), Some(bv)) =
                    (&ak, &av, &bt.key_type, &bt.value_type)
                {
                    return (ak.is_assignable_to(bk) || is_table_subtype_impl(ir, resolved_expr_cache, ak, bk))
                        && (av.is_assignable_to(bv) || is_table_subtype_impl(ir, resolved_expr_cache, av, bv));
                }
                if !bt.fields.is_empty()
                    && fields_structurally_match_impl(ir, resolved_expr_cache, *a, *b) {
                        return true;
                    }
            }
            false
        }
        (ValueType::Table(Some(_)) | ValueType::Number, ValueType::Union(types)) => {
            types.iter().any(|t| is_table_subtype_impl(ir, resolved_expr_cache, actual, t))
        }
        (ValueType::Intersection(actuals), ValueType::Intersection(expecteds)) => {
            expecteds.iter().all(|e| actuals.iter().any(|a|
                a.is_assignable_to(e) || is_table_subtype_impl(ir, resolved_expr_cache, a, e)))
        }
        (ValueType::Intersection(types), expected) => {
            types.iter().any(|t| t.is_assignable_to(expected) || is_table_subtype_impl(ir, resolved_expr_cache, t, expected))
        }
        (actual, ValueType::Intersection(types)) => {
            types.iter().all(|t| actual.is_assignable_to(t) || is_table_subtype_impl(ir, resolved_expr_cache, actual, t))
        }
        (ValueType::Union(types), expected) => {
            // When the union contains both hash-map tables (non-number keys) and
            // array-compatible members, tolerate the hash-map members. In Lua,
            // hash entries and sequential entries coexist on the same table, so
            // `table<K,V>|T[]` passed as an array param is valid.
            let expected_is_array = matches!(expected, ValueType::Table(Some(idx)) if {
                let et = ir.table(*idx);
                et.key_type.as_ref() == Some(&ValueType::Number) || !et.array_fields.is_empty()
            });
            let has_array_compatible = expected_is_array && types.iter().any(|t|
                t.is_assignable_to(expected) || is_table_subtype_impl(ir, resolved_expr_cache, t, expected));
            types.iter().all(|t| {
                if t.is_assignable_to(expected) || is_table_subtype_impl(ir, resolved_expr_cache, t, expected) {
                    return true;
                }
                if has_array_compatible
                    && let ValueType::Table(Some(idx)) = t
                {
                    let tbl = ir.table(*idx);
                    if tbl.key_type.is_some()
                        && !matches!(tbl.key_type.as_ref(), Some(k) if k.is_assignable_to(&ValueType::Number))
                    {
                        return true;
                    }
                }
                false
            })
        }
        _ => false,
    }
}

pub(crate) fn fields_structurally_match_impl(
    ir: &Ir,
    resolved_expr_cache: &[Option<ValueType>],
    actual_idx: TableIndex,
    expected_idx: TableIndex,
) -> bool {
    check_fields_impl(ir, resolved_expr_cache, actual_idx, expected_idx).is_empty()
}

pub(crate) fn structural_mismatch_details_impl(
    ir: &Ir,
    resolved_expr_cache: &[Option<ValueType>],
    actual: &ValueType,
    expected: &ValueType,
) -> Option<Vec<StructuralMismatchDetail>> {
    let (actual_idx, expected_idx) = match (actual, expected) {
        (ValueType::Table(Some(a)), ValueType::Table(Some(b))) => (*a, *b),
        _ => return None,
    };
    let at = ir.table(actual_idx);
    let bt = ir.table(expected_idx);
    if at.class_name.is_some() || bt.class_name.is_none() {
        return None;
    }
    let details = check_fields_impl(ir, resolved_expr_cache, actual_idx, expected_idx);
    if details.is_empty() { return None; }
    Some(details)
}

fn check_fields_impl(
    ir: &Ir,
    resolved_expr_cache: &[Option<ValueType>],
    actual_idx: TableIndex,
    expected_idx: TableIndex,
) -> Vec<StructuralMismatchDetail> {
    let expected_fields = collect_class_fields_impl(ir, resolved_expr_cache, expected_idx);
    let at = ir.table(actual_idx);
    let mut details = Vec::new();
    for (field_name, expected_type) in &expected_fields {
        let is_optional = matches!(expected_type, ValueType::Union(types) if types.contains(&ValueType::Nil));
        if let Some(actual_field) = at.fields.get(field_name.as_str()) {
            let actual_type = actual_field.annotation.clone().or_else(|| {
                match ir.expr(actual_field.expr) {
                    Expr::Literal(vt) => Some(vt.clone()),
                    _ => resolved_expr_cache.get(actual_field.expr.val())
                        .and_then(|v| v.clone()),
                }
            });
            if let Some(actual_type) = actual_type
                && actual_type != ValueType::Nil
                && !actual_type.is_assignable_to(expected_type)
                && !is_table_subtype_impl(ir, resolved_expr_cache, &actual_type, expected_type)
            {
                details.push(StructuralMismatchDetail::WrongType {
                    field: field_name.clone(),
                    expected: expected_type.clone(),
                    actual: actual_type,
                });
            }
        } else if !is_optional {
            details.push(StructuralMismatchDetail::Missing { field: field_name.clone() });
        }
    }
    details
}

pub(crate) enum StructuralMismatchDetail {
    Missing { field: String },
    WrongType { field: String, expected: ValueType, actual: ValueType },
}

pub(crate) fn collect_class_fields_impl(
    ir: &Ir,
    resolved_expr_cache: &[Option<ValueType>],
    table_idx: TableIndex,
) -> Vec<(String, ValueType)> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    collect_class_fields_inner_impl(ir, resolved_expr_cache, table_idx, &mut result, &mut visited);
    result
}

fn collect_class_fields_inner_impl(
    ir: &Ir,
    resolved_expr_cache: &[Option<ValueType>],
    table_idx: TableIndex,
    result: &mut Vec<(String, ValueType)>,
    visited: &mut HashSet<TableIndex>,
) {
    if !visited.insert(table_idx) { return; }
    let table = ir.table(table_idx);
    for &parent_idx in &table.parent_classes {
        collect_class_fields_inner_impl(ir, resolved_expr_cache, parent_idx, result, visited);
    }
    if let Some(bt_idx) = table.built_table {
        collect_class_fields_inner_impl(ir, resolved_expr_cache, bt_idx, result, visited);
    }
    for (name, field) in &table.fields {
        if name.starts_with("__") { continue; }
        let field_type = field.annotation.clone().or_else(|| {
            match ir.expr(field.expr) {
                Expr::Literal(vt) => Some(vt.clone()),
                _ => resolved_expr_cache.get(field.expr.val())
                    .and_then(|v| v.clone()),
            }
        });
        if let Some(ft) = field_type {
            result.retain(|(n, _)| n != name);
            result.push((name.clone(), ft));
        }
    }
}
