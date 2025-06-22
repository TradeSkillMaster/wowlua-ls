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

use crate::lexer::LuaLexer;
use crate::lexer::Token;
use crate::lexer::TokenKind;
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
    ForLoop,
    Statement,
    Assignment,
    ReturnStatement,
    VarList,
    VariableName,
    ExpList,
    Expression,
    GroupedExpression,
    PrefixExpression,
    ArgumentList,
    ParameterList,
    Parameter,
    ParameterVarArgs,
    TableConstructor,
    FieldList,

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
    NotClosedString,
    UnexpectedKeyword,
    UnexpectedToken,
    UnexpectedOperator,
    ExpectingComma,
    ExpectingName,
    ExpectingClosingBracket,
    ExpectingFunctionCall,
    UnexpectedParameter,
    InvalidName,
    InvalidVariableName,
    InvalidFunction,
}
#[derive(Debug, Clone, Copy)]
pub struct Error {
    start: usize,
    end: usize,
    kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum PreExpressionKind {
    FunctionCall,
    Name,
    Nested,
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
    lexer: LuaLexer<'a>,
    builder: GreenNodeBuilder<'a>,
    errors: Vec<Error>,
    token_cache: Option<Token>,
}

impl<'a> Generator<'a> {
    pub fn new(text: &'a str) -> Generator<'a> {
        Generator {
            text: text,
            lexer: LuaLexer::new(text),
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            token_cache: None,
        }
    }

    pub fn errors(&self) -> &Vec<Error> {
        &self.errors
    }

    fn next_raw_token(&mut self) -> Option<Token> {
        if let Some(t) = self.token_cache {
            self.token_cache = None;
            return Some(t)
        } else {
            return self.lexer.next_token()
        }
    }

    fn peek_raw_token(&mut self) -> Option<Token> {
        if let Some(t) = self.token_cache {
            return Some(t)
        } else {
            self.token_cache = self.lexer.next_token();
            return self.token_cache
        }
    }

    pub fn process_all(&mut self) -> rowan::GreenNode {
        let t = self.next_raw_token();
        
        if let Some(t) = t {
            self.scan_block(None, Some(&t), 0);
        } else {
            self.builder.token(to_raw(SyntaxKind::EoF), "")
        }
        let b = std::mem::take(&mut self.builder);
        return b.finish()
    }

    #[inline]
    fn eat_whitespace(&mut self) {
        loop {
            if let Some(token) = self.peek_raw_token() {
                match token.kind {
                    TokenKind::Whitespace => {
                        self.builder.token(to_raw(SyntaxKind::Whitespace), &self.text[token.start..token.end])
                    },
                    TokenKind::Newline => { 
                        self.builder.token(to_raw(SyntaxKind::Newline), &self.text[token.start..token.end])
                    },
                    TokenKind::Comment { validity: v, modifier: _ } => {
                        let text = &self.text[token.start .. token.end];
                        if v == crate::lexer::token_validity::Comment::NotTerminated {
                            self.errors.push(Error{ start: token.start, end: self.text.len(), kind: ErrorKind::NotClosedComment });
                        }
                        self.builder.token(to_raw(SyntaxKind::Comment), text)
                    },
                    _ => return,
                };
                self.next_raw_token();
            } else {
                break;
            }
        }
    }

    fn scan_function_identifier(&mut self, token: &Token, _text: &str) -> (bool, bool) {
        self.builder.start_node(to_raw(SyntaxKind::Identifier));
        self.eat_whitespace();
        let mut t = *token;
        let mut skip_forward = false;
        let mut id_expected = true;
        let mut terminate_next = false;
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
                    self.eat_whitespace();
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
                    self.eat_whitespace();
                }
                TokenKind::Colon => {
                    if id_expected {
                        self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::InvalidName });
                        break
                    }
                    terminate_next = true;
                    id_expected = true;
                    self.builder.token(to_raw(SyntaxKind::Colon), text);
                    if skip_forward {
                        self.next_raw_token();
                    }
                    self.eat_whitespace();
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
        }
        self.builder.finish_node();

