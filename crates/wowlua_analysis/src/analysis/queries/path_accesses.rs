//! Global and field reads and writes keyed by the workspace-wide path they
//! use, for the `crossfile-types` contributor command. A path should show one
//! type in every file; one typed where it is written and `any` (or another
//! type) in the files reading it is a cross-file type loss, which no
//! diagnostic shows.

use std::collections::VecDeque;

use crate::collections::{HashMap, HashSet};
use crate::pre_globals::PreResolvedGlobals;
use super::*;

/// Where a [`FieldPath`] starts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PathRoot {
    /// The addon namespace a file-level `...` binds, or the `@class` declared on it.
    AddonNs,
    /// A global variable.
    Global(String),
    /// A `@class` the workspace declares.
    Class(String),
}

/// A root and the fields read from it. A `[]` field is an element of a list or map.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldPath {
    pub root: PathRoot,
    pub fields: Vec<String>,
}

/// `<ns>.Settings.threshold`, `MyClass.items[].label`.
impl std::fmt::Display for FieldPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.root {
            PathRoot::AddonNs => f.write_str("<ns>")?,
            PathRoot::Global(name) | PathRoot::Class(name) => f.write_str(name)?,
        }
        for field in &self.fields {
            if field != "[]" { f.write_str(".")?; }
            f.write_str(field)?;
        }
        Ok(())
    }
}

impl FieldPath {
    fn root(root: PathRoot) -> Self {
        Self { root, fields: Vec::new() }
    }

    fn child(mut self, field: &str) -> Self {
        self.fields.push(field.to_string());
        self
    }
}

/// Why an access has no [`FieldPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Unkeyed {
    /// Its receiver is of unknown type (`any`, a bare `table`, unresolved).
    Untyped,
    /// Its receiver is a table only this file reaches (no global, namespace or
    /// class field holds it).
    FileLocal,
    /// The stubs type it the same in every file: a WoW API global, a member a
    /// built-in class declares (`frame:SetPoint()`), a string's method.
    BuiltIn,
    /// Its receiver is a cross-file table, shape, union or intersection no
    /// path leads to.
    Unnamed,
}

/// One read or write of a global or a field.
#[derive(Debug, Clone)]
pub struct PathAccess {
    /// Start of the access's name token.
    pub offset: u32,
    /// The path used (a global's has no fields), or why it has none.
    pub path: Result<FieldPath, Unkeyed>,
    /// The type the access shows, naming classes without expanding them and
    /// collapsing table shapes to `{…}`: `?` when it has none, as its hover;
    /// `<none>` for a constructor key no table it builds has a field for.
    pub display: String,
    /// An assignment, a function definition or a table constructor key.
    pub write: bool,
}

/// The [`FieldPath`] of each named table.
pub type TablePaths = HashMap<TableIndex, FieldPath>;

/// What an assignment writes that other files read: a field of a cross-file
/// table (the table, in ext space), or a global.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WrittenSlot {
    Field(TableIndex, String),
    Global(String),
}

/// The paths of one file's own tables and of the workspace's cross-file ones.
struct Paths<'a> {
    local: TablePaths,
    ext: &'a TablePaths,
}

/// How deep [`AnalysisResult::receiver_path`] follows a receiver.
const MAX_RECEIVER_DEPTH: usize = 32;

/// The single table a type is, ignoring `nil`.
fn single_table(vt: &ValueType) -> Option<TableIndex> {
    match vt {
        ValueType::Table(Some(idx)) => Some(*idx),
        ValueType::OpaqueAlias(_, inner) => single_table(inner),
        ValueType::Union(members) => {
            let mut non_nil = members.iter().filter(|m| !matches!(m, ValueType::Nil));
            let only = non_nil.next()?;
            if non_nil.next().is_some() { return None; }
            single_table(only)
        }
        _ => None,
    }
}

/// The root a class table stands for: the addon namespace for its `@class`, a
/// workspace class by name. A built-in class (`Frame`) is shared by every
/// instance, so it stands for no single table.
fn class_root(pg: &PreResolvedGlobals, name: &str) -> Option<PathRoot> {
    if pg.addon_ns_class_own_fields.contains_key(name) {
        Some(PathRoot::AddonNs)
    } else if name == "_G" || pg.stub_class_names.contains(name) {
        None
    } else {
        Some(PathRoot::Class(name.to_string()))
    }
}

