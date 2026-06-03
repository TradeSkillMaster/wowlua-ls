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
        if is_bracket_access {
            // Collect names from the base (before `[`), then try to extract
            // a string literal key from the bracket content.
            for child in node.children_with_tokens() {
                if let NodeOrToken::Token(ref t) = child
                    && t.kind() == SyntaxKind::LeftSquareBracket {
                        break;
                    }
                match child {
                    NodeOrToken::Node(n) => match n.kind() {
                        SyntaxKind::NameRef
                        | SyntaxKind::DotAccess
                        | SyntaxKind::BracketAccess => Self::collect_names(n, out),
                        _ => {}
                    }
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => {
                        out.push(t.text().to_string());
                    }
                    _ => {}
                }
            }
            if let Some(key) = extract_bracket_string_key(node) {
                out.push(key);
            }
            return;
        }
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => {
                    match n.kind() {
                        SyntaxKind::NameRef
                        | SyntaxKind::DotAccess
                        | SyntaxKind::BracketAccess => {
                            Self::collect_names(n, out);
                        }
                        SyntaxKind::MethodCall
                        | SyntaxKind::FunctionCall
                        | SyntaxKind::GroupedExpression => {}
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


    /// Returns true when the outermost access is a non-string bracket (e.g.
    /// `ns.field[123]`). In that case, the assignment writes to an *element* of
    /// the table, not to the dot-chain field itself.
    pub(crate) fn has_non_string_bracket_tail(&self) -> bool {
        self.node.kind() == SyntaxKind::BracketAccess
            && extract_bracket_string_key(self.node).is_none()
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
    pub(crate) fn contains_call(&self) -> bool {
        Self::has_call_descendant(self.node)
    }

    fn has_call_descendant(node: SyntaxNode<'a>) -> bool {
        for child in node.children() {
            match child.kind() {
                SyntaxKind::MethodCall | SyntaxKind::FunctionCall => return true,
                SyntaxKind::NameRef | SyntaxKind::DotAccess | SyntaxKind::BracketAccess => {
                    if Self::has_call_descendant(child) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub(crate) fn is_indexed_expression(&self) -> bool {
        self.node.kind() == SyntaxKind::BracketAccess
    }

    /// Returns true when there is a non-string BracketAccess anywhere in the
    /// identifier chain (e.g. `a[x].b`, `a.b[x].c`). Assignments through such
    /// chains target a field on a dynamically-indexed element, so field
    /// inference on the parent table would be incorrect.
    pub(crate) fn has_non_string_bracket_in_chain(&self) -> bool {
        Self::check_non_string_bracket(self.node)
    }

    fn check_non_string_bracket(node: SyntaxNode<'a>) -> bool {
        if node.kind() == SyntaxKind::BracketAccess
            && extract_bracket_string_key(node).is_none()
        {
            return true;
        }
        for child in node.children() {
            match child.kind() {
                SyntaxKind::NameRef | SyntaxKind::DotAccess | SyntaxKind::BracketAccess => {
                    if Self::check_non_string_bracket(child) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    /// Like `names()` but includes numeric literal bracket keys in the chain
    /// (formatted as `[N]`). Used for nil-guard field narrowing where numeric
    /// indices are part of the identity (e.g. `obj.arr[1].field`).
    pub(crate) fn names_with_brackets(&self) -> Vec<String> {
        let mut result = Vec::new();
        Self::collect_names_with_brackets(self.node, &mut result);
        result
    }

    fn collect_names_with_brackets(node: SyntaxNode<'a>, out: &mut Vec<String>) {
        let is_bracket_access = node.kind() == SyntaxKind::BracketAccess;
        if is_bracket_access {
            for child in node.children_with_tokens() {
                if let NodeOrToken::Token(ref t) = child
                    && t.kind() == SyntaxKind::LeftSquareBracket {
                        break;
                    }
                match child {
                    NodeOrToken::Node(n) => match n.kind() {
                        SyntaxKind::NameRef
                        | SyntaxKind::DotAccess
                        | SyntaxKind::BracketAccess => Self::collect_names_with_brackets(n, out),
                        _ => {}
                    }
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => {
                        out.push(t.text().to_string());
                    }
                    _ => {}
                }
            }
            if let Some(key) = extract_bracket_literal_key(node) {
                out.push(key);
            } else if let Some(var_key) = extract_bracket_variable_key(node) {
                out.push(var_key);
            }
            return;
        }
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => {
                    match n.kind() {
                        SyntaxKind::NameRef
                        | SyntaxKind::DotAccess
                        | SyntaxKind::BracketAccess => {
                            Self::collect_names_with_brackets(n, out);
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

    /// Returns true if any bracket access in the chain has a non-literal key.
    pub(crate) fn has_any_dynamic_bracket(&self) -> bool {
        Self::check_dynamic_bracket_in_chain(self.node)
    }

    /// Returns true if any bracket access in the chain has a key that is neither
    /// a literal nor a simple variable reference. Simple variable keys like `[KEY]`
    /// are considered resolvable for matching purposes.
    pub(crate) fn has_complex_dynamic_bracket(&self) -> bool {
        Self::check_complex_dynamic_bracket_in_chain(self.node)
    }

    fn check_dynamic_bracket_in_chain(node: SyntaxNode<'a>) -> bool {
        if node.kind() == SyntaxKind::BracketAccess
            && extract_bracket_literal_key(node).is_none() {
            return true;
        }
        for child in node.children() {
            match child.kind() {
                SyntaxKind::NameRef | SyntaxKind::DotAccess | SyntaxKind::BracketAccess => {
                    if Self::check_dynamic_bracket_in_chain(child) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn check_complex_dynamic_bracket_in_chain(node: SyntaxNode<'a>) -> bool {
        if node.kind() == SyntaxKind::BracketAccess
            && extract_bracket_literal_key(node).is_none()
            && extract_bracket_variable_key(node).is_none() {
            return true;
        }
        for child in node.children() {
            match child.kind() {
                SyntaxKind::NameRef | SyntaxKind::DotAccess | SyntaxKind::BracketAccess => {
                    if Self::check_complex_dynamic_bracket_in_chain(child) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
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

fn syntax_kind_to_operator(kind: SyntaxKind) -> Option<Operator> {
    match kind {
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
}

impl<'a> BinaryExpression<'a> {
    pub(crate) fn kind(&self) -> Operator {
        self.node.children_with_tokens()
            .find_map(|node| syntax_kind_to_operator(node.kind()))
            .unwrap_or(Operator::None)
    }
    pub(crate) fn op_token_range(&self) -> Option<crate::syntax::TextRange> {
        self.node.children_with_tokens()
            .find_map(|node| syntax_kind_to_operator(node.kind()).map(|_| node.text_range()))
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

pub(crate) fn extract_bracket_string_key(node: SyntaxNode<'_>) -> Option<String> {
    let mut seen_bracket = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::LeftSquareBracket => {
                seen_bracket = true;
            }
            NodeOrToken::Node(n) if seen_bracket => {
                if let Some(lit) = Literal::cast(n)
                    && let Some(raw) = lit.get_string() {
                        return Some(raw.trim_matches(|c| c == '"' || c == '\'').to_string());
                    }
                return None;
            }
            _ => {}
        }
    }
    None
}

/// Extract the variable name used as a bracket key when it's a simple NameRef.
/// For `tbl[DIALOG_NAME]`, returns `Some("DIALOG_NAME")`.
/// Returns None for complex expressions, literals, or multi-part identifiers.
pub(crate) fn extract_bracket_variable_key(node: SyntaxNode<'_>) -> Option<String> {
    let mut seen_bracket = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::LeftSquareBracket => {
                seen_bracket = true;
            }
            NodeOrToken::Node(n) if seen_bracket => {
                // Must be a simple NameRef with a single Name token
                if n.kind() == SyntaxKind::NameRef {
                    let mut name = None;
                    let mut count = 0;
                    for c in n.children_with_tokens() {
                        if let NodeOrToken::Token(t) = c
                            && t.kind() == SyntaxKind::Name {
                                name = Some(t.text().to_string());
                                count += 1;
                            }
                    }
                    if count == 1 {
                        return name;
                    }
                }
                return None;
            }
            _ => {}
        }
    }
    None
}

/// Like `extract_bracket_string_key` but also handles numeric literal keys.
/// Returns the string key bare (e.g. `"foo"`) or numeric keys wrapped in brackets (e.g. `"[1]"`).
pub(crate) fn extract_bracket_literal_key(node: SyntaxNode<'_>) -> Option<String> {
    let mut seen_bracket = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::LeftSquareBracket => {
                seen_bracket = true;
            }
            NodeOrToken::Node(n) if seen_bracket => {
                if let Some(lit) = Literal::cast(n) {
                    if let Some(raw) = lit.get_string() {
                        return Some(raw.trim_matches(|c| c == '"' || c == '\'').to_string());
                    }
                    if let Some(num) = lit.get_number() {
                        return Some(format!("[{}]", num));
                    }
                }
                return None;
            }
            _ => {}
        }
    }
    None
}

