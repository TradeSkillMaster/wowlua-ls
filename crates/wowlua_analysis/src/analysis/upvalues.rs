//! Upvalue entry versions. A function reading a local declared outside it can
//! run after any write the file makes to that local — in another function, or
//! later in the declaring one — so on entry the local holds any of those values
//! (`local db` set in `Init` and read in `Use`), not only the version visible
//! where the function is defined.
//!
//! When a function body starts, each such local it reads gets an *entry
//! version* in the body's scope, which everything in the function reads in
//! place of the version outside, so its narrowings and branch merges build on
//! it. While the IR is built the entry version aliases that outside version;
//! once every write is known it becomes the union of the outside version and
//! the writes the function can't rule out. A declaration without a value
//! (`local db`, `local db = nil`) adds no `nil` (as in LuaLS): readers expect
//! one of the writes to have run.

use crate::collections::{HashMap, HashSet};
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::types::*;
use super::{Analysis, Ir};

/// The plain writes (`x = v`, `function x() end`) to one `local`, which
/// `Analysis::local_writes` keys by the start of its declaration and its name.
pub struct LocalWrites {
    /// The start of the innermost function containing the declaration (`None`
    /// at file level).
    decl_func: Option<u32>,
    writes: Vec<LocalWrite>,
}

/// A write's statement range and the start of the innermost function
/// containing it.
struct LocalWrite {
    start: u32,
    end: u32,
    func: Option<u32>,
}

/// An upvalue entry version (see the module docs).
#[derive(Debug, Clone)]
pub struct UpvalueEntry {
    /// The version visible where the reading function is defined.
    pub prior: usize,
    /// The expression typing the entry version.
    pub merge: ExprId,
    /// The reading function's body scope.
    pub body_scope: ScopeIndex,
    /// The local's declaration gives it no value: it is nil until one of the
    /// writes runs, though its type leaves `nil` out.
    pub uninitialized: bool,
}

/// A step of the walk in [`Analysis::collect_local_writes`].
enum Step<'a> {
    Visit(SyntaxNode<'a>, Option<u32>),
    /// A name, its declaration's start, and the function the declaration is in.
    Declare(&'a str, u32, Option<u32>),
    OpenScope,
    CloseScope,
}

/// The name a `NameRef` node reads.
fn name_ref_text<'a>(node: SyntaxNode<'a>) -> Option<&'a str> {
    node.children_with_tokens().find_map(|c| match c {
        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text()),
        _ => None,
    })
}

/// The text of `node`'s direct `kind` tokens.
fn child_tokens<'a>(node: SyntaxNode<'a>, kind: SyntaxKind) -> Vec<&'a str> {
    node.children_with_tokens().filter_map(|c| match c {
        NodeOrToken::Token(t) if t.kind() == kind => Some(t.text()),
        _ => None,
    }).collect()
}

/// The child nodes of `node` of `kind`.
fn child_nodes<'a>(node: SyntaxNode<'a>, kind: SyntaxKind) -> Vec<SyntaxNode<'a>> {
    node.children().filter(|n| n.kind() == kind).collect()
}

/// Queue `nodes` to be visited in order.
fn visit_in_order<'a>(stack: &mut Vec<Step<'a>>, nodes: Vec<SyntaxNode<'a>>, func: Option<u32>) {
    stack.extend(nodes.into_iter().rev().map(|n| Step::Visit(n, func)));
}

/// The names a function body reads (`NameRef`s other than plain assignment
/// targets), outside the bodies of the functions nested in it.
fn body_read_names<'a>(body: SyntaxNode<'a>) -> Vec<&'a str> {
    let mut names = Vec::new();
    let mut stack: Vec<SyntaxNode<'a>> = body.children().collect();
    while let Some(node) = stack.pop() {
        match node.kind() {
            SyntaxKind::FunctionDefinition => {
                // `function t.f()` / `function t:m()` reads `t` here.
                if let Some(path) = child_nodes(node, SyntaxKind::DotAccess).first() {
                    names.extend(child_tokens(*path, SyntaxKind::Name).first().copied());
                }
                continue;
            }
            SyntaxKind::NameRef if node.parent().is_none_or(|p| p.kind() != SyntaxKind::VariableList) => {
                names.extend(name_ref_text(node));
            }
            _ => {}
        }
        stack.extend(node.children());
    }
    names
}

