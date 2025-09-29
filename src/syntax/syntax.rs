//Copyright (C) 2025-  plusmouse and other contributors
//
//This program is free software: you can redistribute it and/or modify
//it under the terms of the GNU General Public License as published by
//the Free Software Foundation, either version 3 of the License, or
//(at your option) any later version.
//
//This program is distributed in the hope that it will be useful,
//but WITHOUT ANY WARRANTY; without even the implied warranty of
//MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//GNU General Public License for more details.
//
//You should have received a copy of the GNU General Public License
//along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::syntax::lexer::token_validity;
use crate::syntax::lexer::Generator as TokenGenerator;
use crate::syntax::lexer::Token;
use crate::syntax::lexer::TokenKind;
use std::cmp::min;
use rowan::GreenNodeBuilder;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SyntaxKind {
    Invalid,
    Whitespace,
    Newline,
    Comment,
    Identifier,
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
    Statement,
    AssignStatement,
    LocalAssignStatement,
    ReturnStatement,
    VariableList,
    NameList,
    Expression,
    ExpressionList,
    BinaryExpression,
    UnaryExpression,
    GroupedExpression,
    PrefixExpression,
    ArgumentList,
    ParameterList,
    Parameter,
    ParameterVarArgs,
    TableConstructor,
    FieldList,
    Condition,
    IfChain,
    IfBranch,
    ElseBranch,
    Field,
    //IndexingVariable,
    Literal,

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

    __LAST,
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorKind {
    NotClosedBlock,
    NotClosedComment,
    NotTerminatedString,
    InvalidNumberFormat,
    UnexpectedKeyword,
    UnexpectedToken,
    UnexpectedOperator,
    ExpectingComma,
    ExpectingCommaOrBracket,
    ExpectingThen,
    ExpectingDo,
    ExpectingToken,
    ExpectingName,
    ExpectingClosingBracket,
    ExpectingFunctionCall,
    ExpectingExpression,
    InvalidName,
    InvalidFunction,
}
#[derive(Debug, Clone, Copy)]
pub struct Error {
    pub start: usize,
    pub end: usize,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExpressionKind {
    FunctionCall,
    Name,
    Identifier,
    Literal,
    Nested,
    Combined,
    None,
}

fn str_to_keyword(text: &str) -> SyntaxKind {
    match text {
        "and" => SyntaxKind::AndKeyword,
        "break" => SyntaxKind::BreakKeyword,
        "do" => SyntaxKind::DoKeyword,
        "else" => SyntaxKind::ElseKeyword,
        "elseif" => SyntaxKind::ElseIfKeyword,
        "end" => SyntaxKind::EndKeyword,
        "false" => SyntaxKind::FalseKeyword,
        "for" => SyntaxKind::ForKeyword,
        "function" => SyntaxKind::FunctionKeyword,
        "if" => SyntaxKind::IfKeyword,
        "in" => SyntaxKind::InKeyword,
        "local" => SyntaxKind::LocalKeyword,
        "nil" => SyntaxKind::NilKeyword,
        "not" => SyntaxKind::NotKeyword,
        "or" => SyntaxKind::OrKeyword,
        "repeat" => SyntaxKind::RepeatKeyword,
        "return" => SyntaxKind::ReturnKeyword,
        "then" => SyntaxKind::ThenKeyword,
        "true" => SyntaxKind::TrueKeyword,
        "until" => SyntaxKind::UntilKeyword,
        "while" => SyntaxKind::WhileKeyword,
        _ => SyntaxKind::Name,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}
impl rowan::Language for Lang {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= SyntaxKind::__LAST as u16);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Lang>;
#[allow(unused)]
pub type SyntaxToken = rowan::SyntaxToken<Lang>;
#[allow(unused)]
pub type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;
#[allow(unused)]
pub type SyntaxNodePtr = rowan::ast::SyntaxNodePtr<Lang>;

#[inline(always)]
fn to_raw(s: SyntaxKind) -> rowan::SyntaxKind {
    rowan::SyntaxKind(s as u16)
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    #[inline]
    fn from(s: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(s as u16)
    }
}
impl From<u16> for SyntaxKind {
    #[inline]
    fn from(d: u16) -> SyntaxKind {
        assert!(d <= (SyntaxKind::__LAST as u16));
        unsafe { std::mem::transmute::<u16, SyntaxKind>(d) }
    }
}

pub struct Generator<'a> {
    text: &'a str,
    lexer: TokenGenerator<'a>,
    builder: GreenNodeBuilder<'a>,
    errors: Vec<Error>,
    token_cache: Option<Token>,
}

impl<'a> Generator<'a> {
    pub fn new(text: &'a str) -> Generator<'a> {
        Generator {
            text: text,
            lexer: TokenGenerator::new(text),
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            token_cache: None,
        }
    }

    pub fn errors(&self) -> &Vec<Error> {
        &self.errors
    }

    #[inline]
    fn next_raw_token(&mut self) -> Option<Token> {
        if let Some(t) = self.token_cache {
            self.token_cache = None;
            return Some(t)
        } else {
            return self.lexer.next_token()
        }
    }

    #[inline]
    fn peek_raw_token(&mut self) -> Option<Token> {
        if let Some(t) = self.token_cache {
            return Some(t)
        } else {
            self.token_cache = self.lexer.next_token();
            return self.token_cache
        }
    }

    #[inline]
    fn get_current_position(&mut self) -> usize {
        let mut end = self.text.len();
        if let Some(t) = self.peek_raw_token() {
            end = t.start;
        }
        return end
    }

    pub fn process_all(&mut self) -> rowan::GreenNode {
        if let Some(first) = self.peek_raw_token() {
            self.scan_block(None, &first);
        }
        let b = std::mem::take(&mut self.builder);
        return b.finish()
    }

    fn eat_whitespace(&mut self) {
        while let Some(token) = self.peek_raw_token() {
            match token.kind {
                TokenKind::Whitespace => {
                    self.builder.token(to_raw(SyntaxKind::Whitespace), &self.text[token.start..token.end])
                },
                TokenKind::Newline => { 
                    self.builder.token(to_raw(SyntaxKind::Newline), &self.text[token.start..token.end])
                },
                TokenKind::Comment { validity: v, modifier: _ } => {
                    let text = &self.text[token.start .. token.end];
                    if v == crate::syntax::lexer::token_validity::Comment::NotTerminated {
                        self.errors.push(Error{ start: token.start, end: self.text.len(), kind: ErrorKind::NotClosedComment });
                    }
                    self.builder.token(to_raw(SyntaxKind::Comment), text)
                },
                _ => return,
            };
            self.next_raw_token();
        }
    }

    fn scan_function_identifier(&mut self, token: &Token, _text: &str) -> ExpressionKind {
        self.builder.start_node(to_raw(SyntaxKind::Identifier));
        let mut t = *token;
        let mut skip_forward = false;
        let mut id_expected = true;
        let mut terminate_next = false;
        let mut multiple_ids = false;
        loop {
            let text = &self.text[t.start..t.end];
            match t.kind {
                TokenKind::Identifier => {
                    if !id_expected {
                        break;
                    }
                    let keyword_kind = str_to_keyword(text);
                    if keyword_kind != SyntaxKind::Name {
                        self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::InvalidName });
                        self.builder.token(to_raw(keyword_kind), text);
                    } else {
                        self.builder.token(to_raw(SyntaxKind::Name), text);
                    }
                    if skip_forward {
                        self.next_raw_token();
                    }
                    id_expected = false;
                    if terminate_next {
                        break;
                    }
                }
                TokenKind::Dot => {
                    if id_expected {
                        self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::InvalidName });
                        break
                    }
                    self.builder.token(to_raw(SyntaxKind::Dot), text);
                    if skip_forward {
                        self.next_raw_token();
                    }
                    id_expected = true;
                    multiple_ids = true;
                }
                TokenKind::Colon => {
                    if id_expected {
                        self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::InvalidName });
                        break
                    }
                    terminate_next = true;
                    id_expected = true;
                    multiple_ids = true;
                    self.builder.token(to_raw(SyntaxKind::Colon), text);
                    if skip_forward {
                        self.next_raw_token();
                    }
                }
                _ => {
                    break;
                }
            }
            skip_forward = true;
            if let Some(next) = self.peek_raw_token() {
                t = next;
            } else {
                break;
            }
            self.eat_whitespace();
        }
        self.builder.finish_node();

        if terminate_next {
            return ExpressionKind::FunctionCall
        } else if !multiple_ids {
            return ExpressionKind::Name
        } else if !id_expected {
            return ExpressionKind::Identifier
        } else {
            return ExpressionKind::None
        }
    }

    fn scan_expression_part(&mut self) -> ExpressionKind {
        if let Some(t) = self.peek_raw_token() {
            match t.kind {
                TokenKind::Number{validity, modifier: _} => {
                    if validity == token_validity::Number::Invalid {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::InvalidNumberFormat });
                    }
                    self.next_raw_token();
                    self.builder.start_node(to_raw(SyntaxKind::Literal));
                    self.builder.token(to_raw(SyntaxKind::Number), &self.text[t.start..t.end]);
                    self.builder.finish_node();
                    return ExpressionKind::Literal
                },
                TokenKind::String{validity, modifier: _} => {
                    if validity == token_validity::String::NotTerminated {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::NotTerminatedString });
                    }
                    self.next_raw_token();
                    self.builder.start_node(to_raw(SyntaxKind::Literal));
                    self.builder.token(to_raw(SyntaxKind::String), &self.text[t.start..t.end]);
                    self.builder.finish_node();
                    return ExpressionKind::Literal
                },
                TokenKind::TripleDot => {
                    self.next_raw_token();
                    self.builder.token(to_raw(SyntaxKind::TripleDot), &self.text[t.start..t.end]);
                    return ExpressionKind::Nested
                },
                TokenKind::Identifier | TokenKind::LeftBracket => {
                    let text = &self.text[t.start..t.end];
                    let keyword_kind = str_to_keyword(text);
                    match keyword_kind {
                        SyntaxKind::Name => {
                            let mut kind = ExpressionKind::None;
                            if t.kind == TokenKind::LeftBracket {
                                kind = ExpressionKind::Nested;
                            }
                            let checkpoint = self.builder.checkpoint();
                            let mut started_group = false;
                            let mut mod_required = false;
                            loop {
                                let mut new_kind = ExpressionKind::None;
                                if let Some(t) = self.peek_raw_token() {
                                    match t.kind {
                                        TokenKind::LeftBracket => {
                                            self.next_raw_token();
                                            new_kind = self.scan_preexp(&t, text);
                                            mod_required = true;
                                        }
                                        TokenKind::Identifier => {
                                            if mod_required {
                                                break;
                                            }
                                            if str_to_keyword(&self.text[t.start..t.end]) == SyntaxKind::Name {
                                                self.next_raw_token();
                                                new_kind = self.scan_preexp(&t, text);
                                            }
                                        }
                                        _ => if !mod_required { break },
                                    }
                                }
                                if new_kind == ExpressionKind::None && !mod_required {
                                    break;
                                }
                                mod_required = false;
                                if kind == ExpressionKind::None {
                                    kind = new_kind;
                                } else {
                                    kind = ExpressionKind::Combined;
                                }
                                self.eat_whitespace();
                                if let Some(t) = self.peek_raw_token() {
                                    let text = &self.text[t.start..t.end];
                                    match t.kind {
                                        TokenKind::Dot => {
                                            if !started_group {
                                                self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Identifier));
                                                started_group = true;
                                            }
                                            self.builder.token(to_raw(SyntaxKind::Dot), text);
                                            self.next_raw_token();
                                        }
                                        TokenKind::Colon => {
                                            if !started_group {
                                                self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Identifier));
                                                started_group = true;
                                            }
                                            self.next_raw_token();
                                            self.builder.token(to_raw(SyntaxKind::Colon), text);
                                            self.eat_whitespace();
                                            let start = t.start;
                                            if let Some(t) = self.peek_raw_token() {
                                                let text = &self.text[t.start..t.end];
                                                if t.kind != TokenKind::Identifier || str_to_keyword(text) != SyntaxKind::Name {
                                                    self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::ExpectingName });
                                                    break;
                                                } else {
                                                    self.builder.token(to_raw(SyntaxKind::Name), text);
                                                    self.next_raw_token();
                                                    self.eat_whitespace();
                                                    if !self.scan_arguments() {
                                                        self.errors.push(Error { start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                                                        break
                                                    }
                                                }
                                            }
                                            mod_required = true;
                                        }
                                        TokenKind::LeftSquareBracket => {
                                            if started_group {
                                                self.builder.finish_node();
                                                started_group = false;
                                            }
                                            self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Identifier));
                                            self.next_raw_token();
                                            self.scan_indexing_variable(&t);
                                            self.builder.finish_node();
                                            mod_required = true;
                                        }
                                        TokenKind::LeftBracket | TokenKind::String{validity: _, modifier: _} | TokenKind::LeftCurlyBracket => {
                                            if started_group {
                                                self.builder.finish_node();
                                                started_group = false;
                                            }
                                            self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::FunctionCall));
                                            self.scan_arguments();
                                            self.builder.finish_node();
                                            mod_required = true;
                                        }
                                        _ => break,
                                    }
                                }
                                self.eat_whitespace();
                            }
                            if started_group {
                                self.builder.finish_node();
                            }
                            return kind
                        },
                        SyntaxKind::NilKeyword | SyntaxKind::FalseKeyword | SyntaxKind::TrueKeyword => {
                            self.next_raw_token();
                            self.builder.token(to_raw(keyword_kind), text);
                            return ExpressionKind::Literal
                        }
                        SyntaxKind::FunctionKeyword => {
                            self.next_raw_token();
                            self.builder.start_node(to_raw(SyntaxKind::FunctionDefinition));
                            self.builder.token(to_raw(SyntaxKind::FunctionKeyword), text);
                            if self.scan_parameters() {
                                self.scan_block(Some(SyntaxKind::EndKeyword), &t);
                                self.builder.finish_node();
                                return ExpressionKind::Literal
                            } else {
                                self.builder.finish_node();
                                return ExpressionKind::None
                            }
                        }
                        _ => {
                            return ExpressionKind::None
                        }
                    }
                },
                TokenKind::LeftCurlyBracket => {
                    self.next_raw_token();
                    self.builder.start_node(to_raw(SyntaxKind::TableConstructor));
                    self.builder.token(to_raw(SyntaxKind::LeftCurlyBracket), &self.text[t.start..t.end]);
                    self.eat_whitespace();
                    self.scan_field_list();
                    self.eat_whitespace();
                    if let Some(t) = self.peek_raw_token() {
                        if t.kind != TokenKind::RightCurlyBracket {
                            self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::ExpectingClosingBracket });
                            self.builder.finish_node();
                            return ExpressionKind::None
                        } else {
                            self.next_raw_token();
                            self.builder.token(to_raw(SyntaxKind::RightCurlyBracket), &self.text[t.start..t.end]);
                            self.builder.finish_node();
                        }
                    } else {
                        self.builder.finish_node();
                        return ExpressionKind::None
                    }
                    return ExpressionKind::Literal
                }
                _ => {
                    return ExpressionKind::None
                },
            }
        } else {
            return ExpressionKind::None
        }
    }

    fn scan_expression(&mut self) -> ExpressionKind {
        let mut group_kind = ExpressionKind::None;
        let mut binary_possible = false;
        let mut expecting_expression = true;
        let mut checkpoints = [self.builder.checkpoint(); 10];
        let mut is_open = [false; 8];
        const OR_PRIORITY: usize = 1;
        const AND_PRIORITY: usize = 2;
        const COMPARISON_PRIORITY: usize = 3;
        const CONCAT_PRIORITY: usize = 4;
        const PLUS_PRIORITY: usize = 5;
        const MULT_PRIORITY: usize = 6;
        const UNARY_PRIORITY: usize = 7;
        const HAT_PRIORITY: usize = 8;
        const COLON_PRIORITY: usize = 9;
        fn close_nodes(priority: usize, is_open: &mut [bool; 8], builder: &mut GreenNodeBuilder) {
            for j in (priority)..8 {
                if is_open[j] {
                    builder.finish_node();
                    is_open[j] = false;
                }
            }
        }
        fn update_checkpoints(priority: usize, checkpoints: &mut [rowan::Checkpoint; 10], point: rowan::Checkpoint) {
            for j in (priority)..9 {
                checkpoints[j] = point;
            }
        }
        #[inline(always)]
        fn apply_operator(token_kind: SyntaxKind, operator_kind: SyntaxKind, priority: usize, text: &str, builder: &mut GreenNodeBuilder, checkpoints: &mut [rowan::Checkpoint; 10], is_open: &mut [bool; 8]) {
            close_nodes(priority - 1, is_open, builder);
            checkpoints[priority] = builder.checkpoint();
            builder.start_node_at(checkpoints[priority - 1], to_raw(operator_kind));
            is_open[priority - 1] = true;
            builder.token(to_raw(token_kind), text);
            update_checkpoints(min(priority + 1, 8), checkpoints, builder.checkpoint());
        }
        while let Some(t) = self.peek_raw_token() {
            let text = &self.text[t.start..t.end];
            if expecting_expression && !binary_possible {
                let checkpoint = self.builder.checkpoint();
                checkpoints[9] = checkpoint;
                let part_kind = self.scan_expression_part();
                self.eat_whitespace();
                if group_kind == ExpressionKind::None {
                    group_kind = part_kind;
                }
                if part_kind != ExpressionKind::None {
                    if part_kind != ExpressionKind::Name && part_kind != ExpressionKind::Literal {
                        self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Expression));
                        self.builder.finish_node();
                    }
                    binary_possible = true;
                    expecting_expression = false;
                    continue;
                }
            }
            match t.kind {
                TokenKind::Identifier => {
                    let keyword_kind = str_to_keyword(text);
                    match keyword_kind {
                        SyntaxKind::NotKeyword => {
                            self.next_raw_token();
                            checkpoints[UNARY_PRIORITY] = self.builder.checkpoint();
                            expecting_expression = true;
                            apply_operator(SyntaxKind::NotKeyword, SyntaxKind::UnaryExpression, UNARY_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        }
                        SyntaxKind::AndKeyword => {
                            group_kind = ExpressionKind::Combined;
                            self.next_raw_token();
                            if binary_possible {
                                apply_operator(SyntaxKind::AndKeyword, SyntaxKind::BinaryExpression, AND_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                                expecting_expression = true;
                                binary_possible = false;
                            } else {
                                self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                            }
                        }
                        SyntaxKind::OrKeyword => {
                            group_kind = ExpressionKind::Combined;
                            self.next_raw_token();
                            if binary_possible {
                                apply_operator(SyntaxKind::OrKeyword, SyntaxKind::BinaryExpression, OR_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                                expecting_expression = true;
                                binary_possible = false;
                            } else {
                                self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                            }
                        }
                        _ => break,
                    }
                }
                TokenKind::Minus => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::Minus, SyntaxKind::BinaryExpression, PLUS_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        checkpoints[UNARY_PRIORITY] = self.builder.checkpoint();
                        expecting_expression = true;
                        apply_operator(SyntaxKind::Minus, SyntaxKind::UnaryExpression, UNARY_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                    }
                }
                TokenKind::Plus => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::Plus, SyntaxKind::BinaryExpression, PLUS_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::Asterisk => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::Asterisk, SyntaxKind::BinaryExpression, MULT_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::Slash => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::Slash, SyntaxKind::BinaryExpression, MULT_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::Modulo => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::Modulo, SyntaxKind::BinaryExpression, MULT_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::Hash => {
                    if binary_possible {
                        break;
                    }
                    self.next_raw_token();
                    group_kind = ExpressionKind::Combined;
                    checkpoints[UNARY_PRIORITY] = self.builder.checkpoint();
                    expecting_expression = true;
                    apply_operator(SyntaxKind::Hash, SyntaxKind::UnaryExpression, UNARY_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                }
                TokenKind::Hat => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::Hat, SyntaxKind::BinaryExpression, HAT_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::DoubleDot => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::DoubleDot, SyntaxKind::BinaryExpression, CONCAT_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::LessThan => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::LessThan, SyntaxKind::BinaryExpression, COMPARISON_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::LessThanOrEquals => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::LessThanOrEquals, SyntaxKind::BinaryExpression, COMPARISON_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::GreaterThan => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::GreaterThan, SyntaxKind::BinaryExpression, COMPARISON_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::GreaterThanOrEquals => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::GreaterThanOrEquals, SyntaxKind::BinaryExpression, COMPARISON_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::EqualsBoolean => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::EqualsBoolean, SyntaxKind::BinaryExpression, COMPARISON_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                TokenKind::NotEqualsBoolean => {
                    group_kind = ExpressionKind::Combined;
                    self.next_raw_token();
                    if binary_possible {
                        apply_operator(SyntaxKind::LessThan, SyntaxKind::BinaryExpression, COMPARISON_PRIORITY, text, &mut self.builder, &mut checkpoints, &mut is_open);
                        expecting_expression = true;
                        binary_possible = false;
                    } else {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                    }
                }
                _ => {
                    break
                }
            }
            self.eat_whitespace();
        }
        if expecting_expression || !binary_possible {
            group_kind = ExpressionKind::None;
        }
        close_nodes(0, &mut is_open, &mut self.builder);
        return group_kind
    }

    fn scan_expression_list(&mut self) {
        let mut seen_comma = false;
        let mut seen_expression = false;
        loop {
            self.eat_whitespace();
            if seen_comma || !seen_expression {
                seen_comma = false;
                let scanned =  self.scan_expression() != ExpressionKind::None;
                seen_expression = true;
                if !scanned {
                    break
                }
            } else if let Some(t) = self.peek_raw_token() {
                match t.kind {
                    TokenKind::Comma => {
                        self.next_raw_token();
                        self.builder.token(to_raw(SyntaxKind::Comma), &self.text[t.start..t.end]);
                        seen_comma = true;
                        if !seen_expression {
                            self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator })
                        }
                    },
                    _ => {
                        break
                    }
                }
            } else {
                break
            }
        }
    }

    fn scan_preexp(&mut self, token: &Token, text: &str) -> ExpressionKind {
        if token.kind == TokenKind::LeftBracket {
            self.builder.start_node(to_raw(SyntaxKind::GroupedExpression));
            self.builder.token(to_raw(SyntaxKind::LeftBracket), &self.text[token.start..token.end]);
            self.eat_whitespace();
            let mut res = self.scan_expression() != ExpressionKind::None;
            self.eat_whitespace();
            if let Some(t) = self.peek_raw_token() {
                if t.kind == TokenKind::RightBracket {
                    self.next_raw_token();
                    self.builder.token(to_raw(SyntaxKind::RightBracket), &self.text[t.start..t.end]);
                } else {
                    self.errors.push(Error { start: t.start, end: t.end, kind: ErrorKind::ExpectingClosingBracket });
                    res = false;
                }
            }
            self.builder.finish_node();
            if res {
                return ExpressionKind::Nested
            } else {
                return ExpressionKind::None
            }
        }
        let checkpoint = self.builder.checkpoint();
        let kind = self.scan_function_identifier(token, text);
        self.eat_whitespace();
        if kind == ExpressionKind::FunctionCall {
            self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::FunctionCall));
            let scanned = self.scan_arguments();
            self.builder.finish_node();
            if !scanned {
                let end = self.get_current_position();
                self.errors.push(Error{ start: token.start, end, kind: ErrorKind::ExpectingFunctionCall });
            }
            return ExpressionKind::FunctionCall
        } else if kind != ExpressionKind::None {
            while let Some(t) = self.peek_raw_token() {
                match t.kind {
                    TokenKind::LeftBracket | TokenKind::String{validity: _, modifier: _} | TokenKind::LeftCurlyBracket => {
                        self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::FunctionCall));
                        let scanned = self.scan_arguments();
                        self.builder.finish_node();
                        if !scanned {
                            self.errors.push(Error{ start: token.start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                        }
                        return ExpressionKind::FunctionCall
                    }
                    TokenKind::LeftSquareBracket => {
                        self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Identifier));
                        self.next_raw_token();
                        self.scan_indexing_variable(&t);
                        self.builder.finish_node();
                    }
                    _ => break,
                }
            }
            return kind
        } else {
            self.errors.push(Error{ start: token.start, end: token.end, kind: ErrorKind::ExpectingName });
            return ExpressionKind::None
        }
    }

    fn scan_statement(&mut self, token: &Token, text: &str) {
        match str_to_keyword(text) {
            SyntaxKind::DoKeyword => {
                self.builder.start_node(to_raw(SyntaxKind::DoBlock));
                self.builder.token(to_raw(SyntaxKind::DoKeyword), text);
                self.scan_block(Some(SyntaxKind::EndKeyword), &token);
                self.builder.finish_node();
            },
            SyntaxKind::BreakKeyword => {
                self.builder.token(to_raw(SyntaxKind::BreakKeyword), text);
            },
            SyntaxKind::FunctionKeyword => {
                let keyword_token = token;
                self.builder.start_node(to_raw(SyntaxKind::FunctionDefinition));
                self.builder.token(to_raw(SyntaxKind::FunctionKeyword), text);
                self.eat_whitespace();
                if let Some(token) = self.peek_raw_token() {
                    let text = &self.text[token.start .. token.end];
                    match token.kind {
                        TokenKind::Identifier => {
                            self.next_raw_token();
                            self.scan_function_identifier(&token, text);
                            if self.scan_parameters() {
                                self.scan_block(Some(SyntaxKind::EndKeyword), &keyword_token);
                            }
                        }
                        TokenKind::LeftBracket => {
                            if self.scan_parameters() {
                                self.scan_block(Some(SyntaxKind::EndKeyword), &keyword_token);
                            }
                        }
                        _ => {
                            self.errors.push(Error { start: keyword_token.start, end: token.end, kind: ErrorKind::InvalidFunction});
                        }
                    }
                }
                self.builder.finish_node();
            }
            SyntaxKind::IfKeyword => {
                self.scan_if_block(&token);
            }
            SyntaxKind::WhileKeyword => {
                self.builder.start_node(to_raw(SyntaxKind::WhileLoop));
                self.builder.token(to_raw(SyntaxKind::WhileKeyword), text);
                self.eat_whitespace();
                self.builder.start_node(to_raw(SyntaxKind::Condition));
                let is_expression = self.scan_expression() != ExpressionKind::None;
                self.builder.finish_node();
                if is_expression {
                    self.eat_whitespace();
                    if let Some(t) = self.peek_raw_token() {
                        if t.kind == TokenKind::Identifier {
                            let text = &self.text[t.start..t.end];
                            let keyword_kind = str_to_keyword(text);
                            if keyword_kind == SyntaxKind::DoKeyword {
                                self.next_raw_token();
                                self.builder.token(to_raw(SyntaxKind::DoKeyword), &self.text[t.start..t.end]);
                                self.scan_block(Some(SyntaxKind::EndKeyword), &token);
                            }
                        }
                    }
                } else {

                }
                self.builder.finish_node();
            }
            SyntaxKind::RepeatKeyword => {
                self.builder.start_node(to_raw(SyntaxKind::RepeatUntilLoop));
                self.builder.token(to_raw(SyntaxKind::RepeatKeyword), text);
                self.eat_whitespace();
                self.scan_block(Some(SyntaxKind::UntilKeyword), &token);
                self.eat_whitespace();
                if self.scan_expression() == ExpressionKind::None {
                    let end = self.get_current_position();
                    self.errors.push(Error {start: token.start, end, kind: ErrorKind::ExpectingExpression });
                }
                self.builder.finish_node();
            }
            SyntaxKind::ForKeyword => {
                let checkpoint = self.builder.checkpoint();
                self.builder.token(to_raw(SyntaxKind::ForKeyword), text);
                self.eat_whitespace();
                if let Some(start_token) = self.peek_raw_token() {
                    let text = &self.text[start_token.start..start_token.end];
                    if start_token.kind != TokenKind::Identifier {
                        self.errors.push(Error { start: start_token.start, end: start_token.end, kind: ErrorKind::ExpectingName });
                    } else if str_to_keyword(text) != SyntaxKind::Name {
                        self.errors.push(Error { start: start_token.start, end: start_token.end, kind: ErrorKind::UnexpectedKeyword });
                    } else {
                        self.next_raw_token();
                        self.eat_whitespace();
                        if let Some(t) = self.peek_raw_token() {
                            if t.kind == TokenKind::Assign {
                                self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::ForCountLoop));
                                self.builder.token(to_raw(SyntaxKind::Name), text);
                                let text  =&self.text[t.start..t.end];
                                self.builder.token(to_raw(SyntaxKind::Assign), text);
                                self.next_raw_token();
                                self.eat_whitespace();
                                self.builder.start_node(to_raw(SyntaxKind::ExpressionList));
                                self.scan_expression_list();
                                self.builder.finish_node();
                            } else {
                                self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::ForInLoop));
                                self.scan_name_list(&start_token, text);
                                self.eat_whitespace();
                                if let Some(t) = self.peek_raw_token() {
                                    let text  =&self.text[t.start..t.end];
                                    if t.kind != TokenKind::Identifier || str_to_keyword(text) != SyntaxKind::InKeyword {
                                        self.errors.push(Error { start: start_token.start, end: start_token.end, kind: ErrorKind::ExpectingToken });
                                    } else {
                                        self.builder.token(to_raw(SyntaxKind::InKeyword), text);
                                        self.next_raw_token();
                                        self.builder.start_node(to_raw(SyntaxKind::ExpressionList));
                                        self.scan_expression_list();
                                        self.builder.finish_node();
                                    }
                                }
                            }
                            self.eat_whitespace();
                            if let Some(t) = self.peek_raw_token() {
                                let text  =&self.text[t.start..t.end];
                                if t.kind != TokenKind::Identifier || str_to_keyword(text) != SyntaxKind::DoKeyword {
                                    self.errors.push(Error { start: start_token.start, end: start_token.end, kind: ErrorKind::ExpectingDo });
                                } else {
                                    self.next_raw_token();
                                    self.builder.token(to_raw(SyntaxKind::DoKeyword), text);
                                    self.eat_whitespace();
                                    self.scan_block(Some(SyntaxKind::EndKeyword), &token);
                                }
                            }
                            self.builder.finish_node();
                        }
                    }
                }
            }
            SyntaxKind::LocalKeyword => {
                let checkpoint = self.builder.checkpoint();
                let keyword_token = token;
                self.builder.token(to_raw(SyntaxKind::LocalKeyword), text);
                self.eat_whitespace();
                if let Some(t) = self.next_raw_token() {
                    let text = &self.text[t.start..t.end];
                    if t.kind != TokenKind::Identifier {
                        self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                        return;
                    } else {
                        let keyword = str_to_keyword(text);
                        if keyword == SyntaxKind::FunctionKeyword {
                            self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::FunctionDefinition));
                            self.builder.token(to_raw(SyntaxKind::FunctionKeyword), text);
                            self.eat_whitespace();
                            if let Some(t) = self.peek_raw_token() {
                                if t.kind == TokenKind::Identifier {
                                    self.next_raw_token();
                                    let text = &self.text[t.start..t.end];
                                    let keyword = str_to_keyword(text);
                                    if keyword != SyntaxKind::Name {
                                        self.builder.token(to_raw(keyword), text);
                                    } else {
                                        self.builder.token(to_raw(SyntaxKind::Name), text);
                                    }
                                    if self.scan_parameters() {
                                        self.scan_block(Some(SyntaxKind::EndKeyword), &keyword_token);
                                    }
                                }
                            }
                            self.builder.finish_node();
                        } else if keyword != SyntaxKind::Name {
                            self.builder.token(to_raw(keyword), text);
                            self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::UnexpectedKeyword });
                        } else {
                            self.scan_name_list(&t, &text);
                            self.eat_whitespace();
                            if let Some(t) = self.peek_raw_token() {
                                if t.kind == TokenKind::Assign {
                                    self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::LocalAssignStatement));
                                    self.builder.token(to_raw(SyntaxKind::Assign), &self.text[t.start..t.end]);
                                    self.next_raw_token();
                                    self.builder.start_node(to_raw(SyntaxKind::ExpressionList));
                                    self.scan_expression_list();
                                    self.builder.finish_node();
                                    self.builder.finish_node();
                                }
                            }
                        }
                    }
                }
            }
            SyntaxKind::ReturnKeyword => {
                self.builder.start_node(to_raw(SyntaxKind::ReturnStatement));
                self.builder.token(to_raw(SyntaxKind::ReturnKeyword), &self.text[token.start..token.end]);
                self.eat_whitespace();
                if let Some(t) = self.peek_raw_token() {
                    if t.kind != TokenKind::Identifier || str_to_keyword(&self.text[t.start..t.end]) != SyntaxKind::EndKeyword {
                        self.scan_expression_list();
                    }
                }
                self.builder.finish_node();
            }
            SyntaxKind::Name => { // variable name
                self.scan_preexp_statement(token, text);
            }
            _ => {
                self.builder.token(to_raw(SyntaxKind::Invalid), text);
                self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedKeyword});
            },
        }
    }

    fn scan_preexp_statement(&mut self, token: &Token, text: &str) {
        let origin = self.builder.checkpoint();
        let mut checkpoint = self.builder.checkpoint();
        let mut kind = self.scan_preexp(token, text);
        let mut needs_indexing = false;
        if kind == ExpressionKind::Nested {
            needs_indexing = true;
        }
        let mut started_group = false;
        let mut expecting_name = false;
        let mut start = token.start;
        while kind != ExpressionKind::None {
            self.eat_whitespace();
            if let Some(t) = self.peek_raw_token() {
                match t.kind {
                    TokenKind::Identifier => break,
                    TokenKind::Dot => {
                        self.next_raw_token();
                        let text = &self.text[t.start..t.end];
                        if !started_group {
                            self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Identifier));
                            started_group = true;
                        }
                        self.builder.token(to_raw(SyntaxKind::Dot), text);
                        if let Some(t) = self.peek_raw_token() {
                            let text = &self.text[t.start..t.end];
                            kind = self.scan_preexp(&t, text);
                        } else {
                            break
                        }
                    }
                    TokenKind::Colon => {
                        self.next_raw_token();
                        let text = &self.text[t.start..t.end];
                        if !started_group {
                            self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Identifier));
                            started_group = true;
                        }
                        self.builder.token(to_raw(SyntaxKind::Colon), text);
                        self.eat_whitespace();
                        if let Some(t) = self.peek_raw_token() {
                            let text = &self.text[t.start..t.end];
                            if t.kind != TokenKind::Identifier {
                                self.errors.push(Error { start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                                break
                            } else if str_to_keyword(text) != SyntaxKind::Name {
                                self.errors.push(Error { start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                            }
                            self.builder.token(to_raw(SyntaxKind::Name), text);
                            self.next_raw_token();
                            if !self.scan_arguments() {
                                self.errors.push(Error { start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                                break
                            }
                        }
                    }
                    TokenKind::Comma => {
                        if started_group {
                            self.builder.finish_node();
                            started_group = false;
                        }
                        start = t.start;
                        self.next_raw_token();
                        self.builder.token(to_raw(SyntaxKind::Comma), &self.text[t.start..t.end]);
                        self.eat_whitespace();
                        if kind != ExpressionKind::Name && kind != ExpressionKind::Identifier {
                            self.errors.push(Error { start, end: t.end, kind: ErrorKind::ExpectingName });
                            break
                        }
                        expecting_name = true;
                        if let Some(t) = self.peek_raw_token() {
                            checkpoint = self.builder.checkpoint();
                            let text = &self.text[t.start..t.end];
                            kind = self.scan_preexp(&t, text);
                        } else {
                            self.errors.push(Error { start, end: t.end, kind: ErrorKind::ExpectingName });
                            break
                        }
                    }
                    TokenKind::LeftSquareBracket => {
                        self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Identifier));
                        self.next_raw_token();
                        self.scan_indexing_variable(&t);
                        self.builder.finish_node();
                        kind = ExpressionKind::Identifier;
                    }
                    TokenKind::LeftBracket | TokenKind::String{validity: _, modifier: _} | TokenKind::LeftCurlyBracket => {
                        self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::FunctionCall));
                        let scanned = self.scan_arguments();
                        self.builder.finish_node();
                        if !scanned {
                            self.errors.push(Error { start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                            break;
                        }
                    }
                    _ => break,
                }
                needs_indexing = false;
            } else {
                break
            }
        }
        if started_group {
            self.builder.finish_node();
        }
        if expecting_name && kind != ExpressionKind::Name || needs_indexing {
            let end = self.get_current_position();
            self.errors.push(Error { start, end, kind: ErrorKind::ExpectingName });
        }
        if kind == ExpressionKind::Name || kind == ExpressionKind::Identifier {
            self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::VariableList));
            self.builder.finish_node();
            self.eat_whitespace();
            if let Some(t) = self.peek_raw_token() {
                if t.kind == TokenKind::Assign {
                    self.builder.start_node_at(origin, to_raw(SyntaxKind::AssignStatement));
                    self.builder.token(to_raw(SyntaxKind::Assign), &self.text[t.start..t.end]);
                    self.next_raw_token();
                    self.builder.start_node(to_raw(SyntaxKind::ExpressionList));
                    self.scan_expression_list();
                    self.builder.finish_node();
                    self.builder.finish_node();
                }
            }
        }

    }

    fn scan_indexing_variable(&mut self, token: &Token) {
        self.builder.token(to_raw(SyntaxKind::LeftSquareBracket), &self.text[token.start..token.end]);
        self.builder.start_node(to_raw(SyntaxKind::Expression));
        self.eat_whitespace();
        let expression_kind = self.scan_expression();
        self.builder.finish_node();
        if expression_kind != ExpressionKind::None {
            self.eat_whitespace();
            if let Some(t) = self.peek_raw_token() {
                if t.kind == TokenKind::RightSquareBracket {
                    let text = &self.text[t.start..t.end];
                    self.builder.token(to_raw(SyntaxKind::RightSquareBracket), text);
                    self.next_raw_token();
                }
            }
        }
    }

    fn scan_block(&mut self, terminator: Option<SyntaxKind>, start_token: &Token) {
        self.builder.start_node(to_raw(SyntaxKind::Block));

        self.eat_whitespace();

        let mut t;
        if let Some(token) = self.next_raw_token() {
            t = token;
        } else {
            self.builder.finish_node();
            if let Some(_) = terminator {
                self.errors.push(Error{ start: start_token.start, end: start_token.end, kind: ErrorKind::NotClosedBlock });
            }
            return
        }
        let mut terminated = false;
        loop {
            let text = &self.text[t.start .. t.end];
            match t.kind {
                TokenKind::Identifier | TokenKind::LeftBracket =>  {
                    let keyword = str_to_keyword(text);
                    if Some(keyword) == terminator {
                        terminated = true;
                        break
                    }
                    self.scan_statement(&t, text);
                },
                TokenKind::Semicolon => {
                    self.builder.token(to_raw(SyntaxKind::Semicolon), text)
                }
                _ => {
                    self.builder.token(to_raw(SyntaxKind::Invalid), text);
                    self.errors.push(Error { start: t.start, end: t.end, kind: ErrorKind::UnexpectedToken});
                }
            }
            self.eat_whitespace();

            if let Some(token) = self.next_raw_token() {
                t = token;
            } else {
                if let Some(_) = terminator {
                    self.errors.push(Error{ start: start_token.start, end: start_token.end, kind: ErrorKind::NotClosedBlock });
                    break
                }
                break
            }
        }

        self.builder.finish_node();

        if terminated {
            let text = &self.text[t.start .. t.end];
            self.builder.token(to_raw(str_to_keyword(text)), text);
        }
    }

    fn scan_if_block(&mut self, start_token: &Token) {
        self.builder.start_node(to_raw(SyntaxKind::IfChain)); //IfChain

        self.builder.start_node(to_raw(SyntaxKind::IfBranch)); //IfBranch

        self.builder.token(to_raw(SyntaxKind::IfKeyword), &self.text[start_token.start..start_token.end]);

        self.eat_whitespace();

        self.builder.start_node(to_raw(SyntaxKind::Condition));
        if self.scan_expression() == ExpressionKind::None {
            let end = self.get_current_position();
            self.errors.push(Error{ start: start_token.end, end, kind: ErrorKind::ExpectingExpression});
        }
        self.builder.finish_node();

        self.eat_whitespace();

        let mut t;
        if let Some(token) = self.peek_raw_token() {
            t = token;
            if t.kind != TokenKind::Identifier || str_to_keyword(&self.text[t.start..t.end]) != SyntaxKind::ThenKeyword {
                let end =  self.get_current_position();
                self.errors.push(Error{ start: start_token.start, end, kind: ErrorKind::ExpectingThen });
                self.builder.finish_node(); //IfBranch
                self.builder.finish_node(); //IfChain
                return
            } else {
                self.next_raw_token();
                self.builder.token(to_raw(SyntaxKind::ThenKeyword), &self.text[t.start..t.end]);
                self.eat_whitespace();
                if let Some(token) = self.next_raw_token() {
                    t = token;
                } else {
                    self.builder.finish_node(); //IfBranch
                    self.builder.finish_node(); //IfChain
                    self.errors.push(Error{ start: start_token.start, end: start_token.end, kind: ErrorKind::NotClosedBlock });
                    return
                }
            }
        } else {
            self.builder.finish_node(); //IfBranch
            self.builder.finish_node(); //IfChain
            self.errors.push(Error{ start: start_token.start, end: start_token.end, kind: ErrorKind::NotClosedBlock });
            return
        }

        self.builder.start_node(to_raw(SyntaxKind::Block)); //Block

        let mut terminated = false;
        let mut seen_else = false;
        loop {
            let text = &self.text[t.start .. t.end];
            match t.kind {
                TokenKind::Identifier =>  {
                    let keyword = str_to_keyword(text);
                    if !seen_else {
                        match keyword {
                            SyntaxKind::ElseIfKeyword => {
                                self.builder.finish_node(); //Block
                                self.builder.finish_node(); //IfBranch
                                self.builder.start_node(to_raw(SyntaxKind::IfBranch)); //IfBranch
                                self.builder.token(to_raw(SyntaxKind::ElseIfKeyword), &self.text[t.start..t.end]);
                                self.eat_whitespace();
                                self.builder.start_node(to_raw(SyntaxKind::Condition));
                                if self.scan_expression() == ExpressionKind::None {
                                    let end = self.get_current_position();
                                    self.errors.push(Error{ start: t.end, end, kind: ErrorKind::ExpectingExpression});
                                }
                                self.builder.finish_node();
                                if let Some(token) = self.next_raw_token() {
                                    t = token;
                                    self.eat_whitespace();
                                    if t.kind != TokenKind::Identifier || str_to_keyword(&self.text[t.start..t.end]) != SyntaxKind::ThenKeyword {
                                        self.errors.push(Error{ start: t.start, end: self.text.len(), kind: ErrorKind::ExpectingThen });
                                    } else {
                                        self.builder.token(to_raw(SyntaxKind::ThenKeyword), &self.text[t.start..t.end]);
                                    }
                                }
                                self.builder.start_node(to_raw(SyntaxKind::Block)); //IfBranch
                            }
                            SyntaxKind::ElseKeyword => {
                                self.builder.finish_node(); //Block
                                self.builder.finish_node(); //IfBranch
                                self.builder.start_node(to_raw(SyntaxKind::ElseBranch));
                                self.builder.token(to_raw(SyntaxKind::ElseKeyword), &self.text[t.start..t.end]);
                                self.builder.start_node(to_raw(SyntaxKind::Block));
                                seen_else = true
                            }
                            SyntaxKind::EndKeyword => {
                                self.builder.finish_node(); //Block
                                let text = &self.text[t.start .. t.end];
                                self.builder.token(to_raw(SyntaxKind::EndKeyword), text);
                                self.builder.finish_node(); //IfBranch
                                self.builder.finish_node(); //IfChain
                                terminated = true;
                                break;
                            }
                            _ => self.scan_statement(&t, text),
                        }
                    } else {
                        match keyword {
                            SyntaxKind::EndKeyword => {
                                self.builder.finish_node(); //Block
                                let text = &self.text[t.start .. t.end];
                                self.builder.token(to_raw(SyntaxKind::EndKeyword), text);
                                self.builder.finish_node(); //IfBranch
                                self.builder.finish_node(); //IfChain
                                terminated = true;
                                break;
                            }
                            _ => self.scan_statement(&t, text),
                        }
                    }
                },
                TokenKind::Semicolon => {
                    self.builder.token(to_raw(SyntaxKind::Semicolon), text)
                },
                TokenKind::LeftBracket => {
                    self.scan_statement(&t, text);
                }
                _ => {
                    self.builder.token(to_raw(SyntaxKind::Invalid), text);
                    self.errors.push(Error { start: t.start, end: t.end, kind: ErrorKind::UnexpectedToken});
                }
            }
            self.eat_whitespace();

            if let Some(token) = self.next_raw_token() {
                t = token;
            } else {
                self.errors.push(Error{ start: start_token.start, end: start_token.end, kind: ErrorKind::NotClosedBlock });
                break
            }
        }

        if !terminated {
            self.builder.finish_node();
            self.builder.finish_node();
            self.builder.finish_node();
        }
    }

    fn scan_arguments(&mut self) -> bool {
        if let Some(t) = self.peek_raw_token() {
            match t.kind {
                TokenKind::LeftBracket | TokenKind::Number{validity: _, modifier: _} | TokenKind::String{validity: _, modifier: _} | TokenKind::LeftCurlyBracket => (),
                _ => return false,
            }
            let expecting_closing_bracket = t.kind == TokenKind::LeftBracket;
            if expecting_closing_bracket {
                self.next_raw_token();
            }
            self.builder.start_node(to_raw(SyntaxKind::ArgumentList));
            self.builder.token(to_raw(SyntaxKind::LeftBracket), &self.text[t.start..t.end]);
            self.scan_expression_list();
            self.eat_whitespace();
            let mut is_closed = false;
            if let Some(t) = self.peek_raw_token() {
                if t.kind == TokenKind::RightBracket && expecting_closing_bracket {
                    is_closed = true;
                    self.next_raw_token();
                    self.builder.token(to_raw(SyntaxKind::RightBracket), &self.text[t.start..t.end]);
                }
            }
            if !is_closed && expecting_closing_bracket {
                self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::ExpectingClosingBracket });
            }
            self.builder.finish_node();
        }
        return true;
    }

    fn scan_parameters(&mut self) -> bool {
        self.eat_whitespace();
        self.builder.start_node(to_raw(SyntaxKind::ParameterList));
        let mut result = false;
        if let Some(token) = self.peek_raw_token() {
            match token.kind {
                TokenKind::LeftBracket => {
                    self.builder.token(to_raw(SyntaxKind::LeftBracket), &self.text[token.start..token.end]);
                    self.next_raw_token();
                    let mut expecting_closure = false;
                    let mut expecting_terminator = false;
                    let mut seen_parameter = false;
                    self.eat_whitespace();
                    let mut comma_point = self.get_current_position();
                    while let Some(token) = self.peek_raw_token()  {
                        let text = &self.text[token.start..token.end];
                        match token.kind {
                            TokenKind::Identifier => {
                                if expecting_terminator {
                                    self.errors.push(Error { start: comma_point, end: comma_point + 1, kind: ErrorKind::ExpectingClosingBracket });
                                    break;
                                }
                                if expecting_closure && seen_parameter {
                                    self.errors.push(Error { start: comma_point, end: comma_point + 1, kind: ErrorKind::ExpectingCommaOrBracket });
                                    break;
                                }
                                comma_point = token.end;
                                seen_parameter = true;
                                let keyword_type= str_to_keyword(text);
                                if keyword_type != SyntaxKind::Name {
                                    self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedKeyword });
                                    self.builder.token(to_raw(keyword_type), text);
                                } else {
                                    self.builder.token(to_raw(SyntaxKind::Parameter), text);
                                }
                                expecting_closure = true;
                                self.next_raw_token();
                            },
                            TokenKind::Comma => {
                                if expecting_terminator {
                                    self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::ExpectingClosingBracket });
                                }
                                if !expecting_closure {
                                    self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedOperator });
                                }
                                expecting_closure = false;
                                self.builder.token(to_raw(SyntaxKind::Comma), text);
                                self.next_raw_token();
                            }
                            TokenKind::RightBracket => {
                                if !expecting_closure && !expecting_terminator && seen_parameter {
                                    self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::ExpectingName });
                                }
                                self.builder.token(to_raw(SyntaxKind::RightBracket), text);
                                self.next_raw_token();
                                result = true;
                                break;
                            }
                            TokenKind::TripleDot => {
                                if expecting_closure && seen_parameter {
                                    self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedOperator });
                                }
                                self.builder.token(to_raw(SyntaxKind::ParameterVarArgs), text);
                                expecting_terminator = true;
                                self.next_raw_token();
                            }
                            _ => {
                                self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedOperator });
                                break;
                            }
                        };
                        self.eat_whitespace();
                    }
                },
                _ => ()
            }
        }
        self.builder.finish_node();
        self.eat_whitespace();
        return result
    }

    fn scan_name_list(&mut self, _token: &Token, text: &str) {
        self.builder.start_node(to_raw(SyntaxKind::NameList));
        let mut expecting_closure = true;
        self.builder.token(to_raw(SyntaxKind::Name), text);
        self.eat_whitespace();
        while let Some(token) = self.peek_raw_token()  {
            let text = &self.text[token.start..token.end];
            match token.kind {
                TokenKind::Identifier => {
                    if expecting_closure {
                        break;
                    }
                    let keyword_type= str_to_keyword(text);
                    if keyword_type != SyntaxKind::Name {
                        self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedKeyword });
                        self.builder.token(to_raw(keyword_type), text);
                    } else {
                        self.builder.token(to_raw(SyntaxKind::Name), text);
                    }
                    expecting_closure = true;
                    self.next_raw_token();
                },
                TokenKind::Comma => {
                    if !expecting_closure {
                        self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedOperator });
                    }
                    expecting_closure = false;
                    self.builder.token(to_raw(SyntaxKind::Comma), text);
                    self.next_raw_token();
                }
                _ => {
                    break;
                }
            };
            self.eat_whitespace();
        }
        self.builder.finish_node();
    }

    fn scan_field_list(&mut self) {
        let mut assign_expected = false;
        let mut comma_expected = false;
        let mut field_open = false;
        self.eat_whitespace();
        while let Some(t) = self.peek_raw_token() {
            if !comma_expected {
                let checkpoint = self.builder.checkpoint();
                let kind = self.scan_expression();
                if kind != ExpressionKind::None {
                    if kind != ExpressionKind::Name {
                        self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Expression));
                        self.builder.finish_node();
                    } else {
                        assign_expected = true;
                    }
                    self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::Field));
                    field_open = true;
                    comma_expected = true;
                    continue;
                }
            }
            let text = &self.text[t.start..t.end];
            match t.kind {
                TokenKind::LeftSquareBracket => {
                    if comma_expected {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::ExpectingComma });
                        break;
                    }
                    self.builder.start_node(to_raw(SyntaxKind::Field));
                    field_open = true;
                    self.next_raw_token();
                    self.eat_whitespace();
                    self.builder.token(to_raw(SyntaxKind::LeftSquareBracket), text);
                    if self.scan_expression() == ExpressionKind::None {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::ExpectingExpression });
                        break
                    }
                    self.eat_whitespace();
                    if let Some(t) = self.peek_raw_token() {
                        if t.kind == TokenKind::RightSquareBracket {
                            let text = &self.text[t.start..t.end];
                            self.next_raw_token();
                            self.builder.token(to_raw(SyntaxKind::RightSquareBracket), text)
                        } else {
                            self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::ExpectingClosingBracket });
                        }
                    }

                    assign_expected = true;
                    comma_expected = true;
                }
                TokenKind::Comma | TokenKind::Semicolon => {
                    if field_open {
                        field_open = false;
                        self.builder.finish_node();
                    }
                    if !comma_expected {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                        break
                    }
                    assign_expected = false;
                    comma_expected = false;
                    self.next_raw_token();
                    if t.kind == TokenKind::Comma {
                        self.builder.token(to_raw(SyntaxKind::Comma), text);
                    } else {
                        self.builder.token(to_raw(SyntaxKind::Semicolon), text);
                    }
                }
                TokenKind::Assign => {
                    if !assign_expected {
                        self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                        break
                    }
                    self.next_raw_token();
                    self.builder.token(to_raw(SyntaxKind::Assign), text);
                    self.eat_whitespace();
                    self.builder.start_node(to_raw(SyntaxKind::Expression));
                    self.scan_expression();
                    self.builder.finish_node();
                    comma_expected = true;
                    assign_expected = false;
                }
                _ => {
                    break
                }
            }
            self.eat_whitespace();
        }
        if field_open {
            self.builder.finish_node();
        }
    }
}
