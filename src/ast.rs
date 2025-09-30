use rowan::NodeOrToken;

use crate::syntax::SyntaxNode;
use crate::syntax::SyntaxKind;

pub trait AstNode {
    fn cast(node: SyntaxNode) -> Option<Self>
    where
        Self: Sized;

    fn syntax(&self) -> &SyntaxNode;
}

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
}

impl AstNode for Statement {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::Assign => Some(Self::Assign(Assign{node})),
            SyntaxKind::LocalAssignStatement => Some(Self::LocalAssign(LocalAssign{node})),
            SyntaxKind::FunctionCall => Some(Self::FunctionCall(FunctionCall{node})),
            SyntaxKind::DoBlock => Some(Self::Do(DoGroup{node})),
            SyntaxKind::WhileLoop => Some(Self::While(WhileLoop{node})),
            SyntaxKind::RepeatUntilLoop => Some(Self::Repeat(RepeatUntilLoop{node})),
            SyntaxKind::IfChain => Some(Self::If(IfChain{node})),
            SyntaxKind::ForCountLoop => Some(Self::ForCountLoop(ForCountLoop{node})),
            SyntaxKind::ForInLoop => Some(Self::ForInLoop(ForInLoop{node})),
            SyntaxKind::FunctionDefinition => Some(Self::FunctionDefinition(FunctionDefinition{node})),
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
        }
    }
}

