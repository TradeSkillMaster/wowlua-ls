use std::collections::HashMap;

use rowan::GreenNode;
use crate::ast::*;
use crate::syntax::{SyntaxNode, SyntaxNodePtr};

#[derive(Debug, Clone, PartialEq)]
enum ValueType {
    Nil,
    Boolean(Option<bool>),
    Number,
    String,
    Function(Option<Function>),
    Table,
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
            ValueType::Table => false, // TODO: Support __concat metamethod
        }
    }
}

type ScopeIndex = usize;
type SymbolIndex = usize;
type FunctionIndex = usize;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum SymbolIdentifier {
    Name(String),
    FunctionArg(FunctionIndex, String),
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

#[derive(Debug, Clone, PartialEq)]
struct SymbolVersion {
    def_node: SyntaxNodePtr,
    value_type: Option<ValueType>,
}

#[derive(Debug, Clone, PartialEq)]
struct Function {
    def_node: SyntaxNodePtr,
    scope: ScopeIndex,
    args: Vec<SymbolIndex>,
    rets: Vec<SymbolIndex>,
}

#[derive(Debug)]
pub struct Variables {
    root: SyntaxNode,
    filename: String,
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    scope_of_node: HashMap<SyntaxNodePtr, ScopeIndex>,
    functions: Vec<Function>,
    def_of_function: HashMap<SymbolIndex, FunctionIndex>,
}

impl Variables {
    pub fn new(filename: String, green: GreenNode) -> Variables {
        let root = SyntaxNode::new_root(green);
        let mut variables = Variables {
            root,
            filename: filename,
            scopes: Vec::new(),
            symbols: Vec::new(),
            scope_of_node: HashMap::new(),
            functions: Vec::new(),
            def_of_function: HashMap::new(),
        };
        variables.collect_identifiers();
        variables
    }

    pub fn dump(&self) {
        println!("Symbols:");
        for symbol in self.symbols.iter() {
            println!("    {:?} (scope_idx: {:?}):", &symbol.id, &symbol.scope_idx);
            for version in &symbol.versions {
                println!("        {:?}", version);
            }
        }
        println!("Functions:");
        for func in self.functions.iter() {
            println!("    {:?}", func);
        }
    }
}

// Collecting identifiers
impl Variables {
    fn collect_identifiers(&mut self) {
        self.scopes.push(Scope {
            parent: None,
            symbols: HashMap::new(),
        });

        #[derive(Clone)]
        struct Frame {
            block: Block,
            next_stmt: usize,
            scope_idx: ScopeIndex,
        }

        let root_block = Block::cast(self.root.clone()).expect("everything starts with a block");
        let mut stack = vec![Frame {
            block: root_block,
            next_stmt: 0,
            scope_idx: 0,
        }];

        while let Some(frame) = stack.last_mut() {
            let scope_idx = frame.scope_idx;
            let statements = frame.block.statements();
            if frame.next_stmt >= statements.len() {
                // Finished this block
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
                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                        if let Some(Expression::Function(func)) = expression {
                            let new_scope_idx = self.insert_function_definition(&func, scope_idx, symbol_idx);
                            let inner_block = func.block().expect("FunctionDefinition must have a block");
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                            });
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
                    });
                },
                Statement::FunctionDefinition(func) => {
                    let name = func.name().expect("Standalone function definition must have a name"); // TODO: There are probably other cases
                    let node = SyntaxNodePtr::new(func.syntax());
                    let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name), scope_idx, node);
                    let new_scope_idx = self.insert_function_definition(func, scope_idx, symbol_idx);
                    let inner_block = func.block().expect("FunctionDefinition must have a block");
                    stack.push(Frame {
                        block: inner_block,
                        next_stmt: 0,
                        scope_idx: new_scope_idx,
                    });
                },
                Statement::Return(ret) => {
                    let node = SyntaxNodePtr::new(ret.syntax());
                    // TODO: Better way to get the current function
                    assert!(!self.functions.is_empty());
                    let func_id = self.functions.len() - 1;
                    let expressions = ret
                        .expression_list()
                        .expect("Return should have an expression_list")
                        .expressions();
                    for index in 0..expressions.len() {
                        let symbol_idx = self.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                        self.functions.get_mut(func_id).unwrap().rets.push(symbol_idx);
                    }
                },
                _ => {},
            }
        }
    }

    fn insert_function_definition(&mut self, func: &FunctionDefinition, scope_idx: ScopeIndex, symbol_idx: usize) -> ScopeIndex {
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
        let function_idx = self.functions.len();
        // TODO: Handle duplicate param names
        for name in param_names.iter() {
            function.args.push(self.insert_symbol(SymbolIdentifier::FunctionArg(function_idx, name.clone()), new_scope_idx, node));
        }
        self.functions.push(function);
        self.def_of_function.insert(symbol_idx, function_idx);
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
            value_type: None,
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
            self.scope_of_node.insert(node, scope_idx);
            symbol_idx
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

// Resolving types
impl Variables {
    pub fn resolve_types(&mut self) {
        #[derive(Debug, Clone)]
        struct SymbolPath {
            symbol_idx: usize,
            version_idx: usize,
        }
        let mut missing_symbols: Vec<SymbolPath> = Vec::new();
        for symbol_idx in 0..self.symbols.len() {
            for version_idx in 0..self.symbols[symbol_idx].versions.len() {
                missing_symbols.push(SymbolPath { symbol_idx, version_idx });
            }
        }
        loop {
            let prev_len = missing_symbols.len();
            missing_symbols.retain(|index| {
                let symbol_idx = index.symbol_idx;
                let version_idx = index.version_idx;
                assert!(self.symbols[symbol_idx].versions[version_idx].value_type.is_none());
                let node = self.symbols[symbol_idx].versions[version_idx].def_node.to_node(&self.root);
                let Some(statement) = Statement::cast(node) else { return true };
                let symbol_id = &self.symbols[symbol_idx].id;
                if let Some(value_type) = self.get_statement_value_type(&statement, symbol_id) {
                    self.symbols[symbol_idx].versions[version_idx].value_type = Some(value_type);
                    false
                } else {
                    true
                }
            });
            if missing_symbols.len() == prev_len {
                // No progress was made, so we're done
                break;
            }
        }
    }

    fn get_statement_value_type(&self, statement: &Statement, symbol_id: &SymbolIdentifier) -> Option<ValueType> {
        match statement {
            Statement::LocalAssign(local_assign) => {
                let SymbolIdentifier::Name(symbol_name) = symbol_id else { return None };
                let name_list = local_assign.name_list()?;
                let names = name_list.names();
                let name_index = names.iter().rposition(|n| n == symbol_name)?;
                let expressions = local_assign.expression_list()?.expressions();
                let node = SyntaxNodePtr::new(local_assign.syntax());
                if let Some(expression) = expressions.get(name_index) {
                    self.get_expression_value_type(expression, node, 0)
                } else if let Some(last) = expressions.last() && matches!(last, Expression::FunctionCall(_)) {
                    let ret_index = name_index - (expressions.len() - 1);
                    self.get_expression_value_type(last, node, ret_index)
                } else {
                    None
                }
            },
            Statement::Return(ret) => {
                let SymbolIdentifier::FunctionRet(_, ret_index) = symbol_id else { return None };
                let expr_list = ret.expression_list()?;
                let expressions = expr_list.expressions();
                let expression = expressions.get(*ret_index)?;
                let node = SyntaxNodePtr::new(ret.syntax());
                self.get_expression_value_type(expression, node, *ret_index) // TODO: is ret_index correct here?
            },
            _ => None,
        }
    }

    fn get_expression_value_type(&self, expression: &Expression, node: SyntaxNodePtr, expression_index: usize) -> Option<ValueType> {
        match expression {
            Expression::Literal(l) => {
                if l.get_string().is_some() {
                    Some(ValueType::String)
                } else if let Some(bool_value) = l.get_bool() {
                    Some(ValueType::Boolean(Some(bool_value)))
                } else if l.get_number().is_some() {
                    Some(ValueType::Number)
                } else if l.is_nil() {
                    Some(ValueType::Nil)
                } else {
                    None
                }
            }
            Expression::BinaryExpression(b) => {
                let terms = b.get_terms();
                let [lhs, rhs] = terms.as_slice() else {
                    panic!("BinaryExpression must have exactly two terms");
                };
                let lhs_type = self.get_expression_value_type(lhs, node, expression_index)?;
                let rhs_type = self.get_expression_value_type(rhs, node, expression_index)?;
                match b.kind() {
                    Operator::Or => {
                        match (&lhs_type, &rhs_type) {
                            (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => {
                                // TODO: Diagnostic warning that this expression is superfluous and the right side is always used
                                Some(rhs_type)
                            },
                            (ValueType::Boolean(Some(true)), _) => {
                                // TODO: Diagnostic warning that the right side is never evaluated
                                Some(lhs_type)
                            },
                            (ValueType::Boolean(None), ValueType::Boolean(_)) => Some(lhs_type),
                            (ValueType::Boolean(None), _) => None, // TODO: Union(Boolean, rhs_type)
                            (ValueType::Number | ValueType::String | ValueType::Function(_) | ValueType::Table, _) => {
                                // TODO: Diagnostic warning that the right side is never evaluated
                                Some(lhs_type)
                            },
                        }
                    },
                    Operator::And => {
                        match (&lhs_type, &rhs_type) {
                            (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => {
                                // TODO: Diagnostic warning that this expression is superfluous and the right side is never evalulated
                                Some(lhs_type)
                            },
                            (ValueType::Boolean(Some(true)) | ValueType::Number | ValueType::String | ValueType::Function(_) | ValueType::Table, _) => {
                                // TODO: Diagnostic warning that this expression is superfluous and the right side is always used
                                Some(rhs_type)
                            },
                            (ValueType::Boolean(None), ValueType::Boolean(Some(true))) => {
                                // TODO: Diagnostic warning that this expression is superfluous and the left side is always used
                                Some(lhs_type)
                            },
                            (_, ValueType::Boolean(Some(false)) | ValueType::Nil) => {
                                // TODO: Diagnostic warning that this expression is superfluous and the right side is always used
                                Some(rhs_type)
                            },
                            (ValueType::Boolean(None), _) => None, // TODO: Union(Boolean, rhs_type)
                        }
                    },
                    Operator::LessThan | Operator::GreaterThan | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals => {
                        Some(ValueType::Boolean(None))
                    },
                    Operator::NotEquals | Operator::Equals => Some(ValueType::Boolean(None)),
                    Operator::Concatenate => {
                        if lhs_type.can_concat_to_string() && rhs_type.can_concat_to_string() {
                            Some(ValueType::String)
                        } else {
                            None // TODO: Syntax error?
                        }
                    },
                    Operator::Add | Operator::Subtract | Operator::Divide | Operator::Multiply | Operator::Modulo | Operator::Hat => {
                        match (lhs_type, rhs_type) {
                            (ValueType::Number, ValueType::Number) => Some(ValueType::Number),
                            (ValueType::Table, _) | (_, ValueType::Table) => None, // TODO: Support metamethods
                            _ => None, // TODO: Syntax error?
                        }
                    },
                    Operator::Not | Operator::ArrayLength | Operator::None => {
                        panic!("Unexpected binary expression")
                    },
                }
            }
            Expression::Identifier(i) => {
                let names = i.names();
                let name = names.first()?;
                let scope_idx = *self.scope_of_node.get(&node)?;
                let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx)?;
                let version_idx = self.symbols[symbol_idx].versions
                    .iter()
                    .rposition(|v| v.def_node.text_range().end() <= node.text_range().start())?;
                self.symbols[symbol_idx].versions[version_idx].value_type.clone()
            }
            Expression::Function(func) => {
                let scope_idx = self.scope_of_node.get(&node);
                Some(ValueType::Function(None))
            },
            Expression::TableConstructor(_) => Some(ValueType::Table),
            Expression::UnaryExpression(_) => None, // TODO
            Expression::GroupedExpression(_) => None, // TODO
            Expression::FunctionCall(call) => {
                let names = call.identifier()?.names();
                let name = names.first()?;
                let scope_idx = *self.scope_of_node.get(&node)?;
                let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx)?;
                let func_idx = self.def_of_function.get(&symbol_idx)?;
                let func = self.functions.get(*func_idx).unwrap();
                let ret_symbol_idx = self.get_symbol(&SymbolIdentifier::FunctionRet(*func_idx, expression_index), func.scope)?;
                // TODO: Get the right version
                let ret_version_idx = 0;
                self.symbols[ret_symbol_idx].versions[ret_version_idx].value_type.clone()
            }, // TODO
        }
    }
}
