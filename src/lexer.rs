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

use std::{iter::Peekable, str::Chars};

pub mod token_validity {
    #[derive(PartialEq, Debug)]
    pub enum Number {
        Valid,
        Invalid,
    }

    #[derive(PartialEq, Debug)]
    pub enum String {
        Valid,
        NotTerminated,
    }

    #[derive(PartialEq, Debug)]
    pub enum Comment {
        Valid,
        NotTerminated,
    }
}
pub mod token_modifier {
    #[derive(PartialEq, Debug)]
    pub enum Number {
        Integer,
        Decimal,
        Hex,
        Exponential
    }
    #[derive(PartialEq, Debug)]
    pub enum String {
        LongBrackets,
        Quotes,
        DoubleQuotes
    }
    #[derive(PartialEq, Debug)]
    pub enum Comment {
        Oneline,
        Multiline,
    }
}

#[derive(PartialEq, Debug)]
pub enum TokenKind {
    Invalid,
    Whitespace,
    Newline,
    Comment{validity: token_validity::Comment, modifier: token_modifier::Comment},
    Identifier,
    String{validity: token_validity::String, modifier: token_modifier::String},
    Number{validity: token_validity::Number, modifier: token_modifier::Number},
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
}
#[derive(Debug)]
pub struct Token {
    kind: TokenKind,
    start: Position,
    end: Position,
}
#[derive(Debug, Clone, Copy)]
pub struct Position {
    line: usize,
    column: usize,
    absolute: usize,
}
pub struct LuaLexer<'a> {
    position: Position,
    peek_cache: Option<(Position, char)>,
    chars: Chars<'a>,
} 

