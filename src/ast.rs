#![allow(dead_code)]

use rowan::NodeOrToken;

use crate::syntax::SyntaxNode;
use crate::syntax::SyntaxKind;
use crate::syntax::SyntaxToken;

pub trait AstNode {
    fn cast(node: SyntaxNode) -> Option<Self>
    where
        Self: Sized;

    fn syntax(&self) -> &SyntaxNode;
}

macro_rules! define_ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone)]
        pub struct $name { node: SyntaxNode }
        impl AstNode for $name {
            fn cast(node: SyntaxNode) -> Option<Self> {
                match node.kind() {
                    SyntaxKind::$kind => Some(Self { node }),
                    _ => None,
                }
            }
            fn syntax(&self) -> &SyntaxNode { &self.node }
        }
    };
}

#[derive(Debug)]
pub enum Statement {
    Assign(Assign),
    LocalAssign(LocalAssign),
    FunctionCall(FunctionCall),
    Do(DoGroup),
    While(WhileLoop),
    Repeat(RepeatUntilLoop),
    If(IfChain),
    ForCountLoop(ForCountLoop),
    ForInLoop(ForInLoop),
    FunctionDefinition(FunctionDefinition),
    Return(Return),
}

impl AstNode for Statement {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::AssignStatement => Some(Self::Assign(Assign{node})),
            SyntaxKind::LocalAssignStatement => Some(Self::LocalAssign(LocalAssign{node})),
            SyntaxKind::FunctionCall => Some(Self::FunctionCall(FunctionCall{node})),
            SyntaxKind::DoBlock => Some(Self::Do(DoGroup{node})),
            SyntaxKind::WhileLoop => Some(Self::While(WhileLoop{node})),
            SyntaxKind::RepeatUntilLoop => Some(Self::Repeat(RepeatUntilLoop{node})),
            SyntaxKind::IfChain => Some(Self::If(IfChain{node})),
            SyntaxKind::ForCountLoop => Some(Self::ForCountLoop(ForCountLoop{node})),
            SyntaxKind::ForInLoop => Some(Self::ForInLoop(ForInLoop{node})),
            SyntaxKind::FunctionDefinition => Some(Self::FunctionDefinition(FunctionDefinition{node})),
            SyntaxKind::ReturnStatement => Some(Self::Return(Return{node})),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Assign(x) => x.syntax(),
            Self::LocalAssign(x) => x.syntax(),
            Self::FunctionCall(x) => x.syntax(),
            Self::Do(x) => x.syntax(),
            Self::While(x) => x.syntax(),
            Self::Repeat(x) => x.syntax(),
            Self::If(x) => x.syntax(),
            Self::ForCountLoop(x) => x.syntax(),
            Self::ForInLoop(x) => x.syntax(),
            Self::FunctionDefinition(x) => x.syntax(),
            Self::Return(x) => x.syntax(),
        }
    }
}

define_ast_node!(FunctionDefinition, FunctionDefinition);

impl FunctionDefinition {
    pub fn is_local(&self) -> bool {
        self.node.first_child_or_token_by_kind(&|k| k == SyntaxKind::LocalKeyword).is_some()
    }
    pub fn name(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|n|
            match n {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::Name => Some(t.text().to_string()),
                    _ => None
                }
                _ => None
            })
    }
    pub fn identifier(&self) -> Option<Identifier> {
        self.node.children().find_map(Identifier::cast)
    }
    pub fn params(&self) -> Option<ParameterList> {
        self.node.children().find_map(ParameterList::cast)
    }
    pub fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(ParameterList, ParameterList);

