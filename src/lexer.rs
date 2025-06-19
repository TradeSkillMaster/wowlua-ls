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

use std::{iter::Peekable, str::CharIndices};

enum TokenKind {
    Invalid,
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
}
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
}
pub struct LuaLexer<'a> {
    position: usize,
    text: &'a str,
    chars: Peekable<CharIndices<'a>>,
    tokens: Vec<Token>,
    cache: String,
} 

impl<'a> LuaLexer<'a> {
    pub fn new(text: &'a str) -> LuaLexer<'a> {
        LuaLexer {
            text, chars: text.char_indices().peekable(), position: 0, tokens: Vec::new(), cache: String::new(),
        }
    }

    pub fn process(&mut self) {
    }

    fn next_char(&mut self) -> Option<(usize, char)> {
        self.chars.next()
    }

    fn peek_char(&mut self) -> Option<&(usize, char)> {
        self.chars.peek()
    }

    fn next_token(&mut self) -> Option<Token> {
        let (start, ch) = self.next_char()?;
        
        let mut t: Token;

        match ch {
            '.' => {
                if let Some((pos, ch)) = self.peek_char() {
                    match ch {
                        '.' => {
                            if let Some((pos, ch)) = self.peek_char() {
                                match ch {
                                    '.' => {
                                        return Some(Token{ kind: TokenKind::TripleDot, start, end: *pos })
                                    }
                                    _ => ()
                                }
                            }
                            return Some(Token{ kind: TokenKind::DoubleDot, start, end: *pos })
                        }
                        _ => ()
                    }
                }
                return Some(Token{ kind: TokenKind::Dot, start, end: start })
            } 
            '(' => return Some(Token{ kind: TokenKind::LeftBracket, start, end: start}),
            ')' => return Some(Token{ kind: TokenKind::RightBracket, start, end: start}),
            '{' => return Some(Token{ kind: TokenKind::LeftCurlyBracket, start, end: start}),
            '}' => return Some(Token{ kind: TokenKind::RightCurlyBracket, start, end: start}),
            '0' | '1' | '2' | '3'| '4' | '5' | '6' | '7' | '8' | '9' => {
                let (pos, ch) = self.next_char()?;
            }
        }

        Some(t)
    }
}
