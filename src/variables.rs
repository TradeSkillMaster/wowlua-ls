use std::collections::HashMap;
use std::sync::Arc;

use rowan::GreenNode;
use crate::ast::*;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr};
use crate::annotations::{AnnotationType, extract_annotations, scan_all_annotations};

// ── Signature Help result types ────────────────────────────────────────────────

pub struct SignatureInfo {
    pub label: String,
    pub params: Vec<String>,
    pub doc: Option<String>,
}

pub struct HoverResult {
    pub type_str: String,
    pub doc: Option<String>,
}

pub struct SignatureHelpResult {
    pub signatures: Vec<SignatureInfo>,
    pub active_signature: Option<u32>,
    pub active_parameter: u32,
}

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolType {
    Unknown,
    Value(ValueType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValueType {
    Nil,
    Boolean(Option<bool>),
    Number,
    String,
    Function(Option<FunctionIndex>),
    Table(Option<TableIndex>),
    Union(Vec<ValueType>),
    // TODO: Thread, Userdata
}

impl ValueType {
    fn can_concat_to_string(&self) -> bool {
        match self {
            ValueType::Nil => false,
            ValueType::Boolean(_) => true,
            ValueType::Number => true,
            ValueType::String => true,
            ValueType::Function(_) => false,
            ValueType::Table(_) => false,
            ValueType::Union(_) => false,
        }
    }

    pub fn union(a: ValueType, b: ValueType) -> ValueType {
        let mut types = Vec::new();
        match a {
            ValueType::Union(inner) => types.extend(inner),
            other => types.push(other),
        }
        match b {
            ValueType::Union(inner) => types.extend(inner),
            other => types.push(other),
        }
        types.dedup();
        if types.len() == 1 {
            types.into_iter().next().unwrap()
        } else {
            ValueType::Union(types)
        }
    }
}

// ── Symbol and Scope structures ────────────────────────────────────────────────

type ScopeIndex = usize;
type SymbolIndex = usize;
type FunctionIndex = usize;
type TableIndex = usize;
type ExprId = usize;

/// External globals use indices >= EXT_BASE to avoid conflicts with local indices.
/// Pre-built at startup, shared across files — never cloned per-file.
const EXT_BASE: usize = 1_000_000;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum SymbolIdentifier {
    Name(String),
    FunctionRet(FunctionIndex, usize),
}

#[derive(Debug, Clone)]
struct Scope {
    parent: Option<ScopeIndex>,
    symbols: HashMap<SymbolIdentifier, SymbolIndex>,
}

#[derive(Debug, Clone)]
struct Symbol {
    id: SymbolIdentifier,
    scope_idx: ScopeIndex,
    versions: Vec<SymbolVersion>,
}

#[derive(Debug, Clone)]
struct SymbolVersion {
    def_node: SyntaxNodePtr,
    type_source: Option<ExprId>,
    resolved_type: Option<SymbolType>,
}

/// A resolved overload signature: param types + return types.
#[derive(Debug, Clone, PartialEq)]
struct ResolvedOverload {
    params: Vec<(String, Option<ValueType>)>,
    returns: Vec<ValueType>,
}

#[derive(Debug, Clone, PartialEq)]
struct Function {
    def_node: SyntaxNodePtr,
    scope: ScopeIndex,
    args: Vec<SymbolIndex>,
    rets: Vec<SymbolIndex>,
    return_annotations: Vec<ValueType>,
    overloads: Vec<ResolvedOverload>,
    doc: Option<String>,
}

#[derive(Debug, Clone)]
struct TableInfo {
    fields: HashMap<String, ExprId>,
    class_name: Option<String>,
}

// ── Expression IR ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Expr {
    Literal(ValueType),
    SymbolRef(SymbolIndex, usize), // symbol_idx, version_idx
    BinaryOp { op: Operator, lhs: ExprId, rhs: ExprId },
    UnaryOp { op: Operator, operand: ExprId },
    Grouped(ExprId),
    FunctionCall { func: ExprId, args: Vec<ExprId>, ret_index: usize },
    FunctionDef(FunctionIndex),
    TableConstructor(TableIndex),
    FieldAccess { table: ExprId, field: String },
    Unknown,
}

// ── Main struct ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Variables {
    root: SyntaxNode,
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    functions: Vec<Function>,
    tables: Vec<TableInfo>,
    exprs: Vec<Expr>,
    block_scopes: Vec<(rowan::TextRange, ScopeIndex)>,
    classes: HashMap<String, TableIndex>,
    aliases: HashMap<String, ValueType>,
    // External globals (shared across files, never cloned per-file)
    ext: Arc<PreResolvedGlobals>,
}

impl Variables {
    pub fn new(
        green: GreenNode,
        pre_globals: Arc<PreResolvedGlobals>,
    ) -> Variables {
        let root = SyntaxNode::new_root(green);
        let mut variables = Variables {
            root,
            scopes: Vec::new(),
            symbols: Vec::new(),
            functions: Vec::new(),
            tables: Vec::new(),
            exprs: Vec::new(),
            block_scopes: Vec::new(),
            classes: HashMap::new(),
            aliases: HashMap::new(),
            ext: pre_globals,
        };
        variables.prescan_classes_and_aliases();
        variables.build_ir();
        variables.inject_preresolved();
        variables
    }

    // Two-tier lookup: indices < EXT_BASE are local, >= EXT_BASE are external
    fn sym(&self, idx: SymbolIndex) -> &Symbol {
        if idx >= EXT_BASE {
            &self.ext.symbols[idx - EXT_BASE]
        } else {
            &self.symbols[idx]
        }
    }

    fn func(&self, idx: FunctionIndex) -> &Function {
        if idx >= EXT_BASE {
            &self.ext.functions[idx - EXT_BASE]
        } else {
            &self.functions[idx]
        }
    }

    fn expr(&self, idx: ExprId) -> &Expr {
        if idx >= EXT_BASE {
            &self.ext.exprs[idx - EXT_BASE]
        } else {
            &self.exprs[idx]
        }
    }

    fn table(&self, idx: TableIndex) -> &TableInfo {
        if idx >= EXT_BASE {
            &self.ext.tables[idx - EXT_BASE]
        } else {
            &self.tables[idx]
        }
    }

    pub fn dump(&self) {
        println!("Symbols:");
        for symbol in self.symbols.iter() {
            println!("    {:?} (scope_idx: {:?}):", &symbol.id, &symbol.scope_idx);
            for version in &symbol.versions {
                println!("        def: {:?}, source: {:?}, resolved: {:?}",
                    version.def_node, version.type_source, version.resolved_type);
            }
        }
        println!("Functions:");
        for (i, func) in self.functions.iter().enumerate() {
            println!("    [{}] {:?}", i, func);
        }
        println!("Tables:");
        for (i, table) in self.tables.iter().enumerate() {
            let class_label = table.class_name.as_deref().unwrap_or("");
            println!("    [{}] {} fields: {:?}", i, class_label, table.fields.keys().collect::<Vec<_>>());
        }
        if !self.classes.is_empty() {
            println!("Classes:");
            for (name, table_idx) in &self.classes {
                println!("    {} -> table[{}]", name, table_idx);
            }
        }
        if !self.aliases.is_empty() {
            println!("Aliases:");
            for (name, vt) in &self.aliases {
                println!("    {} -> {:?}", name, vt);
            }
        }
    }
}

// ── Pre-resolved External Globals ─────────────────────────────────────────────
//
// Built once at startup from workspace scan results. Contains pre-built
// Function/Symbol/Scope/Expr entries with 0-based internal indices.
// Injected into each file's Variables with index offsets (~0.1ms vs ~35ms).

#[derive(Debug)]
pub struct PreResolvedGlobals {
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    functions: Vec<Function>,
    exprs: Vec<Expr>,
    tables: Vec<TableInfo>,
    classes: HashMap<String, TableIndex>,
    aliases: HashMap<String, ValueType>,
    scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
}

impl PreResolvedGlobals {
    pub fn empty() -> PreResolvedGlobals {
        PreResolvedGlobals {
            scopes: Vec::new(),
            symbols: Vec::new(),
            functions: Vec::new(),
            exprs: Vec::new(),
            tables: Vec::new(),
            classes: HashMap::new(),
            aliases: HashMap::new(),
            scope0_symbols: HashMap::new(),
        }
    }

