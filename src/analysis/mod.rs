pub mod prescan;
pub mod build_ir;
pub mod resolve;
pub mod checks;
pub mod queries;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rowan::GreenNode;
use crate::diagnostics::WowDiagnostic;
use crate::syntax::{SyntaxNode, SyntaxNodePtr};
use crate::types::*;
use crate::pre_globals::PreResolvedGlobals;

// ── Main struct ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Analysis {
    pub(crate) root: SyntaxNode,
    pub(crate) scopes: Vec<Scope>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) functions: Vec<Function>,
    pub(crate) tables: Vec<TableInfo>,
    pub(crate) exprs: Vec<Expr>,
    pub(crate) block_scopes: Vec<(rowan::TextRange, ScopeIndex)>,
    pub(crate) classes: HashMap<String, TableIndex>,
    pub(crate) aliases: HashMap<String, ValueType>,
    pub(crate) diagnostics: Vec<WowDiagnostic>,
    pub(crate) call_exprs: Vec<ExprId>,
    pub(crate) string_literals: HashMap<ExprId, String>,
    pub(crate) return_type_checks: Vec<ReturnTypeCheck>,
    pub(crate) field_type_checks: Vec<FieldTypeCheck>,
    pub(crate) referenced_symbols: HashSet<SymbolIndex>,
    pub(crate) unresolved_globals: Vec<UnresolvedGlobal>,
    pub(crate) local_defs: Vec<LocalDef>,
    pub(crate) symbol_type_annotations: HashMap<SymbolIndex, ValueType>,
    pub(crate) assign_type_checks: Vec<AssignTypeCheck>,
    pub(crate) functions_with_returns: HashSet<FunctionIndex>,
    pub(crate) narrowed_symbols: HashMap<ScopeIndex, HashSet<SymbolIndex>>,
    pub(crate) nil_check_sites: Vec<NilCheckSite>,
    pub(crate) field_assignment_sites: Vec<FieldAssignmentSite>,
    // External globals (shared across files, never cloned per-file)
    pub(crate) ext: Arc<PreResolvedGlobals>,
    pub(crate) is_meta: bool,
}

impl Analysis {
    pub fn new(
        green: GreenNode,
        pre_globals: Arc<PreResolvedGlobals>,
    ) -> Analysis {
        let root = SyntaxNode::new_root(green);
        let mut variables = Analysis {
            root,
            scopes: Vec::new(),
            symbols: Vec::new(),
            functions: Vec::new(),
            tables: Vec::new(),
            exprs: Vec::new(),
            block_scopes: Vec::new(),
            classes: HashMap::new(),
            aliases: HashMap::new(),
            diagnostics: Vec::new(),
            call_exprs: Vec::new(),
            string_literals: HashMap::new(),
            return_type_checks: Vec::new(),
            field_type_checks: Vec::new(),
            referenced_symbols: HashSet::new(),
            unresolved_globals: Vec::new(),
            local_defs: Vec::new(),
            symbol_type_annotations: HashMap::new(),
            assign_type_checks: Vec::new(),
            functions_with_returns: HashSet::new(),
            narrowed_symbols: HashMap::new(),
            nil_check_sites: Vec::new(),
            field_assignment_sites: Vec::new(),
            ext: pre_globals,
            is_meta: false,
        };
        variables.prescan_classes_and_aliases();
        variables.build_ir();
        variables.inject_preresolved();
        variables
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

    // ── Core helpers used across multiple phases ────────────────────────────

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
            }
            scope_idx = scope_obj.parent;
        }
        None
    }

    pub(crate) fn push_expr(&mut self, expr: Expr) -> ExprId {
        self.exprs.push(expr);
        self.exprs.len() - 1
    }

    pub(super) fn insert_scope(&mut self, parent: Option<ScopeIndex>) -> ScopeIndex {
        self.scopes.push(Scope {
            parent,
            symbols: HashMap::new(),
        });
        self.scopes.len() - 1
    }

    pub(super) fn insert_symbol(&mut self, id: SymbolIdentifier, scope_idx: ScopeIndex, node: SyntaxNodePtr) -> SymbolIndex {
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

    pub(super) fn set_type_source(&mut self, symbol_idx: SymbolIndex, expr_id: ExprId) {
        let symbol = &mut self.symbols[symbol_idx];
        let version = symbol.versions.last_mut().expect("symbol must have at least one version");
        version.type_source = Some(expr_id);
    }

    pub(super) fn find_table_for_symbol(&self, root_name: &str, scope_idx: ScopeIndex) -> Option<TableIndex> {
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name.to_string()), scope_idx)?;
        let ver_idx = self.sym(symbol_idx).versions.len() - 1;
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
            Expr::Grouped(inner) => self.find_table_index(*inner),
            _ => None,
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
            if si < self.scopes.len() {
                current = self.scopes[si].parent;
            } else {
                break;
            }
        }
        false
    }

    pub(super) fn find_root_symbol(&self, expr_id: ExprId) -> Option<SymbolIndex> {
        match self.expr(expr_id) {
            Expr::SymbolRef(sym_idx, _) => Some(*sym_idx),
            Expr::FieldAccess { table, .. } => self.find_root_symbol(*table),
            Expr::Grouped(inner) => self.find_root_symbol(*inner),
            _ => None,
        }
    }
}
