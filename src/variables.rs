use std::collections::HashMap;

use rowan::GreenNode;
use crate::ast::*;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr};

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum SymbolType {
    Unknown,
    Value(ValueType),
}

#[derive(Debug, Clone, PartialEq)]
enum ValueType {
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

    fn union(a: ValueType, b: ValueType) -> ValueType {
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

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum SymbolIdentifier {
    Name(String),
    FunctionRet(FunctionIndex, usize),
}

#[derive(Debug)]
struct Scope {
    parent: Option<ScopeIndex>,
    symbols: HashMap<SymbolIdentifier, SymbolIndex>,
}

#[derive(Debug)]
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

#[derive(Debug, Clone, PartialEq)]
struct Function {
    def_node: SyntaxNodePtr,
    scope: ScopeIndex,
    args: Vec<SymbolIndex>,
    rets: Vec<SymbolIndex>,
}

#[derive(Debug, Clone)]
struct TableInfo {
    fields: HashMap<String, ExprId>,
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
}

impl Variables {
    pub fn new(filename: String, green: GreenNode) -> Variables {
        let root = SyntaxNode::new_root(green);
        let mut variables = Variables {
            root,
            scopes: Vec::new(),
            symbols: Vec::new(),
            functions: Vec::new(),
            tables: Vec::new(),
            exprs: Vec::new(),
            block_scopes: Vec::new(),
        };
        variables.build_ir();
        variables
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
            println!("    [{}] fields: {:?}", i, table.fields.keys().collect::<Vec<_>>());
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
                        // Dotted name: function t.method() or function t:method()
                        let names = ident.names();
                        if names.len() >= 2 {
                            let root_name = &names[0];
                            let field_name = &names[names.len() - 1];
                            let is_method = ident.is_call_to_self();

                            let new_scope_idx = self.insert_function_definition(func, scope_idx, is_method);
                            let func_idx = self.functions.len() - 1;
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
                            self.functions.get_mut(func_id).unwrap().rets.push(symbol_idx);
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
                        let version_idx = self.symbols[symbol_idx].versions.len() - 1;
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
                self.tables.push(TableInfo { fields });
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
        if let Some(existing_symbol) = self.get_symbol(&id, scope_idx) {
            self.symbols.get_mut(existing_symbol).unwrap().versions.push(version);
            existing_symbol
        } else {
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
        let ver_idx = self.symbols[symbol_idx].versions.len() - 1;
        let type_source = self.symbols[symbol_idx].versions[ver_idx].type_source?;
        self.find_table_index(type_source)
    }

    fn find_table_index(&self, expr_id: ExprId) -> Option<TableIndex> {
        match &self.exprs[expr_id] {
            Expr::TableConstructor(idx) => Some(*idx),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let type_source = self.symbols[*sym_idx].versions[*ver_idx].type_source?;
                self.find_table_index(type_source)
            }
            Expr::Grouped(inner) => self.find_table_index(*inner),
            _ => None,
        }
    }

    fn get_symbol(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex) -> Option<SymbolIndex> {
        let mut scope_idx = Some(scope_idx);
        while let Some(scope_obj) = self.scopes.get(scope_idx?) {
            if let Some(&sym) = scope_obj.symbols.get(id) {
                return Some(sym);
            }
            scope_idx = scope_obj.parent;
        }
        None
    }
}

// ── Type Resolution (Phase 2) ──────────────────────────────────────────────────

impl Variables {
    pub fn resolve_types(&mut self) {
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
        let expr = self.exprs[expr_id].clone();
        match &expr {
            Expr::Literal(vt) => Some(SymbolType::Value(vt.clone())),

            Expr::SymbolRef(sym_idx, ver_idx) => {
                self.symbols[*sym_idx].versions[*ver_idx].resolved_type.clone()
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
                let func_info = self.functions[func_idx].clone();

                // Propagate call-site arg types to parameter symbols
                for (i, arg_expr_id) in args.iter().enumerate() {
                    if let Some(param_sym_idx) = func_info.args.get(i) {
                        if let Some(ver) = self.symbols[*param_sym_idx].versions.first() {
                            if ver.resolved_type.is_none() {
                                if let Some(arg_type) = self.resolve_expr(*arg_expr_id) {
                                    self.symbols[*param_sym_idx].versions[0].resolved_type = Some(arg_type);
                                }
                            }
                        }
                    }
                }

                let ret_id = SymbolIdentifier::FunctionRet(func_idx, *ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_info.scope)?;
                self.symbols[ret_sym_idx].versions.first()?.resolved_type.clone()
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
                let field_expr_id = *self.tables[table_idx].fields.get(field)?;
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
        let symbol = &self.symbols[symbol_idx];
        let version = symbol.versions.first()?;
        Some(version.def_node.text_range())
    }

    pub fn hover_at(&self, offset: u32) -> Option<String> {
        if let Some((symbol_idx, name)) = self.find_symbol_at(offset) {
            let symbol = &self.symbols[symbol_idx];
            let resolved = symbol.versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            let type_str = self.format_symbol_type(resolved);
            return Some(format!("{}: {}", name, type_str));
        }
        // Try field access (e.g. hovering over "new" in shash.new)
        let (field_name, expr_id) = self.find_field_at(offset)?;
        let resolved = self.resolve_expr_type(expr_id)?;
        let type_str = self.format_symbol_type(&resolved);
        Some(format!("{}: {}", field_name, type_str))
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
        let ver = self.symbols[symbol_idx].versions.last()?;
        let resolved = ver.resolved_type.as_ref()?;
        let SymbolType::Value(ValueType::Table(Some(start_table_idx))) = resolved else {
            return None;
        };
        let mut table_idx = *start_table_idx;

        // Walk intermediate fields
        for i in 1..our_index {
            let name = names[i].text().to_string();
            let field_expr_id = *self.tables[table_idx].fields.get(&name)?;
            let field_type = self.resolve_expr_type(field_expr_id)?;
            let SymbolType::Value(ValueType::Table(Some(next_idx))) = field_type else {
                return None;
            };
            table_idx = next_idx;
        }

        let field_name = names[our_index].text().to_string();
        let field_expr_id = *self.tables[table_idx].fields.get(&field_name)?;
        Some((field_name, field_expr_id))
    }

    fn resolve_expr_type(&self, expr_id: ExprId) -> Option<SymbolType> {
        match &self.exprs[expr_id] {
            Expr::Literal(vt) => Some(SymbolType::Value(vt.clone())),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                self.symbols[*sym_idx].versions[*ver_idx].resolved_type.clone()
            }
            Expr::FunctionDef(func_idx) => {
                Some(SymbolType::Value(ValueType::Function(Some(*func_idx))))
            }
            Expr::TableConstructor(table_idx) => {
                Some(SymbolType::Value(ValueType::Table(Some(*table_idx))))
            }
            Expr::Grouped(inner) => self.resolve_expr_type(*inner),
            Expr::FieldAccess { table, field } => {
                let table_type = self.resolve_expr_type(*table)?;
                let SymbolType::Value(ValueType::Table(Some(idx))) = table_type else {
                    return None;
                };
                let field_expr_id = *self.tables[idx].fields.get(field)?;
                self.resolve_expr_type(field_expr_id)
            }
            Expr::FunctionCall { func, ret_index, .. } => {
                let func_type = self.resolve_expr_type(*func)?;
                let SymbolType::Value(ValueType::Function(Some(func_idx))) = func_type else { return None };
                let func_info = &self.functions[func_idx];
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, *ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_info.scope)?;
                self.symbols[ret_sym_idx].versions.first()?.resolved_type.clone()
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
                let func = &self.functions[*func_idx];
                let args: Vec<String> = func.args.iter().map(|&sym_idx| {
                    match &self.symbols[sym_idx].id {
                        SymbolIdentifier::Name(n) => n.clone(),
                        _ => "?".to_string(),
                    }
                }).collect();
                let rets: Vec<String> = func.rets.iter().map(|&sym_idx| {
                    match self.symbols[sym_idx].versions.first().and_then(|v| v.resolved_type.as_ref()) {
                        Some(rt) => self.format_symbol_type_depth(rt, depth + 1),
                        None => "?".to_string(),
                    }
                }).collect();
                if rets.is_empty() {
                    format!("fun({})", args.join(", "))
                } else {
                    format!("fun({}): {}", args.join(", "), rets.join(", "))
                }
            }
            ValueType::Function(None) => "function".to_string(),
            ValueType::Table(Some(table_idx)) => {
                let table = &self.tables[*table_idx];
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
}