/// Name the tables reachable from `seeds` through fields and elements,
/// breadth-first, so each gets its shortest path. A class table keeps its
/// class root and is never named by a path.
fn name_reachable(
    paths: &mut TablePaths,
    seeds: Vec<(TableIndex, FieldPath)>,
    mut edges: impl FnMut(TableIndex) -> Vec<(String, TableIndex)>,
    mut is_class: impl FnMut(TableIndex) -> bool,
) {
    let mut queue = VecDeque::new();
    for (idx, path) in seeds {
        if let std::collections::hash_map::Entry::Vacant(e) = paths.entry(idx) {
            e.insert(path);
            queue.push_back(idx);
        }
    }
    while let Some(idx) = queue.pop_front() {
        let path = paths[&idx].clone();
        let mut children = edges(idx);
        children.sort();
        for (field, child) in children {
            if paths.contains_key(&child) || is_class(child) { continue; }
            paths.insert(child, path.clone().child(&field));
            queue.push_back(child);
        }
    }
}

/// A cross-file table field's type, as the workspace build stored it.
fn ext_field_type(pg: &PreResolvedGlobals, fi: &FieldInfo) -> Option<ValueType> {
    fi.annotation.clone().or_else(|| match pg.expr(fi.expr) {
        Expr::Literal(vt) => Some(vt.clone()),
        Expr::TableConstructor(idx) => Some(ValueType::Table(Some(*idx))),
        _ => None,
    })
}

/// The cross-file table a cross-file table's field (or, for `[]`, element) holds.
fn ext_child(pg: &PreResolvedGlobals, idx: TableIndex, field: &str) -> Option<TableIndex> {
    let table = pg.table(idx);
    if field == "[]" {
        return table.value_type.as_ref().and_then(single_table).filter(|t| t.is_external());
    }
    let fi = table.fields.get(field)
        .or_else(|| table.parent_classes.iter().find_map(|p| pg.table(*p).fields.get(field)))
        .or_else(|| table.metatable_index.and_then(|m| pg.table(m).fields.get(field)))?;
    single_table(&ext_field_type(pg, fi)?).filter(|t| t.is_external())
}

/// The cross-file table a cross-file global holds.
fn ext_global_table(pg: &PreResolvedGlobals, idx: SymbolIndex) -> Option<TableIndex> {
    pg.sym(idx).versions.iter().rev()
        .find_map(|v| v.resolved_type.as_ref().and_then(single_table))
        .filter(|t| t.is_external())
}

/// The paths of the workspace's cross-file tables: its globals, addon
/// namespaces and classes, and the tables their fields hold. Computed once per
/// workspace and passed to every file's [`AnalysisResult::path_accesses`].
pub fn ext_table_paths(pg: &PreResolvedGlobals) -> TablePaths {
    let mut seeds: Vec<(TableIndex, FieldPath)> = Vec::new();
    for &idx in pg.addon_table_idx.iter().chain(pg.addon_tables.values()) {
        seeds.push((idx, FieldPath::root(PathRoot::AddonNs)));
    }
    // A class's own entry: `classes` also maps names onto other classes' tables.
    let mut classes: Vec<(&String, &TableIndex)> = pg.classes.iter()
        .filter(|(name, idx)| pg.table(**idx).class_name.as_ref() == Some(*name))
        .collect();
    classes.sort();
    for (name, &idx) in classes {
        if let Some(root) = class_root(pg, name) {
            seeds.push((idx, FieldPath::root(root)));
        }
    }
    let mut globals: Vec<(&String, SymbolIndex)> = pg.scope0_symbols.iter()
        .filter(|(_, idx)| idx.ext_offset() >= pg.stub_symbols_end)
        .filter_map(|(id, &idx)| match id {
            SymbolIdentifier::Name(name) => Some((name, idx)),
            _ => None,
        })
        .collect();
    globals.sort();
    for (name, idx) in globals {
        if let Some(t) = ext_global_table(pg, idx) && pg.table(t).class_name.is_none() {
            seeds.push((t, FieldPath::root(PathRoot::Global(name.clone()))));
        }
    }
    let mut paths = TablePaths::default();
    name_reachable(
        &mut paths,
        seeds,
        |idx| {
            let mut out: Vec<(String, TableIndex)> = pg.table(idx).fields.keys()
                .filter_map(|name| Some((name.clone(), ext_child(pg, idx, name)?)))
                .collect();
            out.extend(ext_child(pg, idx, "[]").map(|elem| ("[]".to_string(), elem)));
            out
        },
        |idx| pg.table(idx).class_name.is_some(),
    );
    paths
}