    pub fn build(
        globals: &[crate::annotations::ExternalGlobal],
        external_classes: &[(String, Vec<String>, Vec<(String, AnnotationType)>)],
        external_aliases: &[(String, AnnotationType)],
    ) -> PreResolvedGlobals {
        use crate::annotations::ExternalGlobalKind;

        // All indices in this method use EXT_BASE so they're directly usable
        // in the global index space without any per-file adjustment.

        let mut scopes = Vec::new();
        let mut symbols = Vec::new();
        let mut functions = Vec::new();
        let mut exprs: Vec<Expr> = Vec::new();
        let mut tables: Vec<TableInfo> = Vec::new();
        let mut classes: HashMap<String, TableIndex> = HashMap::new();
        let mut aliases: HashMap<String, ValueType> = HashMap::new();

        // ── Step 1: Build classes and aliases ──────────────────────────────

        // Pass 1: Register all class names (table indices use EXT_BASE)
        for (class_name, _parents, _fields) in external_classes {
            let table_idx = EXT_BASE + tables.len();
            tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class_name.clone()),
            });
            classes.insert(class_name.clone(), table_idx);
        }

        // Pass 2: Populate @field entries (expr indices use EXT_BASE)
        for (class_name, _parents, fields) in external_classes {
            let table_idx = classes[class_name];
            let local_idx = table_idx - EXT_BASE;
            for (field_name, annotation_type) in fields {
                if let Some(vt) = Self::resolve_annotation(annotation_type, &classes, &aliases) {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt));
                    tables[local_idx].fields.insert(field_name.clone(), expr_idx);
                }
            }
        }

        // Register aliases
        for (alias_name, annotation_type) in external_aliases {
            if let Some(vt) = Self::resolve_annotation(annotation_type, &classes, &aliases) {
                aliases.insert(alias_name.clone(), vt);
            }
        }

        // ── Step 2: Build external global entries ──────────────────────────

        // Dummy SyntaxNodePtr (parse a trivial string to get a valid root node)
        let mut parser = crate::syntax::syntax::Generator::new("--");
        let green = parser.process_all();
        let root = crate::syntax::syntax::SyntaxNode::new_root(green);
        let dummy_node = SyntaxNodePtr::new(&root);

        // Create non-class tables in shared data (e.g. math, string, table)
        let mut non_class_tables: HashMap<String, TableIndex> = HashMap::new();
        for g in globals {
            if let ExternalGlobalKind::Table = &g.kind {
                if !classes.contains_key(&g.name) && !non_class_tables.contains_key(&g.name) {
                    let table_idx = EXT_BASE + tables.len();
                    tables.push(TableInfo { fields: HashMap::new(), class_name: None });
                    non_class_tables.insert(g.name.clone(), table_idx);
                }
            }
        }

        // Build method function entries and add directly to class/table tables.
        // Done BEFORE inheritance so methods are inherited by child classes.
        let mut seen_methods: HashMap<(&str, &str), ()> = HashMap::new();
        for g in globals {
            if let ExternalGlobalKind::Method(method_name, _is_colon) = &g.kind {
                let target_table = classes.get(&g.name).or_else(|| non_class_tables.get(&g.name));
                let Some(&table_idx) = target_table else { continue; };
                if seen_methods.contains_key(&(g.name.as_str(), method_name.as_str())) { continue; }
                seen_methods.insert((&g.name, method_name), ());

                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &classes, &aliases,
                );
                let expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));

                let local_idx = table_idx - EXT_BASE;
                tables[local_idx].fields.entry(method_name.clone()).or_insert(expr_id);
            }
        }

        // Pass 3: Resolve inheritance (transitive via fixpoint loop).
        // Each iteration copies parent fields/methods into children.
        // Repeats until no new fields are added, propagating through
        // the full hierarchy (e.g. Object → ScriptRegion → Region → Frame).
        loop {
            let mut changed = false;
            for (class_name, parents, _fields) in external_classes {
                if parents.is_empty() { continue; }
                let child_local = classes[class_name] - EXT_BASE;
                for parent_name in parents {
                    if let Some(&parent_idx) = classes.get(parent_name.as_str()) {
                        let parent_local = parent_idx - EXT_BASE;
                        let parent_fields: Vec<(String, ExprId)> =
                            tables[parent_local].fields.iter()
                                .map(|(k, v)| (k.clone(), *v))
                                .collect();
                        for (fname, expr_id) in parent_fields {
                            if let std::collections::hash_map::Entry::Vacant(e) = tables[child_local].fields.entry(fname) {
                                e.insert(expr_id);
                                changed = true;
                            }
                        }
                    }
                }
            }
            if !changed { break; }
        }

        // Build global function entries
        let mut scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex> = HashMap::new();
        let mut seen_functions: HashMap<&str, ()> = HashMap::new();
        for g in globals {
            if let ExternalGlobalKind::Function = &g.kind {
                if seen_functions.contains_key(g.name.as_str()) { continue; }
                seen_functions.insert(&g.name, ());

                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &classes, &aliases,
                );
                let _expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));

                let sym_idx = EXT_BASE + symbols.len();
                symbols.push(Symbol {
                    id: SymbolIdentifier::Name(g.name.clone()),
                    scope_idx: 0,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type: Some(SymbolType::Value(
                            ValueType::Function(Some(func_idx)),
                        )),
                    }],
                });
                scope0_symbols.insert(SymbolIdentifier::Name(g.name.clone()), sym_idx);
            }
        }

        // Register non-class tables as scope0 symbols
        for (name, &table_idx) in &non_class_tables {
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(name.clone()),
                scope_idx: 0,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: Some(SymbolType::Value(
                        ValueType::Table(Some(table_idx)),
                    )),
                }],
            });
            scope0_symbols.insert(SymbolIdentifier::Name(name.clone()), sym_idx);
        }

        PreResolvedGlobals {
            scopes, symbols, functions, exprs, tables,
            classes, aliases, scope0_symbols,
        }
    }

    fn resolve_annotation(
        at: &AnnotationType,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
    ) -> Option<ValueType> {
        match at {
            AnnotationType::Simple(name) => {
                match name.as_str() {
                    "nil" => return Some(ValueType::Nil),
                    "boolean" | "bool" => return Some(ValueType::Boolean(None)),
                    "number" | "integer" => return Some(ValueType::Number),
                    "string" => return Some(ValueType::String),
                    "table" => return Some(ValueType::Table(None)),
                    "function" | "fun" => return Some(ValueType::Function(None)),
                    "any" => return None,
                    _ => {}
                }
                if (name.starts_with('"') && name.ends_with('"'))
                    || (name.starts_with('\'') && name.ends_with('\''))
                {
                    return Some(ValueType::String);
                }
                if let Some(&table_idx) = classes.get(name.as_str()) {
                    return Some(ValueType::Table(Some(table_idx)));
                }
                if let Some(vt) = aliases.get(name.as_str()) {
                    return Some(vt.clone());
                }
                None
            }
            AnnotationType::Union(parts) => {
                let converted: Vec<ValueType> = parts.iter()
                    .filter_map(|p| Self::resolve_annotation(p, classes, aliases))
                    .collect();
                match converted.len() {
                    0 => None,
                    1 => converted.into_iter().next(),
                    _ => {
                        let mut iter = converted.into_iter();
                        let mut result = iter.next().unwrap();
                        for vt in iter {
                            result = ValueType::union(result, vt);
                        }
                        Some(result)
                    }
                }
            }
        }
    }

    /// Build a Function entry. All returned indices use EXT_BASE so they're
    /// directly usable in the global index space without per-file adjustment.
    fn build_function(
        params: &[(String, AnnotationType)],
        returns: &[AnnotationType],
        overload_sigs: &[crate::annotations::OverloadSig],
        doc: Option<String>,
        dummy_node: SyntaxNodePtr,
        scopes: &mut Vec<Scope>,
        symbols: &mut Vec<Symbol>,
        functions: &mut Vec<Function>,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
    ) -> FunctionIndex {
        let func_scope_local = scopes.len();
        let func_scope = EXT_BASE + func_scope_local;
        scopes.push(Scope {
            parent: Some(0),
            symbols: HashMap::new(),
        });

        let mut arg_symbols = Vec::new();
        for (param_name, param_type) in params {
            let resolved = Self::resolve_annotation(param_type, classes, aliases)
                .map(SymbolType::Value);
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(param_name.clone()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: resolved,
                }],
            });
            scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::Name(param_name.clone()), sym_idx,
            );
            arg_symbols.push(sym_idx);
        }

        let return_annotations: Vec<ValueType> = returns.iter()
            .filter_map(|rt| Self::resolve_annotation(rt, classes, aliases))
            .collect();

        let func_idx = EXT_BASE + functions.len();
        let mut ret_symbols = Vec::new();
        for (i, ret_type) in return_annotations.iter().enumerate() {
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::FunctionRet(func_idx, i),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: Some(SymbolType::Value(ret_type.clone())),
                }],
            });
            ret_symbols.push(sym_idx);
        }

        let overloads: Vec<ResolvedOverload> = overload_sigs.iter().map(|sig| {
            let params = sig.params.iter().map(|(name, at)| {
                (name.clone(), Self::resolve_annotation(at, classes, aliases))
            }).collect();
            let returns = sig.returns.iter()
                .filter_map(|at| Self::resolve_annotation(at, classes, aliases))
                .collect();
            ResolvedOverload { params, returns }
        }).collect();

        functions.push(Function {
            def_node: dummy_node,
            scope: func_scope,
            args: arg_symbols,
            rets: ret_symbols,
            return_annotations,
            overloads,
            doc,
        });

        func_idx
    }
}

