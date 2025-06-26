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

use std::str::Chars;

pub mod token_validity {
    #[derive(PartialEq, Debug, Clone, Copy)]
    pub enum Number {
        Valid,
        Invalid,
    }

    #[derive(PartialEq, Debug, Clone, Copy)]
    pub enum String {
        Valid,
        NotTerminated,
    }

    #[derive(PartialEq, Debug, Clone, Copy)]
    pub enum Comment {
        Valid,
        NotTerminated,
    }
}
pub mod token_modifier {
    #[derive(PartialEq, Debug, Clone, Copy)]
    pub enum Number {
        Integer,
        Decimal,
        Hex,
        Exponential
    }
    #[derive(PartialEq, Debug, Clone, Copy)]
    pub enum String {
        LongBrackets,
        Quotes,
        DoubleQuotes
    }
    #[derive(PartialEq, Debug, Clone, Copy)]
    pub enum Comment {
        Oneline,
        Multiline,
    }
}

#[derive(PartialEq, Debug, Clone, Copy)]
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
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Doesn't determine that `start` is used in another file
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}
pub struct Generator<'a> {
    position: usize,
    peek_cache: Option<(usize, char, usize)>,
    chars: Chars<'a>,
} 

impl<'a> Generator<'a> {
    pub fn new(text: &'a str) -> Generator<'a> {
        Generator {
            chars: text.chars(),
            position: 0,
            peek_cache: None,
        }
    }

    pub fn process_all(&mut self) -> Vec<Token> {
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

    fn next_char(&mut self) -> Option<(usize, char, usize)> {
        if let Some(p) = self.peek_cache {
            self.peek_cache = None;
            return Some(p)
        }
        if let Some(ch) = self.chars.next() {
            let p = self.position;

            let ch_len = ch.len_utf8();
            if ch == '\n' {
                self.position += 1;
            } else {
                self.position += ch_len;
            }

            return Some((p, ch, p + ch_len))
        }
        None
    }

    fn peek_char(&mut self) -> Option<(usize, char, usize)> {
        match self.peek_cache {
            Some(_) => self.peek_cache,
            None => {
                self.peek_cache = self.next_char();
                return self.peek_cache
            }
        }
    }

    pub fn next_token(&mut self) -> Option<Token> {
        let (start, ch, end) = self.next_char()?;

        match ch {
            '.' => return self.scan_dot(start, end),
            '(' => return Some(Token{ kind: TokenKind::LeftBracket, start, end}),
            ')' => return Some(Token{ kind: TokenKind::RightBracket, start, end}),
            '{' => return Some(Token{ kind: TokenKind::LeftCurlyBracket, start, end}),
            '}' => return Some(Token{ kind: TokenKind::RightCurlyBracket, start, end}),
            '[' => return self.scan_open_square_bracket(start, end),
            ']' => return Some(Token{ kind: TokenKind::RightSquareBracket, start, end}),
            '-' => return self.scan_minus(start, end),
            '+' => return Some(Token{ kind: TokenKind::Plus, start, end}),
            '*' => return Some(Token{ kind: TokenKind::Asterisk, start, end}),
            '/' => return Some(Token{ kind: TokenKind::Slash, start, end}),
            '%' => return Some(Token{ kind: TokenKind::Modulo, start, end}),
            ';' => return Some(Token{ kind: TokenKind::Semicolon, start, end}),
            ':' => return Some(Token{ kind: TokenKind::Colon, start, end}),
            ',' => return Some(Token{ kind: TokenKind::Comma, start, end}),
            '=' => return self.scan_equals(start, end),
            '<' => return self.scan_less_than(start, end),
            '>' => return self.scan_greater_than(start, end),
            '~' => return self.scan_tilde(start, end),
            '#' => return Some(Token{ kind: TokenKind::Hash, start, end}),
            '^' => return Some(Token{ kind: TokenKind::Hat, start, end}),
            '0'..='9' => return self.scan_number(start, ch, end),
            '"' => return self.scan_simple_string(start, ch, end),
            '\'' => return self.scan_simple_string(start, ch, end),
            '\n' => return Some(Token{ kind: TokenKind::Newline, start, end}),
            '\r' => {
                if let Some((_, ch, end)) = self.peek_char() {
                    if ch == '\n' {
                        self.next_char();
                        return Some(Token{ kind: TokenKind::Newline, start, end})
                    }
                }
                self.scan_whitespace(start, end)
            }
            _ => {
                if ch.is_alphabetic() || ch == '_' {
                    return self.scan_identifier(start, end)
                } else if ch.is_whitespace() {
                    return self.scan_whitespace(start, end)
                } else {
                    return Some(Token{ kind: TokenKind::Invalid, start, end})
                }
            }
        }
    }

    fn scan_dot(&mut self, start: usize, end: usize) -> Option<Token> {
        if let Some((_, ch, end)) = self.peek_char() {
            match ch {
                '.' => {
                    self.next_char();
                    if let Some((_, ch, end)) = self.peek_char() {
                        match ch {
                            '.' => {
                                self.next_char();
                                return Some(Token{ kind: TokenKind::TripleDot, start, end })
                            }
                            _ => ()
                        }
                    }
                    return Some(Token{ kind: TokenKind::DoubleDot, start, end })
                }
                '0'..='9' => return self.scan_number(start, '.', end),
                _ => ()
            }
        }
        return Some(Token{ kind: TokenKind::Dot, start, end })
    }

    fn scan_open_square_bracket(&mut self, start: usize, end: usize) -> Option<Token> {
        if let Some((_, ch, _)) = self.peek_char() {
            match ch {
                '[' | '=' => return self.scan_long_bracket_string(start), // Got a string 
                _ => return Some(Token{ kind: TokenKind::LeftSquareBracket, start, end })
            }
        }
        return Some(Token{ kind: TokenKind::LeftSquareBracket, start, end })
    }

    fn scan_long_bracket_string(&mut self, start: usize) -> Option<Token> {
        let (_, ch, end) = self.next_char()?;
        let mut opening_counter = 0;
        if ch == '=' {
            opening_counter += 1;
            while let Some((_, ch, end)) = self.peek_char() {
                match ch {
                    '=' => opening_counter += 1,
                    '[' => break,
                    _ => return Some(Token{kind: TokenKind::Invalid, start, end})
                }
                self.next_char();
            }
        } else if ch != '[' {
            return Some(Token{kind: TokenKind::Invalid, start, end})
        }
        let mut end = start;
        while let Some((_, ch, end_2)) = self.next_char() {
            end = end_2;
            if ch == ']' {
                if opening_counter > 0 {
                    let mut closing_counter = 0;
                    while closing_counter < opening_counter {
                        if let Some((_, ch, end_2)) = self.next_char() {
                            end = end_2;
                            match ch {
                                '=' => closing_counter += 1,
                                _ => break
                            }
                        }
                    }
                    if opening_counter == closing_counter {
                        if let Some((_, ch, end_2)) = self.peek_char() {
                            if ch == ']' {
                                end = end_2;
                                self.next_char();
                                return Some(Token{
                                    kind: TokenKind::String { validity: token_validity::String::Valid, modifier: token_modifier::String::LongBrackets },
                                    start,
                                    end,
                                })
                            }
                        }
                    }
                } else if let Some((_, ch, end)) = self.peek_char() {
                    if ch == ']' {
                        self.next_char();
                        return Some(Token{
                            kind: TokenKind::String { validity: token_validity::String::Valid, modifier: token_modifier::String::LongBrackets },
                            start,
                            end,
                        })
                    }
                }
            }
        }
        return Some(Token{
            kind: TokenKind::String { validity: token_validity::String::NotTerminated, modifier: token_modifier::String::LongBrackets },
            start,
            end,
        })

    }

    fn scan_minus(&mut self, start: usize, end: usize) -> Option<Token> {
        if let Some((_, ch, _)) = self.peek_char() {
            if ch == '-' {
                self.next_char();
                if let Some((pos, ch, end)) = self.peek_char() {
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
                    let mut end = end;
                    while let Some((_, ch, end_2)) = self.peek_char() {
                        if ch == '\r' || ch == '\n' {
                            break;
                        } else {
                            end = end_2;
                            self.next_char();
                        }
                    }
                    return Some(Token{
                        kind: TokenKind::Comment { validity: token_validity::Comment::Valid, modifier: token_modifier::Comment::Oneline },
                        start, end,
                    })
                }
            }
        }
        Some(Token{ kind: TokenKind::Minus, start, end })
    }

    fn scan_number(&mut self, start: usize, ch: char, end: usize) -> Option<Token> {
        let mut modifier = token_modifier::Number::Integer;
        let mut validity = token_validity::Number::Valid;
        match ch {
            '0' => match self.peek_char() {
                Some((_, 'x', _)) => {
                    self.next_char();
                    modifier = token_modifier::Number::Hex;
                },
                _ => (),
            },
            '.' => modifier = token_modifier::Number::Decimal,
            _ => ()
        }
        let mut end = end;
        while let Some((_, ch, end_2)) = self.peek_char() {
            if ch.is_alphanumeric() || ch == '.' {
                end = end_2;
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
                                Some((_, '+', _)) | Some((_, '-', _)) => {
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
        }
        Some(Token{ kind: TokenKind::Number { validity, modifier }, start, end })
    }

    fn scan_whitespace(&mut self, start: usize, end: usize) -> Option<Token> {
        let mut end = end;
        while let Some((_, ch, end_2)) = self.peek_char() {
            if ch.is_whitespace() {
                end = end_2;
                self.next_char();
                continue;
            } else {
                break
            }
        }
        return Some(Token { kind: TokenKind::Whitespace, start, end })
    }

    fn scan_identifier(&mut self, start: usize, end: usize) -> Option<Token> {
        let mut end = end;
        while let Some((_, ch, end_2)) = self.peek_char() {
            if ch.is_alphanumeric() || ch == '_' {
                end = end_2;
                self.next_char();
            } else {
                break
            }
        }
        return Some(Token { kind: TokenKind::Identifier, start, end })
    }

    fn scan_equals(&mut self, start: usize, end: usize) -> Option<Token> {
        if let Some((_, ch, end)) = self.peek_char() {
            if ch == '=' {
                self.next_char();
                return Some(Token{ kind: TokenKind::EqualsBoolean, start, end})
            }
        }
        return Some(Token{ kind: TokenKind::Assign, start, end})
    }

    fn scan_less_than(&mut self, start: usize, end: usize) -> Option<Token> {
        if let Some((_, ch, end)) = self.peek_char() {
            if ch == '=' {
                self.next_char();
                return Some(Token{ kind: TokenKind::LessThanOrEquals, start, end})
            }
        }
        return Some(Token{ kind: TokenKind::LessThan, start, end})
    }

    fn scan_greater_than(&mut self, start: usize, end: usize) -> Option<Token> {
        if let Some((_, ch, end)) = self.peek_char() {
            if ch == '=' {
                self.next_char();
                return Some(Token{ kind: TokenKind::GreaterThanOrEquals, start, end})
            }
        }
        return Some(Token{ kind: TokenKind::GreaterThan, start, end })
    }

    fn scan_tilde(&mut self, start: usize, end: usize) -> Option<Token> {
        if let Some((_, ch, end)) = self.peek_char() {
            if ch == '=' {
                self.next_char();
                return Some(Token{ kind: TokenKind::NotEqualsBoolean, start, end})
            }
        }
        return Some(Token{ kind: TokenKind::Invalid, start, end})
    }

    fn scan_simple_string(&mut self, start: usize, terminator: char, _end: usize) -> Option<Token> {
        let modifier;
        if terminator == '\'' {
            modifier = token_modifier::String::Quotes
        } else {
            modifier = token_modifier::String::DoubleQuotes
        }
        let mut seen_escape = false;
        let mut last_stable_pos = start + 1;
        while let Some((_, ch, end)) = self.peek_char() {
            if ch == terminator && !seen_escape {
                self.next_char();
                return Some(Token{
                    kind: TokenKind::String { validity: token_validity::String::Valid, modifier},
                    start, end
                })
            } else if ch == '\\' && !seen_escape {
                seen_escape = true
            } else if (ch == '\n' || ch == '\r') && !seen_escape {
                return Some(Token{
                    kind: TokenKind::String { validity: token_validity::String::NotTerminated, modifier},
                    start, end: last_stable_pos,
                })
            } else {
                seen_escape = seen_escape && (ch == '\r' || ch == '\n');
                last_stable_pos = end;
            }
            self.next_char();
        }
        return Some(Token{
            kind: TokenKind::String { validity: token_validity::String::NotTerminated, modifier},
            start, end: last_stable_pos,
        })
    }
}