        return (terminate_next, !id_expected)
    }

    //TODO: Correct literals
    fn scan_expression_part(&mut self) -> bool {
        if let Some(t) = self.peek_raw_token() {
            match t.kind {
                TokenKind::Number{validity, modifier} => {
                    //XXX: Need to handle validity
                    self.next_raw_token();
                    self.builder.token(to_raw(SyntaxKind::Number), &self.text[t.start..t.end]);
                },
                TokenKind::String{validity, modifier} => {
                    //XXX: Need to handle validity
                    self.next_raw_token();
                    self.builder.token(to_raw(SyntaxKind::String), &self.text[t.start..t.end]);
                },
                TokenKind::TripleDot => {
                    self.next_raw_token();
                    self.builder.token(to_raw(SyntaxKind::Number), &self.text[t.start..t.end]);
                },
                TokenKind::Identifier => {
                    let text = &self.text[t.start..t.end];
                    let keyword_kind = str_to_keyword(text);
                    self.next_raw_token();
                    match keyword_kind {
                        SyntaxKind::Name => {
                            self.scan_preexp(&t, text);
                        },
                        SyntaxKind::NilKeyword | SyntaxKind::FalseKeyword | SyntaxKind::TrueKeyword => {
                            self.builder.token(to_raw(keyword_kind), text)
                        }
                        _ => {
                            self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedKeyword });
                            self.builder.token(to_raw(keyword_kind), text)
                        }
                    }
                },
                //TODO: table constructors, unary operators etc.
                _ => return false,
            }
        }
        return true
    }

    fn scan_expression(&mut self) -> bool {
        return self.scan_expression_part();
        //TODO Process combined expressions
    }

    fn get_current_position(&mut self) -> usize {
        let mut end = self.text.len();
        if let Some(t) = self.peek_raw_token() {
            end = t.start;
        }
        return end
    }

    fn scan_preexp(&mut self, token: &Token, text: &str) -> (bool, PreExpressionKind) {
        if token.kind == TokenKind::LeftBracket {
            return (self.scan_expression(), PreExpressionKind::Nested)
        }
        let checkpoint = self.builder.checkpoint();
        let (is_function_call, seen_name) = self.scan_function_identifier(token, text);
        self.eat_whitespace();
        if is_function_call {
            self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::FunctionCall));
            let scanned = self.scan_arguments();
            self.builder.finish_node();
            if !scanned {
                let end = self.get_current_position();
                self.errors.push(Error{ start: token.start, end, kind: ErrorKind::ExpectingFunctionCall });
            }
            return (scanned, PreExpressionKind::FunctionCall)
        } else if seen_name {
            if let Some(t) = self.peek_raw_token() {
                match t.kind {
                    TokenKind::LeftBracket => {
                        self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::FunctionCall));
                        let scanned = self.scan_arguments();
                        self.builder.finish_node();
                        if !scanned {
                            self.errors.push(Error{ start: token.start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                        }
                        return (scanned, PreExpressionKind::FunctionCall)
                    },
                    _ => (),
                }
            }
            return (true, PreExpressionKind::Name)
        } else {
            self.errors.push(Error{ start: token.start, end: token.end, kind: ErrorKind::ExpectingName });
            return (false, PreExpressionKind::None)
        }
    }

    fn scan_statement_from_identifier(&mut self, token: &Token, text: &str) {
        match str_to_keyword(text) {
            SyntaxKind::DoKeyword => {
                self.builder.start_node(to_raw(SyntaxKind::DoBlock));
                self.builder.token(to_raw(SyntaxKind::DoKeyword), text);
                self.scan_block(Some(SyntaxKind::EndKeyword), None, token.start);
                self.builder.finish_node();
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
                                self.scan_block(Some(SyntaxKind::EndKeyword), None,keyword_token.start);
                            }
                        }
                        TokenKind::LeftBracket => {
                            if self.scan_parameters() {
                                self.scan_block(Some(SyntaxKind::EndKeyword), None, keyword_token.start);
                            }
                        }
                        _ => {
                            self.errors.push(Error { start: keyword_token.start, end: token.end, kind: ErrorKind::InvalidFunction});
                        }
                    }
                }
                self.builder.finish_node();
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
                                        self.scan_block(Some(SyntaxKind::EndKeyword), None, keyword_token.start);
                                    }
                                }
                            }
                            self.builder.finish_node();
                        } else if keyword != SyntaxKind::Name {
                            self.builder.token(to_raw(keyword), text);
                            self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::UnexpectedKeyword });
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
                        self.scan_expression();
                    }
                }
                self.builder.finish_node();
            }
            SyntaxKind::Name => { // variable name
                let mut checkpoint = self.builder.checkpoint();
                let (mut scanned, mut kind) = self.scan_preexp(token, text);
                let mut started_group = false;
                while scanned && kind != PreExpressionKind::Name {
                    self.eat_whitespace();
                    if let Some(t) = self.peek_raw_token() {
                        match t.kind {
                            TokenKind::Identifier => break,
                            TokenKind::Dot => {
                                self.next_raw_token();
                                let text = &self.text[t.start..t.end];
                                if !started_group {
                                    self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::GroupedExpression));
                                    started_group = true;
                                }
                                self.builder.token(to_raw(SyntaxKind::Dot), text);
                                checkpoint = self.builder.checkpoint();
                                if let Some(t) = self.peek_raw_token() {
                                    let text = &self.text[t.start..t.end];
                                    (scanned, kind) = self.scan_preexp(&t, text);
                                } else {
                                    break
                                }
                            }
                            TokenKind::Colon => {
                                self.next_raw_token();
                                let text = &self.text[t.start..t.end];
                                if !started_group {
                                    self.builder.start_node_at(checkpoint, to_raw(SyntaxKind::GroupedExpression));
                                    started_group = true;
                                }
                                self.builder.token(to_raw(SyntaxKind::Colon), text);
                                if let Some(t) = self.peek_raw_token() {
                                    let text = &self.text[t.start..t.end];
                                    if t.kind != TokenKind::Identifier {
                                        self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                                        break
                                    } else if str_to_keyword(text) != SyntaxKind::Name {
                                        self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                                    }
                                    self.next_raw_token();
                                    if !self.scan_arguments() {
                                        self.errors.push(Error { start: token.start, end: t.end, kind: ErrorKind::ExpectingFunctionCall });
                                        break
                                    }
                                }
                            }
                            _ => break,
                        }
                    } else {
                        break
                    }
                }
                if started_group {
                    self.builder.finish_node();
                }
            }
            _ => {
                self.builder.token(to_raw(SyntaxKind::Invalid), text);
                self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedKeyword});
            },
        }
    }

    fn scan_block(&mut self, terminator: Option<SyntaxKind>, starting_token: Option<&Token>, start_position: usize) {
        self.builder.start_node(to_raw(SyntaxKind::Block));

        let mut t;
        if let Some(token) = starting_token {
            t = *token;
        } else if let Some(token) = self.next_raw_token() {
            t = token;
        } else {
            if let Some(_) = terminator {
                self.errors.push(Error{ start: start_position, end: self.text.len(), kind: ErrorKind::NotClosedBlock });
            }
            self.builder.finish_node();
            return
        }
        let mut terminated = false;
        loop {
            let text = &self.text[t.start .. t.end];
            match t.kind {
                TokenKind::Identifier =>  {
                    let keyword = str_to_keyword(text);
                    if Some(keyword) == terminator {
                        terminated = true;
                        break
                    }
                    self.scan_statement_from_identifier(&t, text);
                },
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
                    self.errors.push(Error{ start: start_position, end: self.text.len(), kind: ErrorKind::NotClosedBlock });
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

    fn scan_arguments(&mut self) -> bool {
        if let Some(t) = self.peek_raw_token() {
            if t.kind != TokenKind::LeftBracket {
                return false
            }
            self.next_raw_token();
            self.builder.start_node(to_raw(SyntaxKind::ArgumentList));
            self.builder.token(to_raw(SyntaxKind::LeftBracket), &self.text[t.start..t.end]);
            let mut seen_comma = false;
            let mut seen_expression = false;
            loop {
                if seen_comma || !seen_expression {
                    let scanned =  self.scan_expression();
                    if !scanned && !seen_comma {
                        break
                    } else if scanned {
                        seen_expression = true;
                    } else {
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
                            self.errors.push(Error{ start: t.start, end: t.end, kind: ErrorKind::UnexpectedOperator });
                            break
                        }
                    }
                } else {
                    break
                }
            }
            let mut is_closed = false;
            if let Some(t) = self.peek_raw_token() {
                if t.kind == TokenKind::RightBracket {
                    is_closed = true;
                    self.next_raw_token();
                    self.builder.token(to_raw(SyntaxKind::RightBracket), &self.text[t.start..t.end]);
                }
            }
            if !is_closed {
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
                    loop {
                        self.eat_whitespace();
                        if let Some(token) = self.peek_raw_token() {
                            let text = &self.text[token.start..token.end];
                            match token.kind {
                                TokenKind::Identifier => {
                                    if expecting_closure && seen_parameter {
                                        self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::ExpectingComma });
                                    }
                                    if expecting_terminator {
                                        self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedParameter });
                                    }
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
                                    if !expecting_closure || expecting_terminator {
                                        self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedOperator });
                                    }
                                    self.builder.token(to_raw(SyntaxKind::Comma), text);
                                    self.next_raw_token();
                                }
                                TokenKind::RightBracket => {
                                    if !expecting_closure && !expecting_terminator {
                                        self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedOperator });
                                    }
                                    self.builder.token(to_raw(SyntaxKind::RightBracket), text);
                                    self.next_raw_token();
                                    result = true;
                                    break;
                                }
                                TokenKind::TripleDot => {
                                    if !expecting_closure && seen_parameter {
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
                        }
                    }
                },
                _ => ()
            }
        }
        self.builder.finish_node();
        self.eat_whitespace();
        return result
    }
}