// ── Annotation Pre-scan (Phase 0) ─────────────────────────────────────────────

impl Variables {
    fn prescan_classes_and_aliases(&mut self) {
        // Import external classes/aliases from PreResolvedGlobals (cheap map clone)
        let ext = Arc::clone(&self.ext);
        for (name, &table_idx) in &ext.classes {
            self.classes.insert(name.clone(), table_idx);
        }
        for (name, vt) in &ext.aliases {
            self.aliases.insert(name.clone(), vt.clone());
        }

        // Process file-local declarations only
        let (local_classes, local_aliases, _has_meta) = scan_all_annotations(&self.root);

        // Pass 1: Register local class names with empty tables (local indices)
        for (class_name, _parents, _fields) in &local_classes {
            let table_idx = self.tables.len();
            self.tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class_name.clone()),
            });
            self.classes.insert(class_name.clone(), table_idx);
        }

        // Pass 2: Populate local class fields
        for (class_name, _parents, fields) in &local_classes {
            let table_idx = self.classes[class_name];
            for (field_name, annotation_type) in fields {
                if let Some(vt) = self.resolve_annotation_type(annotation_type) {
                    let expr_id = self.push_expr(Expr::Literal(vt));
                    self.tables[table_idx].fields.insert(field_name.clone(), expr_id);
                }
            }
        }

        // Pass 3: Resolve inheritance (transitive via fixpoint loop).
        // Parent may be external (>= EXT_BASE, already fully resolved) or local.
        loop {
            let mut changed = false;
            for (class_name, parents, _fields) in &local_classes {
                if parents.is_empty() { continue; }
                let child_idx = self.classes[class_name];
                for parent_name in parents {
                    if let Some(&parent_idx) = self.classes.get(parent_name.as_str()) {
                        let parent_fields: Vec<(String, ExprId)> =
                            self.table(parent_idx).fields.iter()
                                .map(|(k, v)| (k.clone(), *v))
                                .collect();
                        for (fname, expr_id) in parent_fields {
                            if let std::collections::hash_map::Entry::Vacant(e) = self.tables[child_idx].fields.entry(fname) {
                                e.insert(expr_id);
                                changed = true;
                            }
                        }
                    }
                }
            }
            if !changed { break; }
        }

        // Register local aliases
        for (alias_name, annotation_type) in &local_aliases {
            if let Some(vt) = self.resolve_annotation_type(annotation_type) {
                self.aliases.insert(alias_name.clone(), vt);
            }
        }
    }

    /// Minimal per-file injection: only non-class global tables (a few dozen).
    /// Class tables and scope0 functions are handled via two-tier lookups.
    fn inject_preresolved(&mut self) {
        // Non-class tables (math, string, table, etc.) are now fully built
        // in PreResolvedGlobals and accessible via scope0_symbols / EXT_BASE tables.
        // Nothing to inject per-file.
    }

    fn resolve_annotation_type(&self, at: &AnnotationType) -> Option<ValueType> {
        match at {
            AnnotationType::Simple(name) => {
                // Primitives
                match name.as_str() {
                    "nil" => return Some(ValueType::Nil),
                    "boolean" | "bool" => return Some(ValueType::Boolean(None)),
                    "number" | "integer" => return Some(ValueType::Number),
                    "string" => return Some(ValueType::String),
                    "table" => return Some(ValueType::Table(None)),
                    "function" | "fun" => return Some(ValueType::Function(None)),
                    "any" => return None,
                    _ => {}
                }
                // Quoted string literals (e.g. "TOPLEFT" in aliases)
                if (name.starts_with('"') && name.ends_with('"'))
                    || (name.starts_with('\'') && name.ends_with('\''))
                {
                    return Some(ValueType::String);
                }
                // Class lookup
                if let Some(&table_idx) = self.classes.get(name.as_str()) {
                    return Some(ValueType::Table(Some(table_idx)));
                }
                // Alias lookup
                if let Some(vt) = self.aliases.get(name.as_str()) {
                    return Some(vt.clone());
                }
                None
            }
            AnnotationType::Union(parts) => {
                let converted: Vec<ValueType> = parts.iter()
                    .filter_map(|p| self.resolve_annotation_type(p))
                    .collect();
                match converted.len() {
                    0 => None,
                    1 => converted.into_iter().next(),
                    _ => {
                        let mut iter = converted.into_iter();
                        let mut result = iter.next().unwrap();
                        for vt in iter {
                            result = ValueType::union(result, vt);
                        }
                        Some(result)
                    }
                }
            }
        }
    }
}

// ── IR Building (Phase 1) ──────────────────────────────────────────────────────

