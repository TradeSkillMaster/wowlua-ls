#![allow(dead_code)]

use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken};
use crate::syntax::SyntaxKind;

pub(crate) trait AstNode<'a>: Sized {
    fn cast(node: SyntaxNode<'a>) -> Option<Self>;
    fn syntax(&self) -> SyntaxNode<'a>;
}

macro_rules! define_ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone, Copy)]
        pub(crate) struct $name<'a> { node: SyntaxNode<'a> }
        impl<'a> AstNode<'a> for $name<'a> {
            fn cast(node: SyntaxNode<'a>) -> Option<Self> {
                match node.kind() {
                    SyntaxKind::$kind => Some(Self { node }),
                    _ => None,
                }
            }
            fn syntax(&self) -> SyntaxNode<'a> { self.node }
        }
    };
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Statement<'a> {
    Assign(Assign<'a>),
    LocalAssign(LocalAssign<'a>),
    FunctionCall(FunctionCall<'a>),
    Do(DoGroup<'a>),
    While(WhileLoop<'a>),
    Repeat(RepeatUntilLoop<'a>),
    If(IfChain<'a>),
    ForCountLoop(ForCountLoop<'a>),
    ForInLoop(ForInLoop<'a>),
    FunctionDefinition(FunctionDefinition<'a>),
    Return(Return<'a>),
}

impl<'a> AstNode<'a> for Statement<'a> {
    fn cast(node: SyntaxNode<'a>) -> Option<Self> {
        match node.kind() {
            SyntaxKind::AssignStatement => Some(Self::Assign(Assign{node})),
            SyntaxKind::LocalAssignStatement => Some(Self::LocalAssign(LocalAssign{node})),
            SyntaxKind::FunctionCall | SyntaxKind::MethodCall => Some(Self::FunctionCall(FunctionCall{node})),
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
    fn syntax(&self) -> SyntaxNode<'a> {
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

impl<'a> FunctionDefinition<'a> {
    pub(crate) fn is_local(&self) -> bool {
        self.node.first_child_or_token_by_kind(&|k| k == SyntaxKind::LocalKeyword).is_some()
    }
    pub(crate) fn name(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|n|
            match n {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::Name => Some(t.text().to_string()),
                    _ => None
                }
                _ => None
            })
    }
    pub(crate) fn identifier(&self) -> Option<Identifier<'a>> {
        self.node.children().find_map(Identifier::cast)
    }
    pub(crate) fn params(&self) -> Option<ParameterList<'a>> {
        self.node.children().find_map(ParameterList::cast)
    }
    pub(crate) fn block(&self) -> Option<Block<'a>> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(ParameterList, ParameterList);

impl<'a> ParameterList<'a> {
    pub(crate) fn parameters(&self) -> Vec<String> {
        self.node.children_with_tokens().filter_map(|t| match t  {
            NodeOrToken::Token(t) => if t.kind() == SyntaxKind::Parameter { Some(t.text().to_string()) } else { None },
            _ => None,
        }).collect()
    }
    pub(crate) fn ellipsis(&self) -> bool {
        self.node.children_with_tokens().any(|t| match t  {
            NodeOrToken::Token(t) => t.kind() == SyntaxKind::ParameterVarArgs,
            _ => false,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Name<'a> {
    node: SyntaxNode<'a>
}

impl<'a> AstNode<'a> for Name<'a> {
    fn cast(node: SyntaxNode<'a>) -> Option<Self> {
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

    fn syntax(&self) -> SyntaxNode<'a> {
        self.node
    }
}

impl<'a> Name<'a> {
    pub(crate) fn text(&self) -> String {
        self.node.children_with_tokens()
            .find_map(|n| match n {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text().to_string()),
                _ => None,
            })
            .unwrap_or_else(|| self.node.text().to_string())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Identifier<'a> { node: SyntaxNode<'a> }

impl<'a> AstNode<'a> for Identifier<'a> {
    fn cast(node: SyntaxNode<'a>) -> Option<Self> {
        match node.kind() {
            SyntaxKind::NameRef
            | SyntaxKind::DotAccess
            | SyntaxKind::MethodCall
            | SyntaxKind::BracketAccess => Some(Self { node }),
            _ => None,
        }
    }
    fn syntax(&self) -> SyntaxNode<'a> { self.node }
}

impl<'a> Identifier<'a> {
    /// Collect all Name tokens from this identifier chain.
    /// For parser2's split nodes (NameRef/DotAccess/MethodCall), this recursively
    /// walks nested identifier children to gather names in left-to-right order.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut result = Vec::new();
        Self::collect_names(self.node, &mut result);
        result
    }

    fn collect_names(node: SyntaxNode<'a>, out: &mut Vec<String>) {
        let is_bracket_access = node.kind() == SyntaxKind::BracketAccess;
        for child in node.children_with_tokens() {
            // Inside BracketAccess, stop collecting at `[` — bracket keys
            // are index expressions, not name segments in the dot chain.
            if is_bracket_access
                && let NodeOrToken::Token(ref t) = child
                    && t.kind() == SyntaxKind::LeftSquareBracket {
                        break;
                    }
            match child {
                NodeOrToken::Node(n) => {
                    match n.kind() {
                        SyntaxKind::NameRef
                        | SyntaxKind::DotAccess
                        | SyntaxKind::BracketAccess => {
                            // Recurse into pure identifier nodes (not MethodCall/FunctionCall
                            // which contain call args and represent nested calls)
                            Self::collect_names(n, out);
                        }
                        SyntaxKind::MethodCall
                        | SyntaxKind::FunctionCall
                        | SyntaxKind::GroupedExpression => {
                            // Don't recurse into call-like children — they represent
                            // nested calls in the chain, not name segments.
                        }
                        _ => {}
                    }
                }
                NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::Name {
                        out.push(t.text().to_string());
                    }
                }
            }
        }
    }

    pub(crate) fn is_call_to_self(&self) -> bool {
        // Check this node and any nested identifier nodes for a Colon token
        self.node.children_with_tokens().any(|n|
            match n {
                NodeOrToken::Token(t) => t.kind() == SyntaxKind::Colon,
                _ => false,
            }
        ) || matches!(self.node.kind(), SyntaxKind::MethodCall)
    }
    pub(crate) fn is_indexed_expression(&self) -> bool {
        self.node.kind() == SyntaxKind::BracketAccess
    }
    pub(crate) fn final_expression(&self) -> Option<Expression<'a>> {
        self.node.children().find_map(Expression::cast)
    }
}

define_ast_node!(Block, Block);

impl<'a> Block<'a> {
    pub(crate) fn statements(&self) -> Vec<Statement<'a>> {
        self.node.children().filter_map(Statement::cast).collect()
    }
}

define_ast_node!(Assign, AssignStatement);

impl<'a> Assign<'a> {
    pub(crate) fn variable_list(&self) -> Option<VariableList<'a>> {
        self.node.children().find_map(VariableList::cast)
    }
    pub(crate) fn expression_list(&self) -> Option<ExpressionList<'a>> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

define_ast_node!(VariableList, VariableList);

impl<'a> VariableList<'a> {
    pub(crate) fn identifiers(&self) -> Vec<Identifier<'a>> {
        self.node.children().filter_map(Identifier::cast).collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExpressionList<'a> {
    node: SyntaxNode<'a>
}

impl<'a> AstNode<'a> for ExpressionList<'a> {
    fn cast(node: SyntaxNode<'a>) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ExpressionList | SyntaxKind::ArgumentList => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> SyntaxNode<'a> {
        self.node
    }
}

impl<'a> ExpressionList<'a> {
    pub(crate) fn expressions(&self) -> Vec<Expression<'a>> {
        self.node.children().filter_map(Expression::cast).collect()
    }
}

define_ast_node!(Literal, Literal);

impl<'a> Literal<'a> {
    pub(crate) fn get_string(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::String => Some(self.node.text().to_string()),
            _ => None
        })
    }
    pub(crate) fn get_number(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::Number => Some(self.node.text().to_string()),
            _ => None
        })
    }
    pub(crate) fn get_bool(&self) -> Option<bool> {
        self.node.children_with_tokens().find_map(|t| match t.kind() {
            SyntaxKind::TrueKeyword => Some(true),
            SyntaxKind::FalseKeyword => Some(false),
            _ => None
        })
    }
    pub(crate) fn is_nil(&self) -> bool {
        self.node.children_with_tokens().any(|t| matches!(t.kind(), SyntaxKind::NilKeyword))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Expression<'a> {
    UnaryExpression(UnaryExpression<'a>),
    BinaryExpression(BinaryExpression<'a>),
    GroupedExpression(GroupedExpression<'a>),

    Identifier(Identifier<'a>),
    Literal(Literal<'a>),
    Function(FunctionDefinition<'a>),
    FunctionCall(FunctionCall<'a>),
    TableConstructor(TableConstructor<'a>),
    VarArgs(VarArgs<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct VarArgs<'a> {
    node: SyntaxNode<'a>,
}

impl<'a> VarArgs<'a> {
    pub(crate) fn syntax(&self) -> SyntaxNode<'a> {
        self.node
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Operator {
    Or, And, Not,
    LessThan, GreaterThan, LessThanOrEquals, GreaterThanOrEquals, NotEquals, Equals,
    Concatenate,
    Add, Subtract, Multiply, Divide, Modulo,
    ArrayLength,
    Hat,
    None,
}

impl Operator {
    pub(crate) fn is_arithmetic(self) -> bool {
        matches!(self, Self::Add | Self::Subtract | Self::Multiply | Self::Divide | Self::Modulo | Self::Hat)
    }

    pub(crate) fn is_comparison(self) -> bool {
        matches!(self, Self::LessThan | Self::GreaterThan | Self::LessThanOrEquals | Self::GreaterThanOrEquals | Self::Equals | Self::NotEquals)
    }
}

impl<'a> AstNode<'a> for Expression<'a> {
    fn cast(node: SyntaxNode<'a>) -> Option<Self> {
        match node.kind() {
            SyntaxKind::UnaryExpression => Some(Self::UnaryExpression(UnaryExpression{node})),
            SyntaxKind::BinaryExpression => Some(Self::BinaryExpression(BinaryExpression{node})),
            SyntaxKind::GroupedExpression => Some(Self::GroupedExpression(GroupedExpression{node})),
            SyntaxKind::NameRef
            | SyntaxKind::DotAccess
            | SyntaxKind::BracketAccess => Some(Self::Identifier(Identifier{node})),
            // MethodCall in parser2 includes args (like FunctionCall) — treat as FunctionCall
            SyntaxKind::MethodCall => Some(Self::FunctionCall(FunctionCall{node})),
            SyntaxKind::Literal => {
                // A Literal node containing TripleDot is a VarArgs expression
                if node.children_with_tokens().any(|t| t.kind() == SyntaxKind::TripleDot) {
                    Some(Self::VarArgs(VarArgs { node }))
                } else {
                    Some(Self::Literal(Literal{node}))
                }
            }
            SyntaxKind::FunctionDefinition => Some(Self::Function(FunctionDefinition{node})),
            SyntaxKind::FunctionCall => Some(Self::FunctionCall(FunctionCall{node})),
            SyntaxKind::TableConstructor => Some(Self::TableConstructor(TableConstructor{node})),
            _ => None,
        }
    }
    fn syntax(&self) -> SyntaxNode<'a> {
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

#[derive(Debug, Clone, Copy)]
pub struct UnaryExpression<'a> {
    node: SyntaxNode<'a>
}

impl<'a> AstNode<'a> for UnaryExpression<'a> {
    fn cast(node: SyntaxNode<'a>) -> Option<Self> {
        match node.kind() {
            SyntaxKind::UnaryExpression => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> SyntaxNode<'a> {
        self.node
    }
}

impl<'a> UnaryExpression<'a> {
    pub(crate) fn kind(&self) -> Operator {
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
    pub(crate) fn get_terms(&self) -> Vec<Expression<'a>> {
        self.node.children().filter_map(Expression::cast).collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BinaryExpression<'a> {
    node: SyntaxNode<'a>
}

impl<'a> AstNode<'a> for BinaryExpression<'a> {
    fn cast(node: SyntaxNode<'a>) -> Option<Self> {
        match node.kind() {
            SyntaxKind::BinaryExpression => Some(Self{node}),
            _ => None,
        }
    }
    fn syntax(&self) -> SyntaxNode<'a> {
        self.node
    }
}

impl<'a> BinaryExpression<'a> {
    pub(crate) fn kind(&self) -> Operator {
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
    pub(crate) fn get_terms(&self) -> Vec<Expression<'a>> {
        self.node.children().filter_map(Expression::cast).collect()
    }
}

define_ast_node!(GroupedExpression, GroupedExpression);

impl<'a> GroupedExpression<'a> {
    pub(crate) fn get_expression(&self) -> Option<Expression<'a>> {
        self.node.children().find_map(Expression::cast)
    }
}

define_ast_node!(LocalAssign, LocalAssignStatement);

impl<'a> LocalAssign<'a> {
    pub(crate) fn name_list(&self) -> Option<NameList<'a>> {
        self.node.children().find_map(NameList::cast)
    }
    pub(crate) fn expression_list(&self) -> Option<ExpressionList<'a>> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

define_ast_node!(NameList, NameList);

impl<'a> NameList<'a> {
    pub(crate) fn names(&self) -> Vec<String> {
        self.node.children_with_tokens().filter_map(|t| match t  {
            NodeOrToken::Token(t) => if t.kind() == SyntaxKind::Name { Some(t.text().to_string()) } else { None },
            _ => None,
        }).collect()
    }
    pub(crate) fn name_tokens(&self) -> Vec<SyntaxToken<'a>> {
        self.node.children_with_tokens()
            .filter_map(|t| t.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect()
    }
}

define_ast_node!(Return, ReturnStatement);

impl<'a> Return<'a> {
    pub(crate) fn expression_list(&self) -> Option<ExpressionList<'a>> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FunctionCall<'a> { node: SyntaxNode<'a> }

impl<'a> AstNode<'a> for FunctionCall<'a> {
    fn cast(node: SyntaxNode<'a>) -> Option<Self> {
        match node.kind() {
            SyntaxKind::FunctionCall | SyntaxKind::MethodCall => Some(Self { node }),
            _ => None,
        }
    }
    fn syntax(&self) -> SyntaxNode<'a> { self.node }
}

impl<'a> FunctionCall<'a> {
    pub(crate) fn identifier(&self) -> Option<Identifier<'a>> {
        if self.node.kind() == SyntaxKind::MethodCall {
            // For MethodCall, the node itself acts as the identifier
            // (it contains NameRef/DotAccess/etc prefix + Colon + Name)
            Some(Identifier { node: self.node })
        } else {
            // For FunctionCall, find the identifier child
            self.node.children().find_map(Identifier::cast)
        }
    }
    pub(crate) fn arguments(&self) -> Option<ExpressionList<'a>> {
        self.node.children().find_map(ExpressionList::cast)
    }
}

define_ast_node!(DoGroup, DoBlock);

impl<'a> DoGroup<'a> {
    pub(crate) fn block(&self) -> Option<Block<'a>> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(WhileLoop, WhileLoop);

impl<'a> WhileLoop<'a> {
    pub(crate) fn condition(&self) -> Option<Expression<'a>> {
        self.node.children()
            .find(|n| n.kind() == SyntaxKind::Condition)
            .and_then(|cond_node| cond_node.children().find_map(Expression::cast))
    }
    pub(crate) fn block(&self) -> Option<Block<'a>> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(RepeatUntilLoop, RepeatUntilLoop);

impl<'a> RepeatUntilLoop<'a> {
    pub(crate) fn condition(&self) -> Option<Expression<'a>> {
        self.node.children().find_map(Expression::cast)
    }
    pub(crate) fn block(&self) -> Option<Block<'a>> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(ForCountLoop, ForCountLoop);

impl<'a> ForCountLoop<'a> {
    pub(crate) fn name(&self) -> Option<String> {
        self.node.children_with_tokens().find_map(|n|
            match n {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::Name => Some(t.text().to_string()),
                    _ => None
                }
                _ => None
            })
    }
    pub(crate) fn expression_list(&self) -> Option<ExpressionList<'a>> {
        self.node.children().find_map(ExpressionList::cast)
    }
    pub(crate) fn block(&self) -> Option<Block<'a>> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(ForInLoop, ForInLoop);

impl<'a> ForInLoop<'a> {
    pub(crate) fn name_list(&self) -> Option<NameList<'a>> {
        self.node.children().find_map(NameList::cast)
    }
    pub(crate) fn expression_list(&self) -> Option<ExpressionList<'a>> {
        self.node.children().find_map(ExpressionList::cast)
    }
    pub(crate) fn block(&self) -> Option<Block<'a>> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(IfChain, IfChain);

impl<'a> IfChain<'a> {
    pub(crate) fn if_branches(&self) -> Vec<IfBranch<'a>> {
        self.node.children().filter_map(IfBranch::cast).collect()
    }
    pub(crate) fn else_branch(&self) -> Option<ElseBranch<'a>> {
        self.node.children().find_map(ElseBranch::cast)
    }
}

define_ast_node!(IfBranch, IfBranch);

impl<'a> IfBranch<'a> {
    pub(crate) fn expression(&self) -> Option<Expression<'a>> {
        // The condition is wrapped in a Condition node
        self.node.children()
            .find(|n| n.kind() == SyntaxKind::Condition)
            .and_then(|cond| cond.children().find_map(Expression::cast))
    }
    pub(crate) fn block(&self) -> Option<Block<'a>> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(ElseBranch, ElseBranch);

impl<'a> ElseBranch<'a> {
    pub(crate) fn expression(&self) -> Option<Expression<'a>> {
        self.node.children().find_map(Expression::cast)
    }
    pub(crate) fn block(&self) -> Option<Block<'a>> {
        self.node.children().find_map(Block::cast)
    }
}

define_ast_node!(TableConstructor, TableConstructor);

impl<'a> TableConstructor<'a> {
    pub(crate) fn expression_list(&self) -> Option<ExpressionList<'a>> {
        self.node.children().find_map(ExpressionList::cast)
    }
    pub(crate) fn fields(&self) -> Vec<Field<'a>> {
        self.node.children().filter_map(Field::cast).collect()
    }
}

define_ast_node!(Field, Field);

pub(crate) enum FieldKind<'a> {
    Named { name: String, value: Expression<'a> },
    Positional(Expression<'a>),
}

impl<'a> Field<'a> {
    pub(crate) fn kind(&self) -> Option<FieldKind<'a>> {
        let has_assign = self.node.children_with_tokens().any(|n| {
            matches!(n, NodeOrToken::Token(ref t) if t.kind() == SyntaxKind::Assign)
        });
        let has_bracket = self.node.children_with_tokens().any(|n| {
            matches!(n, NodeOrToken::Token(ref t) if t.kind() == SyntaxKind::LeftSquareBracket)
        });
        if has_assign && has_bracket {
            // Bracket-keyed field: [expr] = Expression — return None so build_ir
            // handles it by lowering both key and value expressions.
            return None;
        }
        if has_assign {
            // Named field: Name = Expression
            // In parser2 the name is a bare Name token; in old parser it may be
            // wrapped in an Identifier node. Find the name before `=`.
            let name = self.node.children_with_tokens().find_map(|n| match n {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text().to_string()),
                NodeOrToken::Node(n) if n.kind().is_identifier() => {
                    n.children_with_tokens().find_map(|c| match c {
                        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text().to_string()),
                        _ => None,
                    })
                }
                _ => None,
            })?;
            // Find the value expression after the `=` token.
            let mut seen_assign = false;
            let value = self.node.children_with_tokens().find_map(|n| {
                match &n {
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::Assign => {
                        seen_assign = true;
                        None
                    }
                    NodeOrToken::Node(node) if seen_assign => {
                        Expression::cast(*node)
                    }
                    _ => None,
                }
            })?;
            Some(FieldKind::Named { name, value })
        } else {
            // Positional field: just an expression (or bare name used as variable ref)
            let value = self.node.children().find_map(Expression::cast)?;
            Some(FieldKind::Positional(value))
        }
    }
}