impl ParameterList {
    pub fn parameters(&self) -> Vec<String> {
        self.node.children_with_tokens().filter_map(|t| match t  {
            NodeOrToken::Token(t) => if t.kind() == SyntaxKind::Parameter { Some(t.text().to_string()) } else { None },
            _ => None,
        }).collect()
    }
    pub fn ellipsis(&self) -> bool {
        self.node.children_with_tokens().any(|t| match t  {
            NodeOrToken::Token(t) => t.kind() == SyntaxKind::ParameterVarArgs,
            _ => false,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Name {
    node: SyntaxNode
}

impl AstNode for Name {
    fn cast(node: SyntaxNode) -> Option<Self> {
        // Check if this node contains Name tokens
        let has_name_token = node.children_with_tokens().any(|n| match n {
            NodeOrToken::Token(t) => t.kind() == SyntaxKind::Name,
            _ => false,
        });
        if has_name_token {
            Some(Self{node})
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl Name {
    pub fn text(&self) -> String {
        self.node.children_with_tokens()
            .find_map(|n| match n {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text().to_string()),
                _ => None,
            })
            .unwrap_or_else(|| self.node.text().to_string())
    }
}

define_ast_node!(Identifier, Identifier);

impl Identifier {
    pub fn names(&self) -> Vec<String> {
        self.node.children_with_tokens().filter_map(|n|
            match n {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::Name => Some(t.text().to_string()),
                    _ => None
                }
                _ => None,
            }).collect()
    }
    pub fn is_call_to_self(&self) -> bool {
        self.node.children_with_tokens().any(|n|
            match n {
                NodeOrToken::Token(t) => t.kind() == SyntaxKind::Colon,
                _ => false,
            }
        )
    }
    pub fn is_indexed_expression(&self) -> bool {
        self.node.children().any(|n| n.kind() == SyntaxKind::Expression)
    }
    pub fn final_expression(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
}

define_ast_node!(Block, Block);

impl Block {
    pub fn statements(&self) -> Vec<Statement> {
        self.node.children().filter_map(Statement::cast).collect()
    }
}

define_ast_node!(Assign, AssignStatement);

impl Assign {
    pub fn variable_list(&self) -> Option<VariableList> {
        self.node.children().find_map(VariableList::cast)
    }
    pub fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

define_ast_node!(VariableList, VariableList);

impl VariableList {
    pub fn identifiers(&self) -> Vec<Identifier> {
        self.node.children().filter_map(Identifier::cast).collect()
    }
}

#[derive(Debug, Clone)]
pub struct ExpressionList {
    node: SyntaxNode
}

impl AstNode for ExpressionList {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ExpressionList | SyntaxKind::ArgumentList => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl ExpressionList {
    pub fn expressions(&self) -> Vec<Expression> {
        self.node.children().filter_map(Expression::cast).collect()
    }
}

define_ast_node!(Literal, Literal);

impl Literal {
    pub fn get_string(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::String => Some(String::from(self.node.text())),
            _ => None
        })
    }
    pub fn get_number(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::Number => Some(String::from(self.node.text())),
            _ => None
        })
    }
    pub fn get_bool(&self) -> Option<bool> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::TrueKeyword => Some(true),
            SyntaxKind::FalseKeyword => Some(false),
            _ => None
        })
    }
    pub fn is_nil(&self) -> bool {
        self.node.children_with_tokens().any(|t| match t.kind() {
            SyntaxKind::NilKeyword => true,
            _ => false
        })
    }
}

#[derive(Debug)]
pub enum Expression {
    UnaryExpression(UnaryExpression),
    BinaryExpression(BinaryExpression),
    GroupedExpression(GroupedExpression),

    Identifier(Identifier),
    Literal(Literal),
    Function(FunctionDefinition),
    FunctionCall(FunctionCall),
    TableConstructor(TableConstructor),
    VarArgs(VarArgs),
}

#[derive(Debug, Clone)]
pub struct VarArgs {
    node: SyntaxNode,
}

impl VarArgs {
    pub fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operator {
    Or, And, Not,
    LessThan, GreaterThan, LessThanOrEquals, GreaterThanOrEquals, NotEquals, Equals,
    Concatenate,
    Add, Subtract, Multiply, Divide, Modulo,
    ArrayLength,
    Hat,
    None,
}

impl Operator {
    pub fn is_arithmetic(self) -> bool {
        matches!(self, Self::Add | Self::Subtract | Self::Multiply | Self::Divide | Self::Modulo | Self::Hat)
    }