impl<'a> LuaLexer<'a> {
    pub fn new(text: &'a str) -> LuaLexer<'a> {
        LuaLexer {
            chars: text.chars(),
            position: Position{line: 0, column: 0, absolute: 0},
            peek_cache: None,
        }
    }

    pub fn process(&mut self) -> Vec<Token> {
        let mut res: Vec<Token> = Vec::new();

        loop {
            let token = self.next_token();
            match token {
                Some(t) => res.push(t),
                None => {
                    res.push(Token { kind: TokenKind::EoF, start: self.position, end: self.position});
                    return res
                }
            }
        }
    }

    fn next_char(&mut self) -> Option<(Position, char)> {
        if let Some(p) = self.peek_cache {
            self.peek_cache = None;
            return Some(p)
        }
        if let Some(ch) = self.chars.next() {
            let p = self.position;

            if ch == '\n' {
                self.position.absolute += 1;
                self.position.line += 1;
                self.position.column = 0;
            } else {
                let ch_len = ch.len_utf8();
                self.position.absolute += ch_len;
                self.position.column += ch_len;
            }

            return Some((p, ch))
        }
        None
    }

    fn peek_char(&mut self) -> Option<(Position, char)> {
        match self.peek_cache {
            Some(_) => self.peek_cache,
            None => {
                self.peek_cache = self.next_char();
                return self.peek_cache
            }
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        let (start, ch) = self.next_char()?;

        match ch {
            '.' => return self.scan_dot(start),
            '(' => return Some(Token{ kind: TokenKind::LeftBracket, start, end: start}),
            ')' => return Some(Token{ kind: TokenKind::RightBracket, start, end: start}),
            '{' => return Some(Token{ kind: TokenKind::LeftCurlyBracket, start, end: start}),
            '}' => return Some(Token{ kind: TokenKind::RightCurlyBracket, start, end: start}),
            '[' => return self.scan_open_square_bracket(start),
            ']' => return Some(Token{ kind: TokenKind::RightSquareBracket, start, end: start}),
            '-' => return self.scan_minus(start),
            '+' => return Some(Token{ kind: TokenKind::Plus, start, end: start}),
            '*' => return Some(Token{ kind: TokenKind::Asterisk, start, end: start}),
            '/' => return Some(Token{ kind: TokenKind::Slash, start, end: start}),
            '%' => return Some(Token{ kind: TokenKind::Modulo, start, end: start}),
            ';' => return Some(Token{ kind: TokenKind::Semicolon, start, end: start}),
            ':' => return Some(Token{ kind: TokenKind::Colon, start, end: start}),
            ',' => return Some(Token{ kind: TokenKind::Comma, start, end: start}),
            '=' => return self.scan_equals(start),
            '<' => return self.scan_less_than(start),
            '>' => return self.scan_greater_than(start),
            '~' => return self.scan_tilde(start),
            '#' => return Some(Token{ kind: TokenKind::Hash, start, end: start}),
            '^' => return Some(Token{ kind: TokenKind::Hat, start, end: start}),
            '0'..='9' => return self.scan_number(start, ch),
            '"' => return self.scan_simple_string(start, ch),
            '\'' => return self.scan_simple_string(start, ch),
            '\n' => return Some(Token{ kind: TokenKind::Newline, start, end: start}),
            '\r' => {
                if let Some((end, ch)) = self.peek_char() {
                    if ch == '\n' {
                        self.next_char();
                        return Some(Token{ kind: TokenKind::Newline, start, end: end})
                    }
                }
                self.scan_whitespace(start)
            }
            _ => {
                if ch.is_alphabetic() || ch == '_' {
                    return self.scan_identifier(start)
                } else if ch.is_whitespace() {
                    return self.scan_whitespace(start)
                } else {
                    return Some(Token{ kind: TokenKind::Invalid, start, end: start})
                }
            }
        }
    }

    fn scan_dot(&mut self, start: Position) -> Option<Token> {
        if let Some((pos, ch)) = self.peek_char() {
            match ch {
                '.' => {
                    self.next_char();
                    if let Some((pos, ch)) = self.peek_char() {
                        match ch {
                            '.' => {
                                return Some(Token{ kind: TokenKind::TripleDot, start, end: pos })
                            }
                            _ => ()
                        }
                    }
                    return Some(Token{ kind: TokenKind::DoubleDot, start, end: pos })
                }
                '0'..='9' => return self.scan_number(start, '.'),
                _ => ()
            }
        }
        return Some(Token{ kind: TokenKind::Dot, start, end: start })
    }

    fn scan_open_square_bracket(&mut self, start: Position) -> Option<Token> {
        if let Some((pos, ch)) = self.peek_char() {
            match ch {
                '[' | '=' => return self.scan_long_bracket_string(start), // Got a string 
                _ => return Some(Token{ kind: TokenKind::LeftSquareBracket, start, end: pos })
            }
        }
        return Some(Token{ kind: TokenKind::Dot, start, end: start })
    }

    fn scan_long_bracket_string(&mut self, start: Position) -> Option<Token> {
        let (pos, ch) = self.next_char()?;
        let mut opening_counter = 0;
        if ch == '=' {
            opening_counter += 1;
            loop {
                if let Some((pos, ch)) = self.peek_char() {
                    match ch {
                        '=' => opening_counter += 1,
                        '[' => break,
                        _ => return Some(Token{kind: TokenKind::Invalid, start, end: pos})
                    }
                    self.next_char();
                }
            }
        } else if ch != '[' {
            return Some(Token{kind: TokenKind::Invalid, start, end: pos})
        }
        let mut end = start;
        loop {
            if let Some((pos, ch)) = self.next_char() {
                end = pos;
                if ch == ']' {
                    if opening_counter > 0 {
                        let mut closing_counter = 0;
                        while closing_counter < opening_counter {
                            if let Some((_, ch)) = self.next_char() {
                                match ch {
                                    '=' => closing_counter += 1,
                                    _ => break
                                }
                            }
                        }
                        if opening_counter == closing_counter {
                            if let Some((pos, ch)) = self.peek_char() {
                                if ch == ']' {
                                    self.next_char();
                                    return Some(Token{
                                        kind: TokenKind::String { validity: token_validity::String::Valid, modifier: token_modifier::String::LongBrackets },
                                        start,
                                        end: pos,
                                    })
                                }
                            }
                        }
                    } else if let Some((pos, ch)) = self.peek_char() {
                        if ch == ']' {
                            return Some(Token{
                                kind: TokenKind::String { validity: token_validity::String::Valid, modifier: token_modifier::String::LongBrackets },
                                start,
                                end: pos,
                            })
                        }
                    }
                }
            } else {
                return Some(Token{
                    kind: TokenKind::String { validity: token_validity::String::NotTerminated, modifier: token_modifier::String::LongBrackets },
                    start,
                    end,
                })
            }
        }

    }

    fn scan_minus(&mut self, start: Position) -> Option<Token> {
        if let Some((_, ch)) = self.peek_char() {
            if ch == '-' {
                self.next_char();
                if let Some((pos, ch)) = self.peek_char() {
                    if ch == '[' {
                        self.next_char();
                        let multiline = self.scan_long_bracket_string(pos);
                        if let Some(t) = multiline {
                            match t.kind {
                                TokenKind::String { validity: token_validity::String::NotTerminated, modifier: _} => {
                                    return Some(Token{
                                        kind: TokenKind::Comment { validity: token_validity::Comment::NotTerminated, modifier: token_modifier::Comment::Multiline },
                                        start, end: t.end,
                                    })
                                }
                                TokenKind::Invalid => (),
                                _ => return Some(Token{
                                        kind: TokenKind::Comment { validity: token_validity::Comment::Valid, modifier: token_modifier::Comment::Multiline },
                                        start, end: t.end,
                                    })
                            }

                        }
                    }
                    let mut end = pos;
                    loop {
                        if let Some((pos, ch)) = self.peek_char() {
                            if ch == '\r' || ch == '\n' {
                                break;
                            } else {
                                end = pos;
                                self.next_char();
                            }
                        } else {
                            break
                        }
                    }
                    return Some(Token{
                        kind: TokenKind::Comment { validity: token_validity::Comment::Valid, modifier: token_modifier::Comment::Oneline },
                        start, end,
                    })
                }
            }
        }
        Some(Token{ kind: TokenKind::Minus, start, end: start })
    }

    fn scan_number(&mut self, start: Position, ch: char) -> Option<Token> {
        let mut modifier = token_modifier::Number::Integer;
        let mut validity = token_validity::Number::Valid;
        match ch {
            '0' => match self.peek_char() {
                Some((_, 'x')) => {
                    self.next_char();
                    modifier = token_modifier::Number::Hex;
                },
                _ => (),
            },
            '.' => modifier = token_modifier::Number::Decimal,
            _ => ()
        }
        let mut end = start;
        loop {
            if let Some((pos, ch)) = self.peek_char() {
                if ch.is_alphanumeric() || ch == '.' {
                    end = pos;
                    self.next_char();
                    match modifier {
                        token_modifier::Number::Hex => {
                            if !ch.is_ascii_hexdigit() {
                                validity = token_validity::Number::Invalid;
                            }
                        }
                        token_modifier::Number::Integer => {
                            if ch == '.' {
                                modifier = token_modifier::Number::Decimal;
                            } else if !ch.is_numeric() {
                                validity = token_validity::Number::Invalid;
                            }
                        }
                        token_modifier::Number::Decimal => {
                            if ch == 'E' || ch == 'e' {
                                modifier = token_modifier::Number::Exponential;
                                match self.peek_char() { // Eat + or - at the start of the exponent
                                    Some((_, '+')) | Some((_, '-')) => {
                                        self.next_char();
                                    }
                                    _ => ()
                                }
                            } else if !ch.is_numeric() {
                                validity = token_validity::Number::Invalid;
                            }
                        }
                        token_modifier::Number::Exponential => {
                            if !ch.is_numeric() {
                                validity = token_validity::Number::Invalid;
                            }
                        }
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        Some(Token{ kind: TokenKind::Number { validity, modifier }, start, end })
    }

    fn scan_whitespace(&mut self, start: Position) -> Option<Token> {
        let mut end = start;
        loop {
            if let Some((pos, ch)) = self.peek_char() {
                if ch.is_whitespace() {
                    end = pos;
                    self.next_char();
                    continue;
                }
            }
            return Some(Token { kind: TokenKind::Whitespace, start, end })
        }
    }

    fn scan_identifier(&mut self, start: Position) -> Option<Token> {
        let mut end = start;
        loop {
            if let Some((pos, ch)) = self.peek_char() {
                if ch.is_alphanumeric() || ch == '_' {
                    end = pos;
                    self.next_char();
                    continue;
                }
            }
            return Some(Token { kind: TokenKind::Identifier, start, end })
        }
    }

    fn scan_equals(&mut self, start: Position) -> Option<Token> {
        if let Some((pos, ch)) = self.peek_char() {
            if ch == '=' {
                self.next_char();
                return Some(Token{ kind: TokenKind::EqualsBoolean, start, end: pos})
            }
        }
        return Some(Token{ kind: TokenKind::Assign, start, end: start})
    }

    fn scan_less_than(&mut self, start: Position) -> Option<Token> {
        if let Some((pos, ch)) = self.peek_char() {
            if ch == '=' {
                self.next_char();
                return Some(Token{ kind: TokenKind::LessThanOrEquals, start, end: pos})
            }
        }
        return Some(Token{ kind: TokenKind::LessThan, start, end: start})
    }

    fn scan_greater_than(&mut self, start: Position) -> Option<Token> {
        if let Some((pos, ch)) = self.peek_char() {
            if ch == '=' {
                self.next_char();
                return Some(Token{ kind: TokenKind::GreaterThanOrEquals, start, end: pos})
            }
        }
        return Some(Token{ kind: TokenKind::GreaterThan, start, end: start})
    }

    fn scan_tilde(&mut self, start: Position) -> Option<Token> {
        if let Some((pos, ch)) = self.peek_char() {
            if ch == '=' {
                self.next_char();
                return Some(Token{ kind: TokenKind::NotEqualsBoolean, start, end: pos})
            }
        }
        return Some(Token{ kind: TokenKind::Invalid, start, end: start})
    }

    fn scan_simple_string(&mut self, start: Position, terminator: char) -> Option<Token> {
        let modifier;
        if terminator == '\'' {
            modifier = token_modifier::String::Quotes
        } else {
            modifier = token_modifier::String::DoubleQuotes
        }
        let mut seen_escape = false;
        let mut last_stable_pos: Option<Position> = None;
        loop {
            if let Some((pos, ch)) = self.peek_char() {
                if ch == terminator {
                    self.next_char();
                    return Some(Token{
                        kind: TokenKind::String { validity: token_validity::String::Valid, modifier},
                        start, end: pos
                    })
                } else if ch == '\\' {
                    seen_escape = true
                } else if (ch == '\n' || ch == '\r') && !seen_escape {
                    return Some(Token{
                        kind: TokenKind::String { validity: token_validity::String::NotTerminated, modifier},
                        start, end: match last_stable_pos { Some(p) => p, None => start },
                    })
                } else {
                    seen_escape = seen_escape && ch != '\r' && ch != '\n';
                    last_stable_pos = Some(pos);
                }
                self.next_char();
            } else {
                return Some(Token{
                    kind: TokenKind::String { validity: token_validity::String::NotTerminated, modifier},
                    start, end: match last_stable_pos { Some(p) => p, None => start },
                })
            }
        }
    }
}