impl Ir {
    /// The version declaring the variable version `ver` of local `sym` belongs
    /// to: a `local` statement repeating a name in its own scope versions the
    /// same symbol (see `insert_symbol`) but starts a new variable.
    fn binding_start(&self, sym: SymbolIndex, ver: usize) -> usize {
        (1..=ver).rev().find(|&v| self.local_redeclarations.contains(&(sym, v))).unwrap_or(0)
    }

    /// One past the last version of the variable version `ver` of local `sym`
    /// belongs to (see [`Self::binding_start`]).
    fn binding_end(&self, sym: SymbolIndex, ver: usize) -> usize {
        let len = self.symbols[sym.val()].versions.len();
        (ver + 1..len).find(|&v| self.local_redeclarations.contains(&(sym, v))).unwrap_or(len)
    }

    /// Whether version `ver` of local `sym` gives it no value (`local x`, `x = nil`).
    fn is_valueless_version(&self, sym: SymbolIndex, ver: usize) -> bool {
        self.symbols[sym.val()].versions[ver].type_source
            .is_none_or(|ts| matches!(self.expr(ts), Expr::Literal(ValueType::Nil)))
    }

    /// Whether version `ver` of local `sym` gives it a new value, rather than
    /// narrowing, restoring or merging its earlier ones. Unlike
    /// [`Self::is_narrowing_only_version`], a narrowed read of another variable
    /// (`x = y` under `if type(y) == "string"`) is a write.
    fn is_write_version(&self, sym: SymbolIndex, ver: usize) -> bool {
        if self.narrowing_only_versions.contains(&(sym, ver)) {
            return false;
        }
        let Some(mut expr) = self.symbols[sym.val()].versions[ver].type_source else { return false };
        loop {
            expr = match self.expr(expr) {
                Expr::BranchMerge(_) => return false,
                Expr::SymbolRef(s, _) => return *s != sym,
                Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::StripTruthy(inner)
                | Expr::TypeFilter(inner, _) | Expr::CastAdd(inner, _) | Expr::CastRemove(inner, _)
                | Expr::AssignNarrow { inner, .. } | Expr::OverloadNarrow { inner, .. } => *inner,
                _ => return true,
            };
        }
    }
}

