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

enum TokenKind {
    Invalid,
    Identifier,
    String,
    Number,
    EoF,
}
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
}
struct LuaLexer<'a> {
    position: usize,
    text: &'a str,
    tokens: Vec<Token>,
} 

impl<'a> LuaLexer<'a> {
    pub fn new(text: &'a str) -> LuaLexer<'a> {
        LuaLexer {
            text, position: 0, tokens: Vec::new()
        }
    }

    fn process(&self) {

    }
}