impl AnalysisResult {
    /// Every read and write in the file of a global (`G`) or a field
    /// (`recv.f`, `recv:f()`, `function recv.f()`, `{ f = v }`), with the path
    /// it uses.
    pub fn path_accesses(&self, tree: &SyntaxTree, ext_paths: &TablePaths) -> Vec<PathAccess> {
        let paths = Paths { local: self.local_table_paths(ext_paths), ext: ext_paths };
        let declared_maps = self.declared_map_constructors();
        let mut seen: HashSet<u32> = HashSet::default();
        let mut accesses = Vec::new();
        for (_, expr) in self.local_exprs() {
            let Expr::FieldAccess { table, field, field_range: Some((start, _)) } = expr else { continue };
            if !seen.insert(*start) { continue; }
            let receiver = self.receiver_path(*table, &paths, 0)
                .or_else(|| self.field_owner(tree, *start, &paths).0);
            let path = match receiver {
                Some(receiver) => self.field_path(receiver, field, &paths),
                None => Err(self.unkeyed(self.resolve_expr_type(*table).as_ref())),
            };
            let display = self.access_display(tree, *start, &path, false);
            let path = path.and_then(|p| self.unless_builtin(p));
            accesses.push(PathAccess { offset: *start, path, display, write: false });
        }
        let root = SyntaxNode::new_root(tree);
        for token in tree.all_tokens() {
            if token.kind != SyntaxKind::Name || seen.contains(&token.start) { continue; }
            let Some(t) = root.token_at_offset(TextSize::from(token.start)).right_biased() else { continue };
            let access = if Self::is_field_position(tree, token.start) {
                self.syntax_field_access(tree, t, &paths)
            } else if Self::is_constructor_key(&t) {
                self.constructor_key_access(t, &paths, &declared_maps)
            } else {
                self.global_access(tree, t)
            };
            accesses.extend(access);
        }
        accesses.sort_by_key(|a| a.offset);
        accesses
    }

    /// Every assignment in this file writing a field of a cross-file table or a
    /// global, with the value written (`Ir::target_values`), or `None` for one an
    /// author annotation types (`Ir::annotated_targets`): the slot keyed as
    /// [`Self::path_accesses`] keys the write, through the cross-file table its
    /// receiver's path reaches.
    pub fn slot_writes(&self, tree: &SyntaxTree, ext_paths: &TablePaths) -> Vec<(WrittenSlot, Option<ExprId>)> {
        if self.ir.target_values.is_empty() && self.ir.annotated_targets.is_empty() {
            return Vec::new();
        }
        let paths = Paths { local: self.local_table_paths(ext_paths), ext: ext_paths };
        let root = SyntaxNode::new_root(tree);
        let mut writes: Vec<(u32, Option<ExprId>)> = self.ir.target_values.iter().map(|(&offset, &value)| (offset, Some(value)))
            .chain(self.ir.annotated_targets.iter().map(|&offset| (offset, None)))
            .collect();
        writes.sort_unstable();
        let mut out = Vec::new();
        // A constructor's entries are writes of its table's fields, where a path
        // names the table (`ns.Constants = { Flag = false }`).
        let written: HashSet<ExprId> = writes.iter().filter_map(|&(_, value)| value).collect();
        let mut tables: Vec<(&TableIndex, &FieldPath)> = paths.local.iter().collect();
        tables.sort();
        for (&table, path) in tables {
            let Some(ext_table) = self.canonical_path(path.clone(), &paths).1 else { continue };
            let mut entries: Vec<(&String, Option<ExprId>)> = self.table(table).fields.iter()
                .filter(|(_, fi)| !fi.expr.is_external() && !written.contains(&fi.expr))
                .map(|(name, fi)| (name, fi.annotation.is_none().then_some(fi.expr)))
                .collect();
            entries.sort();
            out.extend(entries.into_iter().map(|(name, value)| (WrittenSlot::Field(ext_table, name.clone()), value)));
        }
        for (offset, value) in writes {
            let Some(token) = root.token_at_offset(TextSize::from(offset)).right_biased() else { continue };
            // The string key of a bracket target (`t["f"]`), a write of `t.f`.
            if token.kind() == SyntaxKind::String {
                let slot = token.parent().and_then(|key| key.parent()).filter(|b| b.kind() == SyntaxKind::BracketAccess).and_then(|bracket| {
                    let field = crate::ast::extract_bracket_string_key(bracket)?;
                    let receiver = self.syntax_value_path(tree, bracket.children().next()?, &paths, 0)?;
                    self.written_field_slot(receiver, &field, &paths)
                });
                out.extend(slot.map(|slot| (slot, value)));
                continue;
            }
            if token.kind() != SyntaxKind::Name { continue; }
            let slot = if Self::is_field_position(tree, offset) {
                let Some(parent) = token.parent() else { continue };
                self.syntax_receiver_path(tree, parent, token, &paths)
                    .or_else(|| self.field_owner(tree, offset, &paths).0)
                    .and_then(|receiver| self.written_field_slot(receiver, token.text(), &paths))
            } else {
                self.find_symbol_at(tree, offset)
                    .filter(|(sym_idx, ..)| self.is_global_symbol(*sym_idx))
                    .map(|(_, name, _)| WrittenSlot::Global(name))
            };
            out.extend(slot.map(|slot| (slot, value)));
        }
        out
    }