pub struct FunctionDefinition {
    node: SyntaxNode
}
impl AstNode for FunctionDefinition {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::FunctionDefinition => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}
impl FunctionDefinition {
    pub fn is_local(&self) -> bool {
        match self.node.first_child_or_token_by_kind(&|k| k == SyntaxKind::LocalKeyword) {
            Some(_) => true,
            _ => false,
        }
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

pub struct ParameterList {
    node: SyntaxNode
}

impl AstNode for ParameterList {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ParameterList => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl ParameterList {
    fn parameters(&self) -> Vec<String> {
        self.node.children_with_tokens().filter_map(|t| match t  {
            NodeOrToken::Token(t) => if t.kind() == SyntaxKind::Parameter { Some(t.text().to_string()) } else { None },
            _ => None,
        }).collect()
    }
    fn ellipsis(&self) -> bool {
        self.node.children_with_tokens().any(|t| match t  {
            NodeOrToken::Token(t) => t.kind() == SyntaxKind::ParameterVarArgs,
            _ => false,
        })
    }
}

pub struct Identifier {
    node: SyntaxNode
}

impl AstNode for Identifier {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::Identifier => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl Identifier {
    fn names(&self) -> Vec<String> {
        self.node.children_with_tokens().filter_map(|n|
            match n {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::Name => Some(t.text().to_string()),
                    _ => None
                }
                _ => None,
            }).collect()
    }
    fn is_call_to_self(&self) -> bool {
        self.node.children_with_tokens().any(|n|
            match n {
                NodeOrToken::Token(t) => t.kind() == SyntaxKind::Colon,
                _ => false,
            }
        )
    }
    fn is_indexed_expression(&self) -> bool {
        self.node.children().any(|n| n.kind() == SyntaxKind::Expression)
    }
    fn final_expression(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
}

pub struct Block {
    node: SyntaxNode
}

impl AstNode for Block {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::Block => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl Block {
    pub fn statements(&self) -> Vec<Statement> {
        self.node.children().filter_map(Statement::cast).collect()
    }
}

pub struct Assign {
    node: SyntaxNode
}

impl AstNode for Assign {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::AssignStatement => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl Assign {
    fn variable_list(&self) -> Option<VariableList> {
        self.node.children().find_map(VariableList::cast)
    }
    fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

pub struct VariableList {
    node: SyntaxNode
}

impl AstNode for VariableList {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::VariableList => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl VariableList {
    fn identifiers(&self) -> Vec<Identifier> {
        self.node.children().filter_map(Identifier::cast).collect()
    }
}

pub struct ExpressionList {
    node: SyntaxNode
}

impl AstNode for ExpressionList {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ExpressionList => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl ExpressionList {
    fn expressions(&self) -> Vec<Expression> {
        self.node.children().filter_map(Expression::cast).collect()
    }
}

pub struct Literal {
    node: SyntaxNode,
}

impl AstNode for Literal {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::Literal => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl Literal {
    fn get_string(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::String => Some(String::from(self.node.text())),
            _ => None
        })
    }
    fn get_number(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::Number => Some(String::from(self.node.text())),
            _ => None
        })
    }
    fn get_bool(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::TrueKeyword => Some(String::from(self.node.text())),
            SyntaxKind::FalseKeyword => Some(String::from(self.node.text())),
            _ => None
        })
    }
    fn is_nil(&self) -> bool {
        self.node.children_with_tokens().any(|t| match t.kind() {
            SyntaxKind::NilKeyword => true,
            _ => false
        })
    }
}

pub enum Expression {
    UnaryExpression(UnaryExpression),
    BinaryExpression(BinaryExpression),
    GroupedExpression(GroupedExpression),

    Identifier(Identifier),
    Literal(Literal),
}

pub enum Operator {
    Or, And, Not,
    LessThan, GreaterThan, LessThanOrEquals, GreaterThanOrEquals, NotEquals, Equals,
    Concatenate,
    Add, Subtract, Multiply, Divide, Modulo,
    ArrayLength,
    Hat,
    None,
}

impl AstNode for Expression {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::UnaryExpression => Some(Self::UnaryExpression(UnaryExpression{node})),
            SyntaxKind::BinaryExpression => Some(Self::BinaryExpression(BinaryExpression{node})),
            SyntaxKind::GroupedExpression => Some(Self::GroupedExpression(GroupedExpression{node})),
            SyntaxKind::Identifier => Some(Self::Identifier(Identifier{node})),
            SyntaxKind::Literal => Some(Self::Literal(Literal{node})),
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
        }
    }
}

pub struct UnaryExpression {
    node: SyntaxNode
}

impl AstNode for UnaryExpression {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ExpressionList => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl UnaryExpression {
    fn kind(&self) -> Operator {
        let some_op = self.node.children_with_tokens().find_map(|token|
            match token.kind() {
                SyntaxKind::NotKeyword => Some(Operator::Not),
                SyntaxKind::Minus => Some(Operator::Subtract),
                SyntaxKind::Hash => Some(Operator::ArrayLength),
                SyntaxKind::Hat => Some(Operator::Hat),
                _ => None,
            }
        );
        match some_op {
            Some(y) => y,
            None => Operator::None,
        }
    }
    fn get_terms(&self) -> Vec<Expression> {
        self.node.children().filter_map(Expression::cast).collect()
    }
}

pub struct BinaryExpression {
    node: SyntaxNode
}

impl AstNode for BinaryExpression {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ExpressionList => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl BinaryExpression {
    fn kind(&self) -> Operator {
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
                _ => None,
            }
        );
        match some_op {
            Some(x) => x,
            None => Operator::None,
        }
    }
}

pub struct GroupedExpression {
    node: SyntaxNode
}

impl AstNode for GroupedExpression {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::GroupedExpression => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl GroupedExpression {
    fn get_expression(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
    fn get_term(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
}

pub struct LocalAssign {
    node: SyntaxNode
}

impl AstNode for LocalAssign {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::LocalAssignStatement => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl LocalAssign {
    fn name_list(&self) -> Option<NameList> {
        self.node.children().find_map(NameList::cast)
    }
    fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

pub struct NameList {
    node: SyntaxNode
}

impl AstNode for NameList {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::NameList => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl NameList {
    fn names(&self) -> Vec<String> {
        self.node.children_with_tokens().filter_map(|t| match t  {
            NodeOrToken::Token(t) => if t.kind() == SyntaxKind::Name { Some(t.text().to_string()) } else { None },
            _ => None,
        }).collect()
    }
}

pub struct FunctionCall {
    node: SyntaxNode
}

impl AstNode for FunctionCall {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::FunctionCall => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl FunctionCall {
    fn identifier(&self) -> Option<Identifier> {
        self.node.children().find_map(Identifier::cast)
    }
    fn arguments(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

pub struct DoGroup {
    node: SyntaxNode
}

impl AstNode for DoGroup {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::DoBlock => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl DoGroup {
    fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

pub struct WhileLoop {
    node: SyntaxNode
}

impl AstNode for WhileLoop {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::WhileLoop => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl WhileLoop {
    fn condition(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
    fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

pub struct RepeatUntilLoop {
    node: SyntaxNode
}

impl AstNode for RepeatUntilLoop {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::RepeatUntilLoop => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl RepeatUntilLoop {
    fn condition(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
    fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

pub struct ForCountLoop {
    node: SyntaxNode
}

impl AstNode for ForCountLoop {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ForCountLoop => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl ForCountLoop {
    fn name(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|n|
            match n {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::Name => Some(t.text().to_string()),
                    _ => None
                }
                _ => None
            })
    }
    fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
    fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

pub struct ForInLoop {
    node: SyntaxNode
}

impl AstNode for ForInLoop {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ForInLoop => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl ForInLoop {
    fn name_list(&self) -> Option<NameList> {
        self.node.children().find_map(NameList::cast)
    }
    fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
    fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

pub struct IfChain {
    node: SyntaxNode
}

impl AstNode for IfChain {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::IfChain => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl IfChain {
    fn if_branches(&self) -> Vec<IfBranch> {
        self.node.children().filter_map(IfBranch::cast).collect()
    }
    fn else_branch(&self) -> Option<ElseBranch> {
        self.node.children().find_map(ElseBranch::cast)
    }
}

pub struct IfBranch {
    node: SyntaxNode
}

impl AstNode for IfBranch {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::IfBranch => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl IfBranch {
    fn expression(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
    fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

pub struct ElseBranch {
    node: SyntaxNode
}

impl AstNode for ElseBranch {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ElseBranch => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl ElseBranch {
    fn expression(&self) -> Option<Expression> {
        self.node.children().find_map(Expression::cast)
    }
    fn block(&self) -> Option<Block> {
        self.node.children().find_map(Block::cast)
    }
}

pub struct TableConstructor {
    node: SyntaxNode
}

impl AstNode for TableConstructor {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::TableConstructor => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> &SyntaxNode {
        &self.node
    }
}

impl TableConstructor {
    fn expression_list(&self) -> Option<ExpressionList> {
        self.node.children().find_map(ExpressionList::cast)
    }
}