    pub fn is_comparison(self) -> bool {
        matches!(self, Self::LessThan | Self::GreaterThan | Self::LessThanOrEquals | Self::GreaterThanOrEquals | Self::Equals | Self::NotEquals)
    }
}

impl AstNode for Expression {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::UnaryExpression => Some(Self::UnaryExpression(UnaryExpression{node})),
            SyntaxKind::BinaryExpression => Some(Self::BinaryExpression(BinaryExpression{node})),
            SyntaxKind::GroupedExpression => Some(Self::GroupedExpression(GroupedExpression{node})),
            SyntaxKind::Identifier => Some(Self::Identifier(Identifier{node})),
            SyntaxKind::Literal => Some(Self::Literal(Literal{node})),
            SyntaxKind::FunctionDefinition => Some(Self::Function(FunctionDefinition{node})),
            SyntaxKind::FunctionCall => Some(Self::FunctionCall(FunctionCall{node})),
            SyntaxKind::TableConstructor => Some(Self::TableConstructor(TableConstructor{node})),
            SyntaxKind::Expression => {
                if let Some(expr) = node.children().find_map(Self::cast) {
                    Some(expr)
                } else if node.children_with_tokens().any(|t| t.kind() == SyntaxKind::TripleDot) {
                    Some(Self::VarArgs(VarArgs { node }))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::UnaryExpression(x) => x.syntax(),
            Self::BinaryExpression(x) => x.syntax(),
            Self::GroupedExpression(x) => x.syntax(),
            Self::Identifier(x) => x.syntax(),
            Self::Literal(x) => x.syntax(),
            Self::Function(x) => x.syntax(),
            Self::FunctionCall(x) => x.syntax(),
            Self::TableConstructor(x) => x.syntax(),
            Self::VarArgs(x) => x.syntax(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnaryExpression {
    node: SyntaxNode
}

impl AstNode for UnaryExpression {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::UnaryExpression => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl UnaryExpression {
    pub fn kind(&self) -> Operator {
        let some_op = self.node.children_with_tokens().find_map(|token|
            match token.kind() {
                SyntaxKind::NotKeyword => Some(Operator::Not),
                SyntaxKind::Minus => Some(Operator::Subtract),
                SyntaxKind::Hash => Some(Operator::ArrayLength),
                _ => None,
            }
        );
        some_op.unwrap_or(Operator::None)
    }
    pub fn get_terms(&self) -> Vec<Expression> {
        self.node.children().filter_map(Expression::cast).collect()
    }
}

#[derive(Debug, Clone)]
pub struct BinaryExpression {
    node: SyntaxNode
}

impl AstNode for BinaryExpression {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::BinaryExpression => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl BinaryExpression {
    pub fn kind(&self) -> Operator {
        let some_op = self.node.children_with_tokens().find_map(|node|
            match node.kind() {
                SyntaxKind::OrKeyword => Some(Operator::Or),
                SyntaxKind::AndKeyword => Some(Operator::And),
                SyntaxKind::LessThan => Some(Operator::LessThan),
                SyntaxKind::GreaterThan => Some(Operator::GreaterThan),
                SyntaxKind::LessThanOrEquals => Some(Operator::LessThanOrEquals),
                SyntaxKind::GreaterThanOrEquals => Some(Operator::GreaterThanOrEquals),
                SyntaxKind::NotEqualsBoolean => Some(Operator::NotEquals),
                SyntaxKind::EqualsBoolean => Some(Operator::Equals),
                SyntaxKind::DoubleDot => Some(Operator::Concatenate),
                SyntaxKind::Plus => Some(Operator::Add),
                SyntaxKind::Minus => Some(Operator::Subtract),
                SyntaxKind::Asterisk => Some(Operator::Multiply),
                SyntaxKind::Slash => Some(Operator::Divide),
                SyntaxKind::Modulo => Some(Operator::Modulo),
                SyntaxKind::Hat => Some(Operator::Hat),
                _ => None,
            }
        );
        some_op.unwrap_or(Operator::None)
    }
    pub fn get_terms(&self) -> Vec<Expression> {
        self.node.children().filter_map(Expression::cast).collect()
    }
}

define_ast_node!(GroupedExpression, GroupedExpression);

impl GroupedExpression {
    pub fn get_expression(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
}

define_ast_node!(LocalAssign, LocalAssignStatement);

impl LocalAssign {
    pub fn name_list(&self) -> Option<NameList> {
        self.node.children().find_map(NameList::cast)
    }
    pub fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

define_ast_node!(NameList, NameList);

impl NameList {
    pub fn names(&self) -> Vec<String> {
        self.node.children_with_tokens().filter_map(|t| match t  {
            NodeOrToken::Token(t) => if t.kind() == SyntaxKind::Name { Some(t.text().to_string()) } else { None },
            _ => None,
        }).collect()
    }
    pub fn name_tokens(&self) -> Vec<SyntaxToken> {
        self.node.children_with_tokens()
            .filter_map(|t| t.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect()
    }
}

define_ast_node!(Return, ReturnStatement);

impl Return {
    pub fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

define_ast_node!(FunctionCall, FunctionCall);

impl FunctionCall {
    pub fn identifier(&self) -> Option<Identifier> {
        self.node.children().find_map(Identifier::cast)
    }
    pub fn arguments(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

define_ast_node!(DoGroup, DoBlock);

impl DoGroup {
    pub fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(WhileLoop, WhileLoop);

impl WhileLoop {
    pub fn condition(&self) -> Option<Expression> {
        self.node.children()
            .find(|n| n.kind() == SyntaxKind::Condition)
            .and_then(|cond_node| cond_node.children().find_map(Expression::cast))
    }
    pub fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(RepeatUntilLoop, RepeatUntilLoop);

impl RepeatUntilLoop {
    pub fn condition(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
    pub fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(ForCountLoop, ForCountLoop);

impl ForCountLoop {
    pub fn name(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|n|
            match n {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::Name => Some(t.text().to_string()),
                    _ => None
                }
                _ => None
            })
    }
    pub fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
    pub fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(ForInLoop, ForInLoop);

impl ForInLoop {
    pub fn name_list(&self) -> Option<NameList> {
        self.node.children().find_map(NameList::cast)
    }
    pub fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
    pub fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(IfChain, IfChain);

impl IfChain {
    pub fn if_branches(&self) -> Vec<IfBranch> {
        self.node.children().filter_map(IfBranch::cast).collect()
    }
    pub fn else_branch(&self) -> Option<ElseBranch> {
        self.node.children().find_map(ElseBranch::cast)
    }
}

define_ast_node!(IfBranch, IfBranch);

impl IfBranch {
    pub fn expression(&self) -> Option<Expression> {
        // The condition is wrapped in a Condition node
        self.node.children()
            .find(|n| n.kind() == SyntaxKind::Condition)
            .and_then(|cond| cond.children().find_map(Expression::cast))
    }
    pub fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(ElseBranch, ElseBranch);

impl ElseBranch {
    pub fn expression(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
    pub fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(TableConstructor, TableConstructor);

impl TableConstructor {
    pub fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
    pub fn fields(&self) -> Vec<Field> {
        self.node.children().filter_map(Field::cast).collect()
    }
}

define_ast_node!(Field, Field);

pub enum FieldKind {
    Named { name: String, value: Expression },
    Positional(Expression),
}

impl Field {
    pub fn kind(&self) -> Option<FieldKind> {
        let has_assign = self.node.children_with_tokens().any(|n| {
            matches!(n, NodeOrToken::Token(ref t) if t.kind() == SyntaxKind::Assign)
        });
        if has_assign {
            // Named field: Name = Expression
            let name = self.node.children_with_tokens().find_map(|n| match n {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text().to_string()),
                NodeOrToken::Node(n) if n.kind() == SyntaxKind::Identifier => {
                    n.children_with_tokens().find_map(|c| match c {
                        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text().to_string()),
                        _ => None,
                    })
                }
                _ => None,
            })?;
            let value = self.node.children()
                .find_map(|n| if n.kind() == SyntaxKind::Expression { Expression::cast(n) } else { None })?;
            Some(FieldKind::Named { name, value })
        } else {
            // Positional field: just an expression (or bare name used as variable ref)
            let value = self.node.children().find_map(Expression::cast)?;
            Some(FieldKind::Positional(value))
        }
    }
}