impl Variables {
    fn build_ir(&mut self) {
        self.scopes.push(Scope {
            parent: None,
            symbols: HashMap::new(),
        });

        #[derive(Clone)]
        struct Frame {
            block: Block,
            next_stmt: usize,
            scope_idx: ScopeIndex,
            func_id: Option<FunctionIndex>,
        }

        let root_block = Block::cast(self.root.clone()).expect("everything starts with a block");
        let mut stack = vec![Frame {
            block: root_block,
            next_stmt: 0,
            scope_idx: 0,
            func_id: None,
        }];

        while let Some(frame) = stack.last_mut() {
            let scope_idx = frame.scope_idx;
            let func_id = frame.func_id;
            if frame.next_stmt == 0 {
                self.block_scopes.push((frame.block.syntax().text_range(), scope_idx));
            }
            let statements = frame.block.statements();
            if frame.next_stmt >= statements.len() {
                stack.pop();
                continue;
            }

            let stmt_index = frame.next_stmt;
            frame.next_stmt += 1;
            match &statements[stmt_index] {
                Statement::LocalAssign(assign) => {
                    let node = SyntaxNodePtr::new(assign.syntax());
                    let names = assign
                        .name_list()
                        .expect("LocalAssign should have a name_list")
                        .names();
                    let expressions = assign
                        .expression_list()
                        .expect("LocalAssign should have an expression_list")
                        .expressions();

                    for (index, name) in names.iter().enumerate() {
                        let expression = expressions.get(index);

                        if let Some(Expression::Function(func)) = expression {
                            // Function: insert symbol first (so function can be recursive),
                            // then create function scope
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = self.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, assign.syntax());
                            let expr_id = self.push_expr(Expr::FunctionDef(func_idx));
                            self.set_type_source(symbol_idx, expr_id);
                            let inner_block = func.block().expect("FunctionDefinition must have a block");
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id: Some(func_idx),
                            });
                        } else {
                            // Non-function: lower RHS BEFORE insert_symbol so that
                            // `local x = x + 1` resolves the old `x`, not the new one
                            let type_source = if let Some(expr) = expression {
                                Some(self.lower_expression(expr, scope_idx))
                            } else if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                if index >= expressions.len() {
                                    // Multi-return: this name gets a later return value
                                    let ret_index = index - (expressions.len() - 1);
                                    Some(self.lower_function_call(call, scope_idx, ret_index))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            if let Some(expr_id) = type_source {
                                self.set_type_source(symbol_idx, expr_id);
                            }
                            // Apply @type and @class annotations (first variable only)
                            if index == 0 {
                                let annotations = extract_annotations(assign.syntax());
                                if let Some(ref at) = annotations.var_type {
                                    if let Some(vt) = self.resolve_annotation_type(at) {
                                        let expr_id = self.push_expr(Expr::Literal(vt));
                                        self.set_type_source(symbol_idx, expr_id);
                                    }
                                }
                                if let Some(ref class_name) = annotations.class {
                                    if let Some(&class_table_idx) = self.classes.get(class_name) {
                                        // Merge runtime table fields into the class table
                                        if let Some(rhs_expr_id) = self.symbols[symbol_idx]
                                            .versions.last()
                                            .and_then(|v| v.type_source)
                                        {
                                            if let Some(rhs_table_idx) = self.find_table_index(rhs_expr_id) {
                                                if rhs_table_idx != class_table_idx {
                                                    let runtime_fields: Vec<(String, ExprId)> =
                                                        self.tables[rhs_table_idx].fields.drain().collect();
                                                    for (name, expr_id) in runtime_fields {
                                                        self.tables[class_table_idx].fields
                                                            .entry(name).or_insert(expr_id);
                                                    }
                                                }
                                            }
                                        }
                                        let expr_id = self.push_expr(Expr::Literal(
                                            ValueType::Table(Some(class_table_idx))
                                        ));
                                        self.set_type_source(symbol_idx, expr_id);
                                    }
                                }
                            }
                        }
                    }
                },
                Statement::Do(group) => {
                    let inner_block = group.block().expect("DoGroup must have a block");
                    let new_scope_idx = self.insert_scope(Some(scope_idx));
                    stack.push(Frame {
                        block: inner_block,
                        next_stmt: 0,
                        scope_idx: new_scope_idx,
                        func_id,
                    });
                },
                Statement::While(while_loop) => {
                    let inner_block = while_loop.block().expect("WhileLoop must have a block");
                    let new_scope_idx = self.insert_scope(Some(scope_idx));
                    stack.push(Frame {
                        block: inner_block,
                        next_stmt: 0,
                        scope_idx: new_scope_idx,
                        func_id,
                    });
                },
                Statement::Repeat(repeat_loop) => {
                    let inner_block = repeat_loop.block().expect("RepeatUntilLoop must have a block");
                    let new_scope_idx = self.insert_scope(Some(scope_idx));
                    stack.push(Frame {
                        block: inner_block,
                        next_stmt: 0,
                        scope_idx: new_scope_idx,
                        func_id,
                    });
                },
                Statement::If(if_chain) => {
                    for branch in if_chain.if_branches() {
                        if let Some(inner_block) = branch.block() {
                            let new_scope_idx = self.insert_scope(Some(scope_idx));
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                            });
                        }
                    }
                    if let Some(else_branch) = if_chain.else_branch() {
                        if let Some(inner_block) = else_branch.block() {
                            let new_scope_idx = self.insert_scope(Some(scope_idx));
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                            });
                        }
                    }
                },
                Statement::ForCountLoop(for_loop) => {
                    let inner_block = for_loop.block().expect("ForCountLoop must have a block");
                    let new_scope_idx = self.insert_scope(Some(scope_idx));
                    if let Some(name) = for_loop.name() {
                        let node = SyntaxNodePtr::new(for_loop.syntax());
                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name), new_scope_idx, node);
                        let expr_id = self.push_expr(Expr::Literal(ValueType::Number));
                        self.set_type_source(symbol_idx, expr_id);
                    }
                    stack.push(Frame {
                        block: inner_block,
                        next_stmt: 0,
                        scope_idx: new_scope_idx,
                        func_id,
                    });
                },
                Statement::ForInLoop(for_in) => {
                    let inner_block = for_in.block().expect("ForInLoop must have a block");
                    let new_scope_idx = self.insert_scope(Some(scope_idx));
                    if let Some(name_list) = for_in.name_list() {
                        let node = SyntaxNodePtr::new(for_in.syntax());
                        for name in name_list.names() {
                            self.insert_symbol(SymbolIdentifier::Name(name), new_scope_idx, node);
                            // type_source stays None — iterator protocol types unknown
                        }
                    }
                    stack.push(Frame {
                        block: inner_block,
                        next_stmt: 0,
                        scope_idx: new_scope_idx,
                        func_id,
                    });
                },
                Statement::FunctionDefinition(func) => {
                    let node = SyntaxNodePtr::new(func.syntax());
                    if let Some(name) = func.name() {
                        // Simple name: function foo()
                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name), scope_idx, node);
                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                        let func_idx = self.functions.len() - 1;
                        self.apply_annotations(func_idx, scope_idx, func.syntax());
                        let expr_id = self.push_expr(Expr::FunctionDef(func_idx));
                        self.set_type_source(symbol_idx, expr_id);
                        let inner_block = func.block().expect("FunctionDefinition must have a block");
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id: Some(func_idx),
                        });
                    } else if let Some(ident) = func.identifier() {
                        let names = ident.names();
                        if names.len() == 1 {
                            // Global function with Identifier wrapper: function foo()
                            let name = &names[0];
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = self.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, func.syntax());
                            let expr_id = self.push_expr(Expr::FunctionDef(func_idx));
                            self.set_type_source(symbol_idx, expr_id);
                            let inner_block = func.block().expect("FunctionDefinition must have a block");
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id: Some(func_idx),
                            });
                        } else if names.len() >= 2 {
                            let root_name = &names[0];
                            let field_name = &names[names.len() - 1];
                            let is_method = ident.is_call_to_self();

                            let new_scope_idx = self.insert_function_definition(func, scope_idx, is_method);
                            let func_idx = self.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, func.syntax());
                            let func_def_expr = self.push_expr(Expr::FunctionDef(func_idx));

                            // Give `self` a type pointing to the table
                            if is_method {
                                if let Some(table_sym_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                                    let self_sym_idx = self.functions[func_idx].args[0];
                                    let ver_idx = self.symbols[table_sym_idx].versions.len() - 1;
                                    let self_expr = self.push_expr(Expr::SymbolRef(table_sym_idx, ver_idx));
                                    self.set_type_source(self_sym_idx, self_expr);
                                }
                            }

                            // Record as field on the table
                            if let Some(table_idx) = self.find_table_for_symbol(root_name, scope_idx) {
                                self.tables[table_idx].fields.insert(field_name.clone(), func_def_expr);
                            }

                            let inner_block = func.block().expect("FunctionDefinition must have a block");
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id: Some(func_idx),
                            });
                        }
                    }
                },
                Statement::Return(ret) => {
                    if let (Some(expr_list), Some(func_id)) = (ret.expression_list(), func_id) {
                        let node = SyntaxNodePtr::new(ret.syntax());
                        let expressions = expr_list.expressions();
                        for (index, expr) in expressions.iter().enumerate() {
                            let expr_id = self.lower_expression(expr, scope_idx);
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                            self.set_type_source(symbol_idx, expr_id);
                            let func = self.functions.get_mut(func_id).unwrap();
                            if !func.rets.contains(&symbol_idx) {
                                func.rets.push(symbol_idx);
                            }
                        }
                    }
                },
                Statement::Assign(assign) => {
                    let node = SyntaxNodePtr::new(assign.syntax());
                    if let Some(var_list) = assign.variable_list() {
                        let identifiers = var_list.identifiers();
                        let expressions = assign
                            .expression_list()
                            .map(|el| el.expressions())
                            .unwrap_or_default();
                        for (index, ident) in identifiers.iter().enumerate() {
                            let names = ident.names();
                            if let Some(root_name) = names.first() {
                                let expression = expressions.get(index);

                                if names.len() > 1 {
                                    // Dotted assignment: t.x = expr
                                    let field_name = &names[names.len() - 1];

                                    if let Some(Expression::Function(func)) = expression {
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let func_def_expr = self.push_expr(Expr::FunctionDef(func_idx));
                                        if let Some(table_idx) = self.find_table_for_symbol(root_name, scope_idx) {
                                            self.tables[table_idx].fields.insert(field_name.clone(), func_def_expr);
                                        }
                                        let inner_block = func.block().expect("FunctionDefinition must have a block");
                                        stack.push(Frame {
                                            block: inner_block,
                                            next_stmt: 0,
                                            scope_idx: new_scope_idx,
                                            func_id: Some(func_idx),
                                        });
                                    } else if let Some(expr) = expression {
                                        let expr_id = self.lower_expression(expr, scope_idx);
                                        if let Some(table_idx) = self.find_table_for_symbol(root_name, scope_idx) {
                                            self.tables[table_idx].fields.insert(field_name.clone(), expr_id);
                                        }
                                    }
                                } else {
                                    // Simple assignment: x = expr
                                    if let Some(Expression::Function(func)) = expression {
                                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let expr_id = self.push_expr(Expr::FunctionDef(func_idx));
                                        self.set_type_source(symbol_idx, expr_id);
                                        let inner_block = func.block().expect("FunctionDefinition must have a block");
                                        stack.push(Frame {
                                            block: inner_block,
                                            next_stmt: 0,
                                            scope_idx: new_scope_idx,
                                            func_id: Some(func_idx),
                                        });
                                    } else {
                                        let type_source = if let Some(expr) = expression {
                                            Some(self.lower_expression(expr, scope_idx))
                                        } else if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                            if index >= expressions.len() {
                                                let ret_index = index - (expressions.len() - 1);
                                                Some(self.lower_function_call(call, scope_idx, ret_index))
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };
                                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        if let Some(expr_id) = type_source {
                                            self.set_type_source(symbol_idx, expr_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Statement::FunctionCall(_) => {},
            }
        }
    }

    fn lower_expression(&mut self, expression: &Expression, scope_idx: ScopeIndex) -> ExprId {
        match expression {
            Expression::Literal(l) => {
                let vt = if l.get_string().is_some() {
                    ValueType::String
                } else if let Some(bool_value) = l.get_bool() {
                    ValueType::Boolean(Some(bool_value))
                } else if l.get_number().is_some() {
                    ValueType::Number
                } else if l.is_nil() {
                    ValueType::Nil
                } else {
                    return self.push_expr(Expr::Unknown);
                };
                self.push_expr(Expr::Literal(vt))
            }
            Expression::Identifier(ident) => {
                let names = ident.names();
                if let Some(name) = names.first() {
                    let base = if let Some(symbol_idx) = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx) {
                        let version_idx = self.sym(symbol_idx).versions.len() - 1;
                        self.push_expr(Expr::SymbolRef(symbol_idx, version_idx))
                    } else {
                        self.push_expr(Expr::Unknown)
                    };
                    // Chain field accesses for dotted names (t.x.y)
                    let mut current = base;
                    for field_name in names.iter().skip(1) {
                        current = self.push_expr(Expr::FieldAccess {
                            table: current,
                            field: field_name.clone(),
                        });
                    }
                    current
                } else {
                    self.push_expr(Expr::Unknown)
                }
            }
            Expression::BinaryExpression(b) => {
                let terms = b.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let lhs_id = self.lower_expression(lhs, scope_idx);
                    let rhs_id = self.lower_expression(rhs, scope_idx);
                    let op = b.kind();
                    self.push_expr(Expr::BinaryOp { op, lhs: lhs_id, rhs: rhs_id })
                } else {
                    self.push_expr(Expr::Unknown)
                }
            }
            Expression::UnaryExpression(u) => {
                let terms = u.get_terms();
                if let Some(operand) = terms.first() {
                    let operand_id = self.lower_expression(operand, scope_idx);
                    let op = u.kind();
                    self.push_expr(Expr::UnaryOp { op, operand: operand_id })
                } else {
                    self.push_expr(Expr::Unknown)
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    let inner_id = self.lower_expression(&inner, scope_idx);
                    self.push_expr(Expr::Grouped(inner_id))
                } else {
                    self.push_expr(Expr::Unknown)
                }
            }
            Expression::FunctionCall(call) => {
                self.lower_function_call(call, scope_idx, 0)
            }
            Expression::Function(_func) => {
                // Inline function expressions that aren't handled at the statement
                // level (e.g. passed as arguments). We don't track their scope here yet.
                self.push_expr(Expr::Unknown)
            }
            Expression::TableConstructor(tc) => {
                let mut fields = HashMap::new();
                for field in tc.fields() {
                    if let Some(FieldKind::Named { name, value }) = field.kind() {
                        let expr_id = self.lower_expression(&value, scope_idx);
                        fields.insert(name, expr_id);
                    }
                }
                let table_idx = self.tables.len();
                self.tables.push(TableInfo { fields, class_name: None });
                self.push_expr(Expr::TableConstructor(table_idx))
            }
        }
    }

    fn lower_function_call(&mut self, call: &FunctionCall, scope_idx: ScopeIndex, ret_index: usize) -> ExprId {
        let func_id = if let Some(ident) = call.identifier() {
            self.lower_expression(&Expression::Identifier(ident), scope_idx)
        } else {
            self.push_expr(Expr::Unknown)
        };
        let args: Vec<ExprId> = call.arguments()
            .map(|arg_list| arg_list.expressions().iter()
                .map(|expr| self.lower_expression(expr, scope_idx))
                .collect())
            .unwrap_or_default();
        self.push_expr(Expr::FunctionCall { func: func_id, args, ret_index })
    }

    fn insert_function_definition(&mut self, func: &FunctionDefinition, scope_idx: ScopeIndex, inject_self: bool) -> ScopeIndex {
        let node = SyntaxNodePtr::new(func.syntax());
        let param_names = func
            .params()
            .expect("FunctionDefinition should have params")
            .parameters();
        let new_scope_idx = self.insert_scope(Some(scope_idx));
        let mut function = Function {
            def_node: node,
            scope: new_scope_idx,
            args: Vec::new(),
            rets: Vec::new(),
            return_annotations: Vec::new(),
            overloads: Vec::new(),
            doc: None,
        };
        if inject_self {
            function.args.push(self.insert_symbol(SymbolIdentifier::Name("self".to_string()), new_scope_idx, node));
        }
        for name in param_names.iter() {
            // Store args as Name so they're findable by normal scope lookup
            function.args.push(self.insert_symbol(SymbolIdentifier::Name(name.clone()), new_scope_idx, node));
        }
        self.functions.push(function);
        new_scope_idx
    }

    fn insert_scope(&mut self, parent: Option<ScopeIndex>) -> ScopeIndex {
        self.scopes.push(Scope {
            parent,
            symbols: HashMap::new(),
        });
        self.scopes.len() - 1
    }

    fn insert_symbol(&mut self, id: SymbolIdentifier, scope_idx: ScopeIndex, node: SyntaxNodePtr) -> SymbolIndex {
        let version = SymbolVersion {
            def_node: node,
            type_source: None,
            resolved_type: None,
        };
        // Only add a version to existing LOCAL symbols; external ones get shadowed
        if let Some(existing_symbol) = self.get_symbol(&id, scope_idx) {
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

    fn set_type_source(&mut self, symbol_idx: SymbolIndex, expr_id: ExprId) {
        let symbol = &mut self.symbols[symbol_idx];
        let version = symbol.versions.last_mut().expect("symbol must have at least one version");
        version.type_source = Some(expr_id);
    }

    fn push_expr(&mut self, expr: Expr) -> ExprId {
        self.exprs.push(expr);
        self.exprs.len() - 1
    }

    fn find_table_for_symbol(&self, root_name: &str, scope_idx: ScopeIndex) -> Option<TableIndex> {
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name.to_string()), scope_idx)?;
        let ver_idx = self.sym(symbol_idx).versions.len() - 1;
        let type_source = self.sym(symbol_idx).versions[ver_idx].type_source?;
        self.find_table_index(type_source)
    }

    fn find_table_index(&self, expr_id: ExprId) -> Option<TableIndex> {
        match self.expr(expr_id) {
            Expr::TableConstructor(idx) => Some(*idx),
            Expr::Literal(ValueType::Table(Some(idx))) => Some(*idx),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let sym_idx = *sym_idx;
                let ver_idx = *ver_idx;
                let type_source = self.sym(sym_idx).versions[ver_idx].type_source?;
                self.find_table_index(type_source)
            }
            Expr::Grouped(inner) => self.find_table_index(*inner),
            _ => None,
        }
    }

    fn apply_annotations(&mut self, func_idx: FunctionIndex, scope_idx: ScopeIndex, node: &SyntaxNode) {
        let annotations = extract_annotations(node);

        // Apply @param annotations to matching function arguments
        for (param_name, annotation_type) in &annotations.params {
            if let Some(vt) = self.resolve_annotation_type(annotation_type) {
                let func = &self.functions[func_idx];
                for &arg_sym_idx in &func.args {
                    if self.symbols[arg_sym_idx].id == SymbolIdentifier::Name(param_name.clone()) {
                        let expr_id = self.push_expr(Expr::Literal(vt.clone()));
                        self.set_type_source(arg_sym_idx, expr_id);
                        break;
                    }
                }
            }
        }

        // Apply @return annotations
        if !annotations.returns.is_empty() {
            let node_ptr = SyntaxNodePtr::new(node);
            let func_scope = self.functions[func_idx].scope;
            let mut return_vts = Vec::new();
            for (i, ret_annotation) in annotations.returns.iter().enumerate() {
                if let Some(vt) = self.resolve_annotation_type(ret_annotation) {
                    let ret_expr = self.push_expr(Expr::Literal(vt.clone()));
                    let ret_sym_idx = self.insert_symbol(
                        SymbolIdentifier::FunctionRet(func_idx, i),
                        func_scope,
                        node_ptr,
                    );
                    self.set_type_source(ret_sym_idx, ret_expr);
                    self.functions[func_idx].rets.push(ret_sym_idx);
                    return_vts.push(vt);
                }
            }
            self.functions[func_idx].return_annotations = return_vts;
        }

        // Apply @overload annotations
        if !annotations.overloads.is_empty() {
            let overloads: Vec<ResolvedOverload> = annotations.overloads.iter()
                .filter_map(|s| crate::annotations::parse_overload(s))
                .map(|sig| {
                    let params = sig.params.iter().map(|(name, at)| {
                        (name.clone(), self.resolve_annotation_type(at))
                    }).collect();
                    let returns = sig.returns.iter()
                        .filter_map(|at| self.resolve_annotation_type(at))
                        .collect();
                    ResolvedOverload { params, returns }
                })
                .collect();
            self.functions[func_idx].overloads = overloads;
        }

        if annotations.doc.is_some() {
            self.functions[func_idx].doc = annotations.doc;
        }
    }

    fn get_symbol(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex) -> Option<SymbolIndex> {
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
            }
            scope_idx = scope_obj.parent;
        }
        None
    }
}