impl<'a> Analysis<'a> {
    /// Attribute each plain write in the file to the `local` it assigns (see
    /// [`LocalWrites`]), scoping names as the build does.
    pub(super) fn collect_local_writes(&mut self) {
        let mut writes: HashMap<(u32, &'a str), LocalWrites> = HashMap::default();
        // The declarations in scope, innermost last, and where each open scope begins.
        let mut visible: Vec<(&'a str, u32, Option<u32>)> = Vec::new();
        let mut scopes: Vec<usize> = Vec::new();
        let mut stack = vec![Step::Visit(self.root(), None)];
        while let Some(step) = stack.pop() {
            let (node, func) = match step {
                Step::Visit(node, func) => (node, func),
                Step::Declare(name, decl, decl_func) => {
                    visible.push((name, decl, decl_func));
                    continue;
                }
                Step::OpenScope => {
                    scopes.push(visible.len());
                    continue;
                }
                Step::CloseScope => {
                    visible.truncate(scopes.pop().unwrap_or_default());
                    continue;
                }
            };
            let range = node.text_range();
            let (start, end) = (u32::from(range.start()), u32::from(range.end()));
            let mut record = |name: &'a str, visible: &[(&'a str, u32, Option<u32>)]| {
                if let Some(&(_, decl, decl_func)) = visible.iter().rev().find(|(n, ..)| *n == name) {
                    writes.entry((decl, name))
                        .or_insert_with(|| LocalWrites { decl_func, writes: Vec::new() })
                        .writes.push(LocalWrite { start, end, func });
                }
            };
            match node.kind() {
                SyntaxKind::Block => {
                    stack.push(Step::CloseScope);
                    visit_in_order(&mut stack, node.children().collect(), func);
                    stack.push(Step::OpenScope);
                }
                SyntaxKind::LocalAssignStatement => {
                    // The values only write inside function bodies, which the build
                    // lowers after the statement: the names are declared by then.
                    let names: Vec<&'a str> = child_nodes(node, SyntaxKind::NameList).into_iter()
                        .flat_map(|list| child_tokens(list, SyntaxKind::Name))
                        .collect();
                    visit_in_order(&mut stack, child_nodes(node, SyntaxKind::ExpressionList), func);
                    stack.extend(names.into_iter().rev().map(|name| Step::Declare(name, start, func)));
                }
                SyntaxKind::AssignStatement => {
                    for list in child_nodes(node, SyntaxKind::VariableList) {
                        for target in child_nodes(list, SyntaxKind::NameRef) {
                            if let Some(name) = name_ref_text(target) {
                                record(name, &visible);
                            }
                        }
                    }
                    visit_in_order(&mut stack, node.children().collect(), func);
                }
                SyntaxKind::FunctionDefinition => {
                    // `local function x` declares `x`, for its own body too;
                    // `function x()` assigns it.
                    let is_local = node.children_with_tokens()
                        .any(|c| matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::LocalKeyword));
                    let name = child_tokens(node, SyntaxKind::Name).first().copied()
                        .or_else(|| child_nodes(node, SyntaxKind::NameRef).into_iter().find_map(name_ref_text));
                    match name {
                        Some(name) if is_local => visible.push((name, start, func)),
                        Some(name) => record(name, &visible),
                        None => {}
                    }
                    let params: Vec<&'a str> = child_nodes(node, SyntaxKind::ParameterList).into_iter()
                        .flat_map(|list| child_tokens(list, SyntaxKind::Parameter))
                        .collect();
                    stack.push(Step::CloseScope);
                    visit_in_order(&mut stack, child_nodes(node, SyntaxKind::Block), Some(start));
                    stack.extend(params.into_iter().rev().map(|param| Step::Declare(param, start, Some(start))));
                    stack.push(Step::OpenScope);
                }
                SyntaxKind::ForCountLoop | SyntaxKind::ForInLoop => {
                    let vars: Vec<&'a str> = child_tokens(node, SyntaxKind::Name).into_iter()
                        .chain(child_nodes(node, SyntaxKind::NameList).into_iter()
                            .flat_map(|list| child_tokens(list, SyntaxKind::Name)))
                        .collect();
                    stack.push(Step::CloseScope);
                    visit_in_order(&mut stack, child_nodes(node, SyntaxKind::Block), func);
                    stack.extend(vars.into_iter().rev().map(|var| Step::Declare(var, start, func)));
                    stack.push(Step::OpenScope);
                    // The bounds and the iterator are evaluated outside the loop.
                    let outside = node.children().filter(|n| n.kind() != SyntaxKind::Block).collect();
                    visit_in_order(&mut stack, outside, func);
                }
                _ => visit_in_order(&mut stack, node.children().collect(), func),
            }
        }
        self.local_written_names = writes.keys().map(|&(_, name)| name).collect();
        self.local_writes = writes;
    }

    /// The start of the innermost function whose body contains `scope` (`None`
    /// at file level).
    fn enclosing_function_start(&self, scope: ScopeIndex) -> Option<u32> {
        self.ir.ancestor_scopes(scope).find_map(|s| self.function_starts.get(&s).copied())
    }

    /// Whether `sym` is a plain local, as opposed to a global or a parameter
    /// (which keeps its declared or inferred type) or an annotated local (which
    /// keeps its declared one). A function body starting can't have declared
    /// any other local yet.
    fn is_plain_local(&self, sym: SymbolIndex) -> bool {
        !sym.is_external()
            && self.ir.local_decl_symbols.contains(&sym)
            && !self.ir.param_symbols.contains(&sym)
            && !self.ir.symbol_type_annotations.contains_key(&sym)
            && !self.ir.class_def_symbols.contains(&sym)
    }

    /// Give the function whose body starts here an entry version of each local
    /// it reads from outside that the file writes where this function can't see
    /// it coming: in another function, or in the declaring one after this
    /// function is created.
    pub(super) fn push_upvalue_entry_versions(&mut self, func_idx: FunctionIndex, body: SyntaxNode<'a>, body_scope: ScopeIndex) {
        if self.local_writes.is_empty() {
            return;
        }
        let func_start = self.ir.functions[func_idx.val()].def_node.start;
        let mut seen: HashSet<SymbolIndex> = HashSet::default();
        let mut upvalues = Vec::new();
        for name in body_read_names(body) {
            if !self.local_written_names.contains(name) {
                continue;
            }
            let Some(sym) = self.get_symbol(&SymbolIdentifier::Name(name.to_string()), body_scope) else { continue };
            if !seen.insert(sym) || !self.is_plain_local(sym) {
                continue;
            }
            let prior = self.ir.version_for_scope(sym, body_scope);
            let decl = self.ir.symbols[sym.val()].versions[self.ir.binding_start(sym, prior)].def_node.start;
            let Some(local) = self.local_writes.get(&(decl, name)) else { continue };
            let valued = !self.ir.is_valueless_version(sym, prior);
            if local.writes.iter().any(|w| (w.func != local.decl_func || w.end > func_start)
                && !(valued && w.start <= func_start && func_start < w.end))
            {
                upvalues.push((sym, prior));
            }
        }
        for (sym, prior) in upvalues {
            let merge = self.ir.push_expr(Expr::SymbolRef(sym, prior));
            let def_node = self.ir.symbols[sym.val()].versions[prior].def_node;
            let creation_order = self.ir.next_order();
            let versions = &mut self.ir.symbols[sym.val()].versions;
            versions.push(SymbolVersion {
                def_node,
                type_source: Some(merge),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: body_scope,
                creation_order,
                original_type_source: None,
            });
            let ver = versions.len() - 1;
            // Not a write: hidden from the enclosing scope and from branch merges.
            self.ir.mark_narrowing_only_version(sym, ver);
            self.ir.upvalue_entries.insert((sym, ver), UpvalueEntry { prior, merge, body_scope, uninitialized: false });
        }
    }

    /// Type each entry version now that every write to its local is known: the
    /// version visible where the function is defined, plus the writes that may
    /// have run before the function does — those in other functions, and those
    /// in the declaring function after this one is created.
    pub(super) fn finish_upvalue_entry_versions(&mut self) {
        let mut entries: Vec<(SymbolIndex, usize)> = self.ir.upvalue_entries.keys().copied().collect();
        // Outer functions' entries first: an inner entry can read one.
        entries.sort_unstable();
        for (sym, ver) in entries {
            let entry = self.ir.upvalue_entries[&(sym, ver)].clone();
            let start = self.ir.binding_start(sym, entry.prior);
            let end = self.ir.binding_end(sym, entry.prior);
            let valueless = self.ir.is_valueless_version(sym, entry.prior);
            let uninitialized = (entry.prior == start && valueless)
                || self.ir.upvalue_entries.get(&(sym, entry.prior)).is_some_and(|e| e.uninitialized);
            let symbol = &self.ir.symbols[sym.val()];
            let decl_func = self.enclosing_function_start(symbol.scope_idx);
            let func_start = self.function_starts.get(&entry.body_scope).copied();
            let body_order = self.ir.scopes[entry.body_scope.val()].creation_order;
            let mut writes: Vec<ExprId> = Vec::new();
            for (i, v) in symbol.versions.iter().enumerate().take(end).skip(start) {
                let Some(ts) = v.type_source.filter(|_| self.ir.is_write_version(sym, i)) else { continue };
                let precedes = v.creation_order < body_order
                    && self.enclosing_function_start(v.created_in_scope) == decl_func;
                // A function built while computing the local's new value reads
                // the old one (`x = f(x, function() return x end)`), unless there
                // is none (`t = NewTicker(1, function() t:Cancel() end)`).
                let reads_old_value = !valueless
                    && func_start.is_some_and(|f| v.def_node.start <= f && f < v.def_node.end);
                let unseen = !(precedes || reads_old_value);
                if unseen && !writes.contains(&ts) {
                    writes.push(ts);
                }
            }
            if !writes.is_empty() {
                if !uninitialized {
                    writes.insert(0, self.ir.push_expr(Expr::SymbolRef(sym, entry.prior)));
                }
                self.ir.exprs[entry.merge.val()] = Expr::BranchMerge(writes);
                self.ir.upvalue_merges.insert(entry.merge);
            }
            if let Some(e) = self.ir.upvalue_entries.get_mut(&(sym, ver)) {
                e.uninitialized = uninitialized;
            }
        }
    }
}
