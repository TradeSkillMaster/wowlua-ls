macro_rules! define_syntax_kind {
    ($( $(#[$attr:meta])* $variant:ident ),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum SyntaxKind {
            $( $(#[$attr])* $variant ),*
        }
    };
}

define_syntax_kind! {
    Invalid,
    Whitespace,
    Newline,
    Comment,
    Name,
    String,
    Number,
    EoF,
    Dot, //.
    DoubleDot, //..
    TripleDot, //...
    LeftBracket, //(
    RightBracket, //)
    LeftCurlyBracket, //{
    RightCurlyBracket, //}
    LeftSquareBracket, //[
    RightSquareBracket, //]
    Minus,
    Plus,
    Asterisk,
    Slash,
    Modulo,
    Semicolon,
    Colon,
    EqualsBoolean,
    NotEqualsBoolean,
    LessThanOrEquals,
    GreaterThanOrEquals,
    LessThan,
    GreaterThan,
    Assign,
    Comma,
    Hash,
    Hat,

    Block,
    FunctionDefinition,
    FunctionCall,
    DoBlock,
    WhileLoop,
    RepeatUntilLoop,
    ForCountLoop,
    ForInLoop,
    AssignStatement,
    LocalAssignStatement,
    ReturnStatement,
    VariableList,
    NameList,
    ExpressionList,
    BinaryExpression,
    UnaryExpression,
    GroupedExpression,
    ArgumentList,
    ParameterList,
    Parameter,
    ParameterVarArgs,
    TableConstructor,
    Condition,
    IfChain,
    IfBranch,
    ElseBranch,
    Field,
    //IndexingVariable,
    Literal,

    // New parser: split Identifier into specific access forms
    NameRef,
    DotAccess,
    MethodCall,
    BracketAccess,

    AndKeyword,
    BreakKeyword,
    DoKeyword,
    ElseKeyword,
    ElseIfKeyword,
    EndKeyword,
    FalseKeyword,
    ForKeyword,
    FunctionKeyword,
    IfKeyword,
    InKeyword,
    LocalKeyword,
    NilKeyword,
    NotKeyword,
    OrKeyword,
    RepeatKeyword,
    ReturnKeyword,
    ThenKeyword,
    TrueKeyword,
    UntilKeyword,
    WhileKeyword,
}

impl SyntaxKind {
    /// Returns true if this kind represents an identifier-like node
    /// (NameRef, DotAccess, MethodCall, BracketAccess).
    pub fn is_identifier(self) -> bool {
        matches!(self,
            SyntaxKind::NameRef
            | SyntaxKind::DotAccess
            | SyntaxKind::MethodCall
            | SyntaxKind::BracketAccess
        )
    }

    /// Returns true if this is a trivia token (whitespace, newline, or comment).
    pub fn is_trivia(self) -> bool {
        matches!(self, Self::Whitespace | Self::Newline | Self::Comment)
    }

    /// Returns true if this is a keyword.
    pub fn is_keyword(self) -> bool {
        matches!(self,
            Self::AndKeyword | Self::BreakKeyword | Self::DoKeyword | Self::ElseKeyword | Self::ElseIfKeyword |
            Self::EndKeyword | Self::FalseKeyword | Self::ForKeyword | Self::FunctionKeyword | Self::IfKeyword |
            Self::InKeyword | Self::LocalKeyword | Self::NilKeyword | Self::NotKeyword | Self::OrKeyword |
            Self::RepeatKeyword | Self::ReturnKeyword | Self::ThenKeyword | Self::TrueKeyword |
            Self::UntilKeyword | Self::WhileKeyword
        )
    }

}