// ── Type Resolution (Phase 2) ──────────────────────────────────────────────────

impl Variables {
    pub fn resolve_types(&mut self) {
        // Pre-resolve annotated return symbols so they're available before
        // the main resolution loop tries to resolve callers
        for func_idx in 0..self.functions.len() {
            let func = &self.functions[func_idx];
            if func.return_annotations.is_empty() {
                continue;
            }
            let scope = func.scope;
            for (i, vt) in func.return_annotations.clone().iter().enumerate() {
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, i);
                if let Some(ret_sym_idx) = self.get_symbol(&ret_id, scope) {
                    if let Some(ver) = self.symbols[ret_sym_idx].versions.first_mut() {
                        if ver.resolved_type.is_none() {
                            ver.resolved_type = Some(SymbolType::Value(vt.clone()));
                        }
                    }
                }
            }
        }

        let mut pending: Vec<(SymbolIndex, usize)> = Vec::new();
        for (si, sym) in self.symbols.iter().enumerate() {
            for (vi, ver) in sym.versions.iter().enumerate() {
                if ver.type_source.is_some() && ver.resolved_type.is_none() {
                    pending.push((si, vi));
                }
            }
        }
        loop {
            let prev_len = pending.len();
            pending.retain(|&(si, vi)| {
                let expr_id = self.symbols[si].versions[vi].type_source.unwrap();
                if let Some(resolved) = self.resolve_expr(expr_id) {
                    self.symbols[si].versions[vi].resolved_type = Some(resolved);
                    false
                } else {
                    true
                }
            });
            if pending.len() == prev_len {
                break;
            }
        }
    }

    fn resolve_expr(&mut self, expr_id: ExprId) -> Option<SymbolType> {
        let expr = self.expr(expr_id).clone();
        match &expr {
            Expr::Literal(vt) => Some(SymbolType::Value(vt.clone())),

            Expr::SymbolRef(sym_idx, ver_idx) => {
                self.sym(*sym_idx).versions[*ver_idx].resolved_type.clone()
            }

            Expr::BinaryOp { op, lhs, rhs } => {
                let lhs_type = self.resolve_expr(*lhs)?;
                let rhs_type = self.resolve_expr(*rhs)?;
                self.resolve_binary_op(*op, lhs_type, rhs_type)
            }

            Expr::UnaryOp { op, operand } => {
                let operand_type = self.resolve_expr(*operand)?;
                let SymbolType::Value(ref vt) = operand_type else { return None };
                match op {
                    Operator::Not => Some(SymbolType::Value(ValueType::Boolean(None))),
                    Operator::Subtract => {
                        match vt {
                            ValueType::Number => Some(SymbolType::Value(ValueType::Number)),
                            _ => None,
                        }
                    }
                    Operator::ArrayLength => Some(SymbolType::Value(ValueType::Number)),
                    _ => None,
                }
            }

            Expr::Grouped(inner) => self.resolve_expr(*inner),

            Expr::FunctionCall { func, args, ret_index } => {
                // Resolve the function expression to get its type
                let func_type = self.resolve_expr(*func)?;
                let SymbolType::Value(ValueType::Function(Some(func_idx))) = func_type else { return None };
                let func_info = self.func(func_idx).clone();

                // Propagate call-site arg types to parameter symbols (local only)
                for (i, arg_expr_id) in args.iter().enumerate() {
                    if let Some(&param_sym_idx) = func_info.args.get(i) {
                        if param_sym_idx >= EXT_BASE { continue; }
                        if let Some(ver) = self.symbols[param_sym_idx].versions.first() {
                            if ver.resolved_type.is_none() {
                                if let Some(arg_type) = self.resolve_expr(*arg_expr_id) {
                                    self.symbols[param_sym_idx].versions[0].resolved_type = Some(arg_type);
                                }
                            }
                        }
                    }
                }

                // Try overload resolution: pick the overload whose param count
                // best matches the call-site arg count, and use its return type.
                if !func_info.overloads.is_empty() {
                    let arg_count = args.len();
                    let best = func_info.overloads.iter()
                        .filter(|o| o.params.len() == arg_count)
                        .next()
                        .or_else(|| {
                            // Fallback: closest param count that can accept our args
                            func_info.overloads.iter()
                                .filter(|o| o.params.len() >= arg_count)
                                .min_by_key(|o| o.params.len())
                        });
                    if let Some(overload) = best {
                        if let Some(ret_vt) = overload.returns.get(*ret_index) {
                            return Some(SymbolType::Value(ret_vt.clone()));
                        }
                        if !overload.returns.is_empty() {
                            return Some(SymbolType::Value(overload.returns[0].clone()));
                        }
                    }
                }

                // Default: use the primary signature's return type
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, *ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_info.scope)?;
                self.sym(ret_sym_idx).versions.first()?.resolved_type.clone()
            }

            Expr::FunctionDef(func_idx) => {
                Some(SymbolType::Value(ValueType::Function(Some(*func_idx))))
            }

            Expr::TableConstructor(table_idx) => {
                Some(SymbolType::Value(ValueType::Table(Some(*table_idx))))
            }

            Expr::FieldAccess { table, field } => {
                let table_type = self.resolve_expr(*table)?;
                let SymbolType::Value(ValueType::Table(Some(table_idx))) = table_type else {
                    return None;
                };
                let field_expr_id = *self.table(table_idx).fields.get(field)?;
                self.resolve_expr(field_expr_id)
            }

            Expr::Unknown => None,
        }
    }

    fn resolve_binary_op(&mut self, op: Operator, lhs_type: SymbolType, rhs_type: SymbolType) -> Option<SymbolType> {
        let SymbolType::Value(ref lhs_vt) = lhs_type else { return None };
        let SymbolType::Value(ref rhs_vt) = rhs_type else { return None };
        match op {
            Operator::Or => {
                match (lhs_vt, rhs_vt) {
                    (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(Some(true)), _) => {
                        Some(lhs_type)
                    },
                    (ValueType::Boolean(None), ValueType::Boolean(_)) => Some(lhs_type),
                    (ValueType::Boolean(None), _) => {
                        Some(SymbolType::Value(ValueType::union(
                            ValueType::Boolean(None),
                            rhs_vt.clone(),
                        )))
                    },
                    (ValueType::Number | ValueType::String | ValueType::Function(_) | ValueType::Table(_) | ValueType::Union(_), _) => {
                        Some(lhs_type)
                    },
                }
            },
            Operator::And => {
                match (lhs_vt, rhs_vt) {
                    (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => {
                        Some(lhs_type)
                    },
                    (ValueType::Boolean(Some(true)) | ValueType::Number | ValueType::String | ValueType::Function(_) | ValueType::Table(_) | ValueType::Union(_), _) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(None), ValueType::Boolean(Some(true))) => {
                        Some(lhs_type)
                    },
                    (_, ValueType::Boolean(Some(false)) | ValueType::Nil) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(None), _) => {
                        Some(SymbolType::Value(ValueType::union(
                            ValueType::Boolean(None),
                            rhs_vt.clone(),
                        )))
                    },
                }
            },
            Operator::LessThan | Operator::GreaterThan | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals => {
                Some(SymbolType::Value(ValueType::Boolean(None)))
            },
            Operator::NotEquals | Operator::Equals => {
                Some(SymbolType::Value(ValueType::Boolean(None)))
            },
            Operator::Concatenate => {
                if lhs_vt.can_concat_to_string() && rhs_vt.can_concat_to_string() {
                    Some(SymbolType::Value(ValueType::String))
                } else {
                    None
                }
            },
            Operator::Add | Operator::Subtract | Operator::Divide | Operator::Multiply | Operator::Modulo | Operator::Hat => {
                match (lhs_vt, rhs_vt) {
                    (ValueType::Number, ValueType::Number) => Some(SymbolType::Value(ValueType::Number)),
                    (ValueType::Table(_), _) | (_, ValueType::Table(_)) => None, // TODO: metamethods
                    _ => None,
                }
            },
            _ => None,
        }
    }
}