    /// The slot a write of `field` through a receiver at `receiver` writes: `_G.X`
    /// is the global `X`, else the field of the cross-file table the receiver's
    /// path reaches.
    fn written_field_slot(&self, receiver: FieldPath, field: &str, paths: &Paths) -> Option<WrittenSlot> {
        let receiver = match &receiver.root {
            PathRoot::Global(g) if g == "_G" => match receiver.fields.split_first() {
                None => return Some(WrittenSlot::Global(field.to_string())),
                Some((global, rest)) => FieldPath { root: PathRoot::Global(global.clone()), fields: rest.to_vec() },
            },
            _ => receiver,
        };
        let (_, table) = self.canonical_path(receiver, paths);
        Some(WrittenSlot::Field(table?, field.to_string()))
    }

    /// A field access the IR records no read site for: the field an assignment
    /// or a function definition writes, or a read in the receiver chain of one
    /// (`a.b` in `a.b[k] = v` or `function a.b.f()`).
    fn syntax_field_access(&self, tree: &SyntaxTree, token: SyntaxToken<'_>, paths: &Paths) -> Option<PathAccess> {
        let parent = token.parent()?;
        let is_last_name = parent.children_with_tokens()
            .filter_map(|c| c.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .last()
            .is_some_and(|t| t.text_range() == token.text_range());
        let write = is_last_name && parent.parent()
            .is_some_and(|p| matches!(p.kind(), SyntaxKind::VariableList | SyntaxKind::FunctionDefinition));
        let offset = u32::from(token.text_range().start());
        let path = match self.syntax_receiver_path(tree, parent, token, paths) {
            Some(receiver) => self.field_path(receiver, token.text(), paths),
            None => match self.field_owner(tree, offset, paths) {
                (Some(receiver), _) => self.field_path(receiver, token.text(), paths),
                (None, owner) => Err(self.unkeyed(owner.map(|t| ValueType::Table(Some(t))).as_ref())),
            },
        };
        let display = self.access_display(tree, offset, &path, write);
        let path = path.and_then(|p| self.unless_builtin(p));
        Some(PathAccess { offset, path, display, write })
    }

    /// The field a table constructor key (`f` in `{ f = v }`) writes: an element
    /// of the declared type for a constructor written to a field declaring a map
    /// (`declared_maps`), as reads through the field show it.
    fn constructor_key_access(&self, token: SyntaxToken<'_>, paths: &Paths, declared_maps: &HashMap<TableIndex, ValueType>) -> Option<PathAccess> {
        let owners = self.constructor_key_owners(token);
        let ctor = *owners.last()?;
        let field = token.text();
        let path = match owners.iter().find_map(|&t| self.table_path(t, paths)) {
            Some(receiver) => self.field_path(receiver, field, paths),
            None => Err(self.unkeyed(Some(&ValueType::Table(Some(ctor))))),
        }.and_then(|p| self.unless_builtin(p));
        let offset = u32::from(token.text_range().start());
        let _secrecy = self.secrecy_display_at(offset);
        let display = match declared_maps.get(&ctor) {
            Some(element) => collapse_shapes(&self.format_type_depth(element, 1)),
            None => owners.iter().find_map(|&t| self.get_field(t, field))
                .map_or_else(|| "<none>".to_string(), |fi| collapse_shapes(&self.format_field_type(fi, 0))),
        };
        Some(PathAccess { offset, path, display, write: true })
    }

    /// The element type of each table constructor this file writes to a field
    /// whose declared type is a map (`---@type table<K, V>`).
    fn declared_map_constructors(&self) -> HashMap<TableIndex, ValueType> {
        let mut out = HashMap::default();
        for fa in &self.ir.field_assignments {
            let Some(fi) = self.get_field(fa.table_idx, &fa.field_name) else { continue };
            let (Some(_), Some(ValueType::Table(Some(map)))) = (&fi.annotation_text, &fi.annotation) else { continue };
            let map = self.table(*map);
            let Some(element) = map.value_type.as_ref().filter(|_| table_is_map(map.key_type.as_ref(), map.is_explicit_map)) else { continue };
            if let Some(ctor) = self.ir.find_table_index(fa.actual_expr).filter(|t| !t.is_external()) {
                out.insert(ctor, element.clone());
            }
        }
        out
    }

    /// A read of a global variable at `token`, or an assignment or function
    /// definition writing one.
    fn global_access(&self, tree: &SyntaxTree, token: SyntaxToken<'_>) -> Option<PathAccess> {
        let parent = token.parent()?;
        let write = match parent.kind() {
            SyntaxKind::NameRef => parent.parent().is_some_and(|p| p.kind() == SyntaxKind::VariableList),
            SyntaxKind::FunctionDefinition => true,
            // The root of a function name (`a` in `function a.b()`).
            SyntaxKind::DotAccess if parent.parent().is_some_and(|p| p.kind() == SyntaxKind::FunctionDefinition) => false,
            _ => return None,
        };
        let offset = u32::from(token.text_range().start());
        let (sym_idx, name, token_start) = self.find_symbol_at(tree, offset)?;
        if !self.is_global_symbol(sym_idx) { return None; }
        let display = self.global_display(sym_idx, token_start, write);
        let path = self.unless_builtin(FieldPath::root(PathRoot::Global(name)));
        Some(PathAccess { offset, path, display, write })
    }

    /// The type a global shows at `offset`, widened: a global's version holds
    /// the literal last written to it. At a write, the global's type as the file
    /// writes it — every value it writes there, as a field's write shows the field —
    /// rather than the value this one writes.
    fn global_display(&self, sym_idx: SymbolIndex, offset: u32, write: bool) -> String {
        let _secrecy = self.secrecy_display_at(offset);
        let written: Vec<ValueType> = match write && !sym_idx.is_external() {
            true => self.sym(sym_idx).versions.iter()
                .filter(|v| v.type_source.is_some())
                .filter_map(|v| v.resolved_type.clone())
                .collect(),
            false => Vec::new(),
        };
        let vt = if written.is_empty() { self.symbol_resolved_type_at(sym_idx, offset).cloned() } else { Some(ValueType::make_union(written)) };
        match vt {
            Some(vt) => collapse_shapes(&self.format_type_depth(&vt.widen_literals(), 1)),
            None => "?".to_string(),
        }
    }

    /// The type a field access at `offset` shows, or for `_G.X`, which `path`
    /// keys as the global `X`, the global's.
    fn access_display(&self, tree: &SyntaxTree, offset: u32, path: &Result<FieldPath, Unkeyed>, write: bool) -> String {
        match path {
            Ok(FieldPath { root: PathRoot::Global(name), fields }) if fields.is_empty() => {
                match self.ir.scope0_global_symbol(&SymbolIdentifier::Name(name.clone())) {
                    Some(sym_idx) => self.global_display(sym_idx, offset, write),
                    None => "?".to_string(),
                }
            }
            _ => self.field_display(tree, offset),
        }
    }

    /// `path`, unless it starts at a WoW API global.
    fn unless_builtin(&self, path: FieldPath) -> Result<FieldPath, Unkeyed> {
        let PathRoot::Global(name) = &path.root else { return Ok(path) };
        let id = SymbolIdentifier::Name(name.clone());
        let builtin = match self.ir.ext.scope0_symbols.get(&id) {
            Some(idx) => idx.ext_offset() < self.ir.ext.stub_symbols_end,
            None => self.ir.ext.framexml_scope0_symbols.contains_key(&id),
        };
        if builtin { Err(Unkeyed::BuiltIn) } else { Ok(path) }
    }

    fn is_global_symbol(&self, sym_idx: SymbolIndex) -> bool {
        sym_idx.is_external()
            || (self.sym(sym_idx).scope_idx == ScopeIndex(0) && !self.ir.local_decl_symbols.contains(&sym_idx))
    }

    /// The type the field access at `offset` shows, as [`PathAccess::display`].
    fn field_display(&self, tree: &SyntaxTree, offset: u32) -> String {
        let _secrecy = self.secrecy_display_at(offset);
        let chain = self.resolve_field_chain_at(tree, offset);
        let shown = match chain.as_ref().and_then(|(table_idx, field_name, ..)| self.get_field(*table_idx, field_name)) {
            // Widened, as a global's: a field's writes hold the literals written. The
            // type the scan gave a field is no author's: the field's writes type it.
            Some(fi) if (fi.annotation.is_none() && fi.annotation_text.is_none()) || fi.scan_inferred() => match self.resolve_field_type(fi) {
                Some(vt) => self.format_type_depth(&vt.widen_literals(), 1),
                None => "?".to_string(),
            },
            Some(fi) => self.format_field_type(fi, 0),
            None => match chain.and_then(|(.., expr_id, _, _)| self.resolve_expr_type(expr_id))
                .or_else(|| self.field_access_type_at(offset))
            {
                Some(vt) => self.format_type_depth(&vt, 1),
                None => "?".to_string(),
            },
        };
        collapse_shapes(&shown)
    }

    /// What a receiver with no path is, from its type.
    fn unkeyed(&self, receiver: Option<&ValueType>) -> Unkeyed {
        if crate::analysis::is_unknown_type(receiver) {
            return Unkeyed::Untyped;
        }
        match receiver.map(ValueType::strip_nil) {
            Some(ValueType::Table(None)) | None => Unkeyed::Untyped,
            Some(vt) if self.is_builtin_instance(&vt) => Unkeyed::BuiltIn,
            Some(ValueType::Table(Some(idx))) if !idx.is_external() => Unkeyed::FileLocal,
            Some(ValueType::Table(_) | ValueType::TableShape(_) | ValueType::Union(_)
                | ValueType::Intersection(_) | ValueType::OpaqueAlias(..) | ValueType::TypeVariable(_)) => Unkeyed::Unnamed,
            Some(_) => Unkeyed::BuiltIn,
        }
    }

    /// An instance of a built-in class: of `Frame`, or `Frame` with a
    /// template's or a file's fields (`Button & UIPanelButtonTemplate`).
    fn is_builtin_instance(&self, vt: &ValueType) -> bool {
        match vt {
            ValueType::Table(Some(idx)) => self.table(*idx).class_name.as_deref()
                .is_some_and(|name| class_root(&self.ir.ext, name).is_none()),
            ValueType::Intersection(members) => members.iter().any(|m| self.is_builtin_instance(m)),
            ValueType::Union(members) => members.iter()
                .filter(|m| !matches!(m, ValueType::Nil))
                .all(|m| self.is_builtin_instance(m)),
            _ => false,
        }
    }

    /// The paths of this file's own tables: its addon namespace views and
    /// classes, the tables its globals hold, the tables it stores in
    /// cross-file tables' fields, and the tables their fields hold.
    fn local_table_paths(&self, ext_paths: &TablePaths) -> TablePaths {
        let mut seeds: Vec<(TableIndex, FieldPath)> = Vec::new();
        let mut views: Vec<TableIndex> = self.ir.addon_ns_views.iter().copied().collect();
        views.sort();
        seeds.extend(views.into_iter().map(|t| (t, FieldPath::root(PathRoot::AddonNs))));
        for (idx, table) in self.local_tables() {
            if let Some(root) = table.class_name.as_deref().and_then(|n| class_root(&self.ir.ext, n)) {
                seeds.push((idx, FieldPath::root(root)));
            }
        }
        for (sym_idx, sym) in self.local_symbols() {
            let SymbolIdentifier::Name(name) = &sym.id else { continue };
            if !self.is_global_symbol(sym_idx) { continue; }
            for v in &sym.versions {
                if let Some(t) = v.resolved_type.as_ref().and_then(single_table)
                    && !t.is_external() && self.table(t).class_name.is_none()
                {
                    seeds.push((t, FieldPath::root(PathRoot::Global(name.clone()))));
                }
            }
        }
        let mut overlays: Vec<(&TableIndex, &HashMap<String, FieldInfo>)> = self.ir.overlay_fields.iter().collect();
        overlays.sort_by_key(|(idx, _)| **idx);
        for (ext_idx, fields) in overlays {
            let Some(path) = ext_paths.get(ext_idx) else { continue };
            let mut names: Vec<&String> = fields.keys().collect();
            names.sort();
            for name in names {
                for t in self.field_tables(&fields[name]).into_iter().filter(|t| !t.is_external()) {
                    seeds.push((t, path.clone().child(name)));
                }
            }
        }
        // Shortest first: `name_reachable` keeps the first path it gives a table.
        seeds.sort_by_key(|(_, p)| p.fields.len());
        let mut paths = TablePaths::default();
        name_reachable(
            &mut paths,
            seeds,
            |idx| {
                if idx.is_external() { return Vec::new(); }
                let table = self.table(idx);
                let mut out: Vec<(String, TableIndex)> = table.fields.iter()
                    .flat_map(|(name, fi)| self.field_tables(fi).into_iter().map(move |t| (name.clone(), t)))
                    .collect();
                out.extend(table.value_type.as_ref().and_then(single_table).map(|elem| ("[]".to_string(), elem)));
                out.retain(|(_, t)| !t.is_external());
                out
            },
            |idx| self.table(idx).class_name.is_some(),
        );
        paths
    }

    /// The tables a field holds: the one its type is, and the ones this file
    /// writes to it (a namespace field typed from the workspace scan holds the
    /// file's own table too).
    fn field_tables(&self, fi: &FieldInfo) -> Vec<TableIndex> {
        let mut tables: Vec<TableIndex> = self.resolve_field_type(fi).as_ref().and_then(single_table).into_iter().collect();
        for &e in std::iter::once(&fi.expr).chain(&fi.extra_exprs) {
            if let Some(t) = self.resolve_expr_type(e).as_ref().and_then(single_table) && !tables.contains(&t) {
                tables.push(t);
            }
        }
        tables
    }

    /// For a receiver naming no path itself (`Frame & MyMixin`), the path of
    /// the first of its tables with the field at `offset` that has one (the
    /// method on `MyMixin`); and the table the field is found on.
    fn field_owner(&self, tree: &SyntaxTree, offset: u32, paths: &Paths) -> (Option<FieldPath>, Option<TableIndex>) {
        let Some((found, name, _, _, receivers)) = self.resolve_field_chain_at(tree, offset) else { return (None, None) };
        let path = std::iter::once(found).chain(receivers)
            .filter(|&t| self.get_field(t, &name).is_some())
            .find_map(|t| self.table_path(t, paths));
        (path, Some(found))
    }

    fn table_path(&self, idx: TableIndex, paths: &Paths) -> Option<FieldPath> {
        if let Some(name) = &self.table(idx).class_name {
            return class_root(&self.ir.ext, name).map(FieldPath::root);
        }
        if idx.is_external() { paths.ext.get(&idx).cloned() } else { paths.local.get(&idx).cloned() }
    }

    /// The path of the named table a type is.
    fn typed_path(&self, vt: Option<&ValueType>, paths: &Paths) -> Option<FieldPath> {
        self.table_path(single_table(vt?)?, paths)
    }

    /// The path an access of `field` through a receiver at `receiver` uses:
    /// `_G.X` is the global `X`, and a member the receiver inherits is keyed by
    /// the class declaring it (`Base.Refresh` through a `Derived` receiver).
    fn field_path(&self, receiver: FieldPath, field: &str, paths: &Paths) -> Result<FieldPath, Unkeyed> {
        let receiver = match &receiver.root {
            PathRoot::Global(g) if g == "_G" => match receiver.fields.split_first() {
                None => return Ok(FieldPath::root(PathRoot::Global(field.to_string()))),
                Some((global, rest)) => FieldPath { root: PathRoot::Global(global.clone()), fields: rest.to_vec() },
            },
            _ => receiver,
        };
        let (receiver, table) = self.canonical_path(receiver, paths);
        match table.and_then(|t| self.inherited_owner(t, field, paths)) {
            Some(owner) => owner.map(|o| o.child(field)),
            None => Ok(receiver.child(field)),
        }
    }

    /// For a member the cross-file `table` inherits rather than declares (from
    /// a parent class or its metatable's `__index`), the declaring table's
    /// path, or `BuiltIn` when a built-in class declares it.
    fn inherited_owner(&self, table: TableIndex, field: &str, paths: &Paths) -> Option<Result<FieldPath, Unkeyed>> {
        let pg = &*self.ir.ext;
        let t = pg.table(table);
        if field == "[]" || t.fields.contains_key(field) { return None; }
        let owner = t.parent_classes.iter().copied()
            .chain(t.metatable_index)
            .find(|&o| pg.table(o).fields.contains_key(field))?;
        match self.table_path(owner, paths) {
            Some(path) => Some(Ok(path)),
            None if pg.table(owner).class_name.is_some() => Some(Err(Unkeyed::BuiltIn)),
            None => None,
        }
    }

    /// `path` re-rooted at the deepest cross-file table it reaches that has a
    /// path of its own (a class, a table stored under another name), so a file
    /// whose types lost the way there still keys its reads like the others;
    /// and the cross-file table the whole of `path` reaches.
    fn canonical_path(&self, path: FieldPath, paths: &Paths) -> (FieldPath, Option<TableIndex>) {
        let pg = &*self.ir.ext;
        let root_table = match &path.root {
            PathRoot::AddonNs => self.ir.addon_table_idx(),
            PathRoot::Global(name) => pg.scope0_symbols.get(&SymbolIdentifier::Name(name.clone()))
                .and_then(|&idx| ext_global_table(pg, idx)),
            PathRoot::Class(name) => pg.classes.get(name).copied().filter(|t| t.is_external()),
        };
        let Some(mut table) = root_table else { return (path, None) };
        let mut best = paths.ext.contains_key(&table).then_some((0, table));
        let mut walked = 0;
        for (i, field) in path.fields.iter().enumerate() {
            let Some(next) = ext_child(pg, table, field) else { break };
            table = next;
            walked = i + 1;
            if paths.ext.contains_key(&table) {
                best = Some((i + 1, table));
            }
        }
        let reached = (walked == path.fields.len()).then_some(table);
        let Some((consumed, idx)) = best else { return (path, reached) };
        let mut out = paths.ext[&idx].clone();
        out.fields.extend(path.fields[consumed..].iter().cloned());
        (out, reached)
    }

    /// The path of the value `expr_id` reads: the one its syntax follows (`a.b`,
    /// a local's value, `x or {}`) to a global or the addon namespace, else the
    /// named table its type is (`self`, a call's result, a file's own table).
    /// Syntax comes first: a file's types are what a loss changes.
    fn receiver_path(&self, expr_id: ExprId, paths: &Paths, depth: usize) -> Option<FieldPath> {
        if depth > MAX_RECEIVER_DEPTH { return None; }
        self.syntactic_path(expr_id, paths, depth)
            .or_else(|| self.typed_path(self.resolve_expr_type(expr_id).as_ref(), paths))
    }

    fn syntactic_path(&self, expr_id: ExprId, paths: &Paths, depth: usize) -> Option<FieldPath> {
        let follow = |e: ExprId| self.receiver_path(e, paths, depth + 1);
        match self.expr(expr_id) {
            Expr::Grouped(inner) | Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::StripTruthy(inner)
            | Expr::CastAdd(inner, _) | Expr::CastRemove(inner, _) | Expr::TypeFilter(inner, _)
            | Expr::AssignNarrow { inner, .. } | Expr::OverloadNarrow { inner, .. } => follow(*inner),
            Expr::FieldAccess { table, field, .. } => follow(*table).map(|p| p.child(field)),
            Expr::BracketIndex { table, literal_key, .. } => {
                follow(*table).map(|p| p.child(literal_key.as_deref().unwrap_or("[]")))
            }
            Expr::BinaryOp { op: Operator::Or, lhs, .. } => follow(*lhs),
            Expr::BinaryOp { op: Operator::And, rhs, .. } => follow(*rhs),
            Expr::BranchMerge(members) => {
                let mut member_paths = members.iter().map(|&m| follow(m));
                let first = member_paths.next()??;
                member_paths.all(|p| p.as_ref() == Some(&first)).then_some(first)
            }
            Expr::SymbolRef(sym_idx, ver) => self.symbol_path(*sym_idx, *ver, paths, depth),
            _ => None,
        }
    }

    /// The path of version `ver` of a symbol: a global's name, or the path of
    /// the value a local was given.
    fn symbol_path(&self, sym_idx: SymbolIndex, ver: usize, paths: &Paths, depth: usize) -> Option<FieldPath> {
        let sym = self.sym(sym_idx);
        if self.is_global_symbol(sym_idx) {
            let SymbolIdentifier::Name(name) = &sym.id else { return None };
            return Some(FieldPath::root(PathRoot::Global(name.clone())));
        }
        self.receiver_path(sym.versions.get(ver)?.type_source?, paths, depth + 1)
    }

    /// The path of the receiver of the field `token` names in `node`: the
    /// names before it in a flat function name (`a.b.f`), else `node`'s base.
    fn syntax_receiver_path(
        &self,
        tree: &SyntaxTree,
        node: SyntaxNode<'_>,
        token: SyntaxToken<'_>,
        paths: &Paths,
    ) -> Option<FieldPath> {
        let before: Vec<SyntaxToken<'_>> = node.children_with_tokens()
            .filter_map(|c| c.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name && t.text_range().start() < token.text_range().start())
            .collect();
        if let Some((first, rest)) = before.split_first() {
            let path = self.syntax_name_path(tree, *first, paths)?;
            return Some(rest.iter().fold(path, |p, name| p.child(name.text())));
        }
        self.syntax_value_path(tree, node.children().next()?, paths, 0)
    }

    /// The path of the value an expression node reads, from its syntax.
    fn syntax_value_path(&self, tree: &SyntaxTree, node: SyntaxNode<'_>, paths: &Paths, depth: usize) -> Option<FieldPath> {
        if depth > MAX_RECEIVER_DEPTH { return None; }
        let last_name = || node.children_with_tokens()
            .filter_map(|c| c.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .last();
        match node.kind() {
            SyntaxKind::NameRef => self.syntax_name_path(tree, last_name()?, paths),
            SyntaxKind::DotAccess => {
                let field = last_name()?;
                self.syntax_receiver_path(tree, node, field, paths).map(|p| p.child(field.text()))
            }
            SyntaxKind::BracketAccess => {
                let key = crate::ast::extract_bracket_string_key(node).unwrap_or_else(|| "[]".to_string());
                self.syntax_value_path(tree, node.children().next()?, paths, depth + 1).map(|p| p.child(&key))
            }
            _ => None,
        }
    }

    /// The path of the value a name reads, as [`Self::receiver_path`]'s.
    fn syntax_name_path(&self, tree: &SyntaxTree, name: SyntaxToken<'_>, paths: &Paths) -> Option<FieldPath> {
        let (sym_idx, _, start) = self.find_symbol_at(tree, u32::from(name.text_range().start()))?;
        let versions = self.sym(sym_idx).versions.len();
        let ver = self.symbol_version_at.get(&start).copied().unwrap_or(versions.saturating_sub(1));
        self.symbol_path(sym_idx, ver, paths, 0)
            .or_else(|| self.typed_path(self.symbol_resolved_type_at(sym_idx, start), paths))
    }
}


/// Collapse every `{…}` table shape in a formatted type, so a path's type
/// doesn't differ between files only by a field nested in it (that field is
/// compared on its own path).
fn collapse_shapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '{' => {
                if depth == 0 { out.push_str("{…}"); }
                depth += 1;
            }
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}
