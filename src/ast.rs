use std::process::Termination;

use crate::lexer::LuaLexer;
use crate::lexer::Token;
use crate::lexer::TokenKind;
use rowan::GreenNodeBuilder;

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SyntaxKind {
    Invalid,
    Whitespace,
    Newline,
    Comment,
    Identifier,
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
    Function,
    Statement,
    Assignment,
    ReturnStatement,
    VarList,
    VariableName,
    ExpList,
    Expression,
    PrefixExpression,
    Arguments,
    ParameterList,
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

pub enum ErrorKind {
    NotClosedBlock,
    UnexpectedKeyword,
    UnexpectedOperator,
    ExpectingComma,
    UnexpectedParameter,
    InvalidFunctionName,
    InvalidFunction,
}
pub struct Error {
    start: usize,
    end: usize,
    kind: ErrorKind,
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
        _ => SyntaxKind::Invalid,
    }
}

fn to_raw(s: SyntaxKind) -> rowan::SyntaxKind {
    rowan::SyntaxKind(s as u16)
}
fn from_raw(s: rowan::SyntaxKind) -> SyntaxKind {
    SyntaxKind::from(s.0)
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

pub struct AstGenerator<'a> {
    text: &'a str,
    lexer: LuaLexer<'a>,
    builder: GreenNodeBuilder<'a>,
    errors: Vec<Error>,
    token_cache: Option<Token>,
}

impl<'a> AstGenerator<'a> {
    pub fn new(text: &'a str) -> AstGenerator<'a> {
        AstGenerator {
            text: text,
            lexer: LuaLexer::new(text),
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            token_cache: None,
        }
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
            self.builder.start_node(to_raw(SyntaxKind::Block));
            self.scan_block(None, Some(t));
            self.builder.finish_node();
        } else {
            self.builder.token(to_raw(SyntaxKind::EoF), "")
        }
        let b = std::mem::take(&mut self.builder);
        return b.finish()
    }

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
                    _ => return,
                };
                self.next_raw_token();
            } else {
                break;
            }
        }
    }

    fn scan_identifier(&mut self, token: &Token, text: &str) {
        match str_to_keyword(text) {
            SyntaxKind::DoKeyword => {
                self.builder.token(to_raw(SyntaxKind::DoKeyword), text);
                self.builder.start_node(to_raw(SyntaxKind::Block));
                if !self.scan_block(Some(SyntaxKind::EndKeyword), None) {
                    self.errors.push(Error{ start: token.start, end: token.end, kind: ErrorKind::NotClosedBlock })
                }
                self.builder.finish_node();
            },
            SyntaxKind::FunctionKeyword => {
                self.builder.token(to_raw(SyntaxKind::FunctionKeyword), text);
                self.eat_whitespace();
                if let Some(token) = self.peek_raw_token() {
                    let text = &self.text[token.start .. token.end];
                    match token.kind {
                        TokenKind::Identifier => {
                            let keyword_kind = str_to_keyword(text);
                            if keyword_kind != SyntaxKind::Invalid {
                                self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::InvalidFunctionName });
                                self.next_raw_token();
                                self.builder.token(to_raw(keyword_kind), text);
                            } else {
                                self.next_raw_token();
                                self.builder.token(to_raw(SyntaxKind::Identifier), text);
                            }
                            self.scan_parameters();
                        }
                        TokenKind::LeftBracket => {
                            self.scan_parameters();
                        }
                        _ => {
                            self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::InvalidFunction});
                            return
                        }
                    }
                }
            }
            SyntaxKind::Invalid => { // variable name
                self.builder.token(to_raw(SyntaxKind::Identifier), text);
            }
            _ => {
                self.builder.token(to_raw(SyntaxKind::Invalid), text);
                self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedKeyword});
            },
        }
    }

    fn scan_block(&mut self, terminator: Option<SyntaxKind>, starting_token: Option<Token>) -> bool {
        let mut t;
        if let Some(token) = starting_token {
            t = token;
        } else if let Some(token) = self.next_raw_token() {
            t = token;
        } else {
            return false
        }
        loop {
            let text = &self.text[t.start .. t.end];
            match t.kind {
                TokenKind::Comment { validity: _, modifier: _ } => {
                    self.builder.token(to_raw(SyntaxKind::Comment), text)
                },
                TokenKind::Whitespace => {
                    self.builder.token(to_raw(SyntaxKind::Whitespace), text);
                },
                TokenKind::Identifier =>  {
                    let keyword = str_to_keyword(text);
                    if Some(keyword) == terminator {
                        return true
                    }
                    self.builder.start_node(to_raw(SyntaxKind::Statement));
                    self.scan_identifier(&t, text);
                    self.builder.finish_node();
                },
                TokenKind::EoF => {
                    return false
                }
                _ => {
                    self.builder.token(to_raw(SyntaxKind::Invalid), text)
                }
            }

            if let Some(token) = self.next_raw_token() {
                t = token;
            } else {
                return false
            }
        }
    }

    fn scan_parameters(&mut self) -> bool {
        self.eat_whitespace();
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
                                    if keyword_type != SyntaxKind::Invalid {
                                        self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedKeyword });
                                        self.builder.token(to_raw(keyword_type), text);
                                    } else {
                                        self.builder.token(to_raw(SyntaxKind::Identifier), text);
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
                                    return true
                                }
                                TokenKind::TripleDot => {
                                    if !expecting_closure && seen_parameter {
                                        self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedOperator });
                                    }
                                    self.builder.token(to_raw(SyntaxKind::TripleDot), text);
                                    expecting_terminator = true;
                                    self.next_raw_token();
                                }
                                _ => {
                                    self.errors.push(Error { start: token.start, end: token.end, kind: ErrorKind::UnexpectedOperator });
                                    return false
                                }
                            };
                        }
                    }
                },
                _ => {
                    return false
                }
            }
        }
        return false
    }
}