// ── LSP Queries ──────────────────────────────────────────────────────────────

impl Variables {
    fn find_symbol_at(&self, offset: u32) -> Option<(SymbolIndex, String)> {
        let text_size = rowan::TextSize::from(offset);
        let token = self.root.token_at_offset(text_size).right_biased()?;
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        let name = token.text().to_string();
        let scope_idx = self.scope_at_offset(text_size)?;
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx)?;
        Some((symbol_idx, name))
    }

    pub fn definition_at(&self, offset: u32) -> Option<rowan::TextRange> {
        let (symbol_idx, _) = self.find_symbol_at(offset)?;
        let symbol = self.sym(symbol_idx);
        let version = symbol.versions.first()?;
        Some(version.def_node.text_range())
    }

    pub fn hover_at(&self, offset: u32) -> Option<HoverResult> {
        if let Some((symbol_idx, name)) = self.find_symbol_at(offset) {
            let symbol = self.sym(symbol_idx);
            let resolved = symbol.versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            let type_str = format!("{}: {}", name, self.format_symbol_type(resolved));
            let doc = self.doc_for_type(resolved);
            return Some(HoverResult { type_str, doc });
        }
        // Try field access (e.g. hovering over "new" in shash.new)
        let (field_name, expr_id) = self.find_field_at(offset)?;
        let resolved = self.resolve_expr_type(expr_id)?;
        let type_str = format!("{}: {}", field_name, self.format_symbol_type(&resolved));
        let doc = self.doc_for_type(&resolved);
        Some(HoverResult { type_str, doc })
    }

    fn doc_for_type(&self, st: &SymbolType) -> Option<String> {
        match st {
            SymbolType::Value(ValueType::Function(Some(func_idx))) => {
                self.func(*func_idx).doc.clone()
            }
            _ => None,
        }
    }

    pub fn completions_at(&self, offset: u32, source: &str) -> Option<Vec<lsp_types::CompletionItem>> {
        use lsp_types::{CompletionItem, CompletionItemKind};

        if offset == 0 {
            return None;
        }

        let prev_char = source.as_bytes().get((offset - 1) as usize).copied()?;

        if prev_char == b'.' || prev_char == b':' {
            // Dot/colon completion: resolve the prefix to a table, enumerate fields
            let prefix_offset = offset - 2;
            let text_size = rowan::TextSize::from(prefix_offset);
            let token = self.root.token_at_offset(text_size).right_biased()?;
            if token.kind() != SyntaxKind::Name {
                return None;
            }

            // Find the Identifier parent and resolve the full chain
            let table_idx = if let Some(parent) = token.parent() {
                if parent.kind() == SyntaxKind::Identifier {
                    let names: Vec<_> = parent.children_with_tokens()
                        .filter_map(|it| it.into_token())
                        .filter(|t| t.kind() == SyntaxKind::Name)
                        .collect();
                    let our_index = names.iter().position(|n| n.text_range() == token.text_range())?;
                    let root_name = names[0].text().to_string();
                    let scope_idx = self.scope_at_offset(text_size)?;
                    let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
                    let ver = self.sym(symbol_idx).versions.last()?;
                    let resolved = ver.resolved_type.as_ref()?;
                    let SymbolType::Value(ValueType::Table(Some(start_idx))) = resolved else {
                        return None;
                    };
                    let mut idx = *start_idx;
                    // Walk intermediate fields
                    for i in 1..=our_index {
                        if i > our_index { break; }
                        if i <= our_index && i < names.len() {
                            let name = names[i].text().to_string();
                            let field_expr_id = *self.table(idx).fields.get(&name)?;
                            let field_type = self.resolve_expr_type(field_expr_id)?;
                            let SymbolType::Value(ValueType::Table(Some(next_idx))) = field_type else {
                                return None;
                            };
                            idx = next_idx;
                        }
                    }
                    Some(idx)
                } else {
                    // Single name, not in an Identifier chain
                    let name = token.text().to_string();
                    let scope_idx = self.scope_at_offset(text_size)?;
                    let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(name), scope_idx)?;
                    let ver = self.sym(symbol_idx).versions.last()?;
                    let resolved = ver.resolved_type.as_ref()?;
                    let SymbolType::Value(ValueType::Table(Some(idx))) = resolved else {
                        return None;
                    };
                    Some(*idx)
                }
            } else {
                return None;
            };

            let table_idx = table_idx?;
            let table = self.table(table_idx);
            let is_colon = prev_char == b':';
            let mut items: Vec<CompletionItem> = table.fields.iter()
                .filter_map(|(name, expr_id)| {
                    let resolved = self.resolve_expr_type(*expr_id);
                    let (detail, kind) = match &resolved {
                        Some(SymbolType::Value(ValueType::Function(_))) => {
                            (Some(self.format_symbol_type(resolved.as_ref().unwrap())),
                             CompletionItemKind::METHOD)
                        }
                        Some(st) => {
                            if is_colon {
                                return None; // colon completions only show methods
                            }
                            (Some(self.format_symbol_type(st)), CompletionItemKind::FIELD)
                        }
                        None => {
                            if is_colon { return None; }
                            (None, CompletionItemKind::FIELD)
                        }
                    };
                    Some(CompletionItem {
                        label: name.clone(),
                        kind: Some(kind),
                        detail,
                        ..CompletionItem::default()
                    })
                })
                .collect();
            items.sort_by(|a, b| a.label.cmp(&b.label));
            Some(items)
        } else {
            // Scope completion: enumerate all visible symbols
            let text_size = rowan::TextSize::from(offset);
            let scope_idx = self.scope_at_offset(text_size)?;

            let mut seen = std::collections::HashSet::new();
            let mut items = Vec::new();
            let mut current_scope = Some(scope_idx);
            while let Some(si) = current_scope {
                let scope = &self.scopes[si];
                for (id, &sym_idx) in &scope.symbols {
                    if let SymbolIdentifier::Name(name) = id {
                        if seen.insert(name.clone()) {
                            let resolved = self.sym(sym_idx).versions.iter().rev()
                                .find_map(|v| v.resolved_type.as_ref());
                            let (detail, kind) = match resolved {
                                Some(SymbolType::Value(ValueType::Function(_))) => {
                                    (Some(self.format_symbol_type(resolved.unwrap())),
                                     CompletionItemKind::FUNCTION)
                                }
                                Some(SymbolType::Value(ValueType::Table(Some(idx)))) => {
                                    let k = if self.table(*idx).class_name.is_some() {
                                        CompletionItemKind::CLASS
                                    } else {
                                        CompletionItemKind::VARIABLE
                                    };
                                    (Some(self.format_symbol_type(resolved.unwrap())), k)
                                }
                                Some(st) => {
                                    (Some(self.format_symbol_type(st)), CompletionItemKind::VARIABLE)
                                }
                                None => (None, CompletionItemKind::VARIABLE),
                            };
                            items.push(CompletionItem {
                                label: name.clone(),
                                kind: Some(kind),
                                detail,
                                ..CompletionItem::default()
                            });
                        }
                    }
                }
                current_scope = scope.parent;
            }
            items.sort_by(|a, b| a.label.cmp(&b.label));
            if items.is_empty() { None } else { Some(items) }
        }
    }

    fn find_field_at(&self, offset: u32) -> Option<(String, ExprId)> {
        let text_size = rowan::TextSize::from(offset);
        let token = self.root.token_at_offset(text_size).right_biased()?;
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        let parent = token.parent()?;
        if parent.kind() != SyntaxKind::Identifier {
            return None;
        }
        // Collect all Name tokens in the Identifier
        let names: Vec<_> = parent.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();
        if names.len() < 2 {
            return None;
        }
        let our_index = names.iter().position(|n| n.text_range() == token.text_range())?;
        if our_index == 0 {
            return None; // Root name is a symbol, handled by find_symbol_at
        }

        // Resolve chain: root symbol → table → field
        let root_name = names[0].text().to_string();
        let scope_idx = self.scope_at_offset(text_size)?;
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
        let ver = self.sym(symbol_idx).versions.last()?;
        let resolved = ver.resolved_type.as_ref()?;
        let SymbolType::Value(ValueType::Table(Some(start_table_idx))) = resolved else {
            return None;
        };
        let mut table_idx = *start_table_idx;

        // Walk intermediate fields
        for i in 1..our_index {
            let name = names[i].text().to_string();
            let field_expr_id = *self.table(table_idx).fields.get(&name)?;
            let field_type = self.resolve_expr_type(field_expr_id)?;
            let SymbolType::Value(ValueType::Table(Some(next_idx))) = field_type else {
                return None;
            };
            table_idx = next_idx;
        }

        let field_name = names[our_index].text().to_string();
        let field_expr_id = *self.table(table_idx).fields.get(&field_name)?;
        Some((field_name, field_expr_id))
    }

    fn resolve_expr_type(&self, expr_id: ExprId) -> Option<SymbolType> {
        match self.expr(expr_id) {
            Expr::Literal(vt) => Some(SymbolType::Value(vt.clone())),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                self.sym(*sym_idx).versions[*ver_idx].resolved_type.clone()
            }
            Expr::FunctionDef(func_idx) => {
                Some(SymbolType::Value(ValueType::Function(Some(*func_idx))))
            }
            Expr::TableConstructor(table_idx) => {
                Some(SymbolType::Value(ValueType::Table(Some(*table_idx))))
            }
            Expr::Grouped(inner) => self.resolve_expr_type(*inner),
            Expr::FieldAccess { table, field } => {
                let table = *table;
                let field = field.clone();
                let table_type = self.resolve_expr_type(table)?;
                let SymbolType::Value(ValueType::Table(Some(idx))) = table_type else {
                    return None;
                };
                let field_expr_id = *self.table(idx).fields.get(&field)?;
                self.resolve_expr_type(field_expr_id)
            }
            Expr::FunctionCall { func, ret_index, .. } => {
                let func = *func;
                let ret_index = *ret_index;
                let func_type = self.resolve_expr_type(func)?;
                let SymbolType::Value(ValueType::Function(Some(func_idx))) = func_type else { return None };
                let func_info = self.func(func_idx);
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_info.scope)?;
                self.sym(ret_sym_idx).versions.first()?.resolved_type.clone()
            }
            _ => None,
        }
    }

    fn format_symbol_type(&self, st: &SymbolType) -> String {
        self.format_symbol_type_depth(st, 0)
    }

    fn format_symbol_type_depth(&self, st: &SymbolType, depth: usize) -> String {
        match st {
            SymbolType::Unknown => "unknown".to_string(),
            SymbolType::Value(vt) => self.format_value_type_depth(vt, depth),
        }
    }

    fn format_value_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        match vt {
            ValueType::Nil => "nil".to_string(),
            ValueType::Boolean(Some(true)) => "true".to_string(),
            ValueType::Boolean(Some(false)) => "false".to_string(),
            ValueType::Boolean(None) => "boolean".to_string(),
            ValueType::Number => "number".to_string(),
            ValueType::String => "string".to_string(),
            ValueType::Function(Some(func_idx)) => {
                let func = self.func(*func_idx);
                let args: Vec<String> = func.args.iter().map(|&sym_idx| {
                    let name = match &self.sym(sym_idx).id {
                        SymbolIdentifier::Name(n) => n.clone(),
                        _ => "?".to_string(),
                    };
                    let type_str = self.sym(sym_idx).versions.iter()
                        .find_map(|v| v.resolved_type.as_ref())
                        .map(|rt| self.format_symbol_type_depth(rt, depth + 1));
                    match type_str {
                        Some(t) => format!("{}: {}", name, t),
                        None => name,
                    }
                }).collect();
                let rets: Vec<String> = func.rets.iter().map(|&sym_idx| {
                    match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                        Some(rt) => self.format_symbol_type_depth(rt, depth + 1),
                        None => "?".to_string(),
                    }
                }).collect();
                let primary = if rets.is_empty() {
                    format!("fun({})", args.join(", "))
                } else {
                    format!("fun({}): {}", args.join(", "), rets.join(", "))
                };
                if func.overloads.is_empty() || depth > 0 {
                    primary
                } else {
                    let mut lines = vec![primary];
                    for overload in &func.overloads {
                        lines.push(self.format_overload(overload));
                    }
                    lines.join("\n")
                }
            }
            ValueType::Function(None) => "function".to_string(),
            ValueType::Table(Some(table_idx)) => {
                let table = self.table(*table_idx);
                if let Some(ref class_name) = table.class_name {
                    if table.fields.is_empty() || depth > 0 {
                        return class_name.clone();
                    }
                    let indent = "  ".repeat(depth + 1);
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, expr_id)| {
                        let type_str = self.resolve_expr_type(*expr_id)
                            .map(|t| self.format_symbol_type_depth(&t, depth + 1))
                            .unwrap_or_else(|| "?".to_string());
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
                    fields.sort();
                    return format!("{} {{\n{}\n}}", class_name, fields.join(",\n"));
                }
                if table.fields.is_empty() || depth > 0 {
                    "table".to_string()
                } else {
                    let indent = "  ".repeat(depth + 1);
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, expr_id)| {
                        let type_str = self.resolve_expr_type(*expr_id)
                            .map(|t| self.format_symbol_type_depth(&t, depth + 1))
                            .unwrap_or_else(|| "?".to_string());
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
                    fields.sort();
                    format!("{{\n{}\n}}", fields.join(",\n"))
                }
            }
            ValueType::Table(None) => "table".to_string(),
            ValueType::Union(types) => {
                let parts: Vec<String> = types.iter().map(|t| self.format_value_type_depth(t, depth)).collect();
                parts.join(" | ")
            }
        }
    }

    fn scope_at_offset(&self, offset: rowan::TextSize) -> Option<ScopeIndex> {
        let mut best: Option<(rowan::TextRange, ScopeIndex)> = None;
        for &(range, scope_idx) in &self.block_scopes {
            if range.contains(offset) {
                match best {
                    None => best = Some((range, scope_idx)),
                    Some((best_range, _)) if range.len() < best_range.len() => {
                        best = Some((range, scope_idx));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, idx)| idx)
    }

    pub fn signature_help_at(&self, offset: u32) -> Option<SignatureHelpResult> {
        let text_size = rowan::TextSize::from(offset);
        let token = self.root.token_at_offset(text_size).left_biased()?;

        // Walk up to find the enclosing FunctionCall node
        let call_node = token.parent_ancestors()
            .find(|n| n.kind() == SyntaxKind::FunctionCall)?;
        let call = FunctionCall::cast(call_node.clone())?;

        // Only trigger if cursor is within the argument list (at or after the open paren)
        let arg_list = call_node.children()
            .find(|n| n.kind() == SyntaxKind::ArgumentList)?;
        if text_size < arg_list.text_range().start() {
            return None;
        }
        let active_parameter = {
            let mut commas = 0u32;
            for child in arg_list.children_with_tokens() {
                if child.text_range().start() >= text_size {
                    break;
                }
                if child.kind() == SyntaxKind::Comma {
                    commas += 1;
                }
            }
            commas
        };

        // Resolve the function being called
        let ident = call.identifier()?;
        let names = ident.names();
        if names.is_empty() {
            return None;
        }

        let scope_idx = self.scope_at_offset(text_size)?;
        let func_idx = if names.len() == 1 {
            // Simple function call: foo()
            let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
            let ver = self.sym(symbol_idx).versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            match ver {
                SymbolType::Value(ValueType::Function(Some(idx))) => *idx,
                _ => return None,
            }
        } else {
            // Method/field call: obj.method() or obj:method()
            let root_sym = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
            let ver = self.sym(root_sym).versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            let SymbolType::Value(ValueType::Table(Some(start_idx))) = ver else {
                return None;
            };
            let mut table_idx = *start_idx;
            // Walk intermediate names
            for name in &names[1..names.len()-1] {
                let field_expr = *self.table(table_idx).fields.get(name)?;
                let ft = self.resolve_expr_type(field_expr)?;
                let SymbolType::Value(ValueType::Table(Some(next))) = ft else {
                    return None;
                };
                table_idx = next;
            }
            let method_name = &names[names.len() - 1];
            let field_expr = *self.table(table_idx).fields.get(method_name)?;
            let ft = self.resolve_expr_type(field_expr)?;
            match ft {
                SymbolType::Value(ValueType::Function(Some(idx))) => idx,
                _ => return None,
            }
        };

        let func = self.func(func_idx);
        let is_colon = ident.is_call_to_self();

        // Build signatures: primary + overloads
        let mut signatures = Vec::new();

        // Primary signature
        let primary = self.build_signature_info(func, is_colon);
        signatures.push(primary);

        // Overload signatures
        for overload in &func.overloads {
            signatures.push(self.build_overload_signature_info(overload));
        }

        let active_signature = Some(0);

        Some(SignatureHelpResult {
            signatures,
            active_signature,
            active_parameter,
        })
    }

    fn build_signature_info(&self, func: &Function, skip_self: bool) -> SignatureInfo {
        let args: Vec<(String, Option<String>)> = func.args.iter()
            .map(|&sym_idx| {
                let name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let type_str = self.sym(sym_idx).versions.iter()
                    .find_map(|v| v.resolved_type.as_ref())
                    .map(|rt| self.format_symbol_type_depth(rt, 1));
                (name, type_str)
            })
            .filter(|(name, _)| !(skip_self && name == "self"))
            .collect();

        let rets: Vec<String> = func.rets.iter().map(|&sym_idx| {
            match self.sym(sym_idx).versions.first().and_then(|v| v.resolved_type.as_ref()) {
                Some(rt) => self.format_symbol_type_depth(rt, 1),
                None => "?".to_string(),
            }
        }).collect();

        let params: Vec<String> = args.iter().map(|(name, type_str)| {
            match type_str {
                Some(t) => format!("{}: {}", name, t),
                None => name.clone(),
            }
        }).collect();

        let label = if rets.is_empty() {
            format!("fun({})", params.join(", "))
        } else {
            format!("fun({}): {}", params.join(", "), rets.join(", "))
        };

        SignatureInfo { label, params, doc: func.doc.clone() }
    }

    fn build_overload_signature_info(&self, overload: &ResolvedOverload) -> SignatureInfo {
        let params: Vec<String> = overload.params.iter().map(|(name, vt)| {
            match vt {
                Some(vt) => format!("{}: {}", name, self.format_value_type_depth(vt, 1)),
                None => name.clone(),
            }
        }).collect();

        let rets: Vec<String> = overload.returns.iter()
            .map(|vt| self.format_value_type_depth(vt, 1))
            .collect();

        let label = if rets.is_empty() {
            format!("fun({})", params.join(", "))
        } else {
            format!("fun({}): {}", params.join(", "), rets.join(", "))
        };

        SignatureInfo { label, params, doc: None }
    }

    fn format_overload(&self, overload: &ResolvedOverload) -> String {
        let args: Vec<String> = overload.params.iter().map(|(name, vt)| {
            match vt {
                Some(vt) => format!("{}: {}", name, self.format_value_type_depth(vt, 1)),
                None => name.clone(),
            }
        }).collect();
        let rets: Vec<String> = overload.returns.iter()
            .map(|vt| self.format_value_type_depth(vt, 1))
            .collect();
        if rets.is_empty() {
            format!("fun({})", args.join(", "))
        } else {
            format!("fun({}): {}", args.join(", "), rets.join(", "))
        }
    }
}
