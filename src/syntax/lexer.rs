use crate::syntax::SyntaxKind as SK;

/// A lexer error attached to a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LexError {
    InvalidNumber,
    UnterminatedString,
    UnterminatedComment,
}

/// A token produced by the lexer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    pub kind: SK,
    pub start: u32,
    pub end: u32,
    pub error: Option<LexError>,
}

impl Token {
    #[inline]
    fn new(kind: SK, start: u32, end: u32) -> Self {
        Self { kind, start, end, error: None }
    }

    #[inline]
    fn with_error(kind: SK, start: u32, end: u32, error: LexError) -> Self {
        Self { kind, start, end, error: Some(error) }
    }
}

/// Byte-based lexer for Lua source code.
/// Produces `SyntaxKind` tokens directly (including keyword resolution).
pub(crate) struct Lexer<'a> {
    source: &'a [u8],
    pos: u32,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self { source: text.as_bytes(), pos: 0 }
    }

    #[inline]
    fn len(&self) -> u32 {
        self.source.len() as u32
    }

    #[inline]
    fn byte(&self, pos: u32) -> u8 {
        self.source[pos as usize]
    }

    #[inline]
    fn peek_byte(&self) -> Option<u8> {
        if self.pos < self.len() { Some(self.byte(self.pos)) } else { None }
    }

    #[inline]
    fn advance(&mut self) -> u8 {
        let b = self.byte(self.pos);
        self.pos += 1;
        b
    }

    pub(crate) fn next_token(&mut self) -> Option<Token> {
        let b = *self.source.get(self.pos as usize)?;
        let start = self.pos;
        self.pos += 1;

        Some(match b {
            b'(' => Token::new(SK::LeftBracket, start, self.pos),
            b')' => Token::new(SK::RightBracket, start, self.pos),
            b'{' => Token::new(SK::LeftCurlyBracket, start, self.pos),
            b'}' => Token::new(SK::RightCurlyBracket, start, self.pos),
            b']' => Token::new(SK::RightSquareBracket, start, self.pos),
            b'+' => Token::new(SK::Plus, start, self.pos),
            b'*' => Token::new(SK::Asterisk, start, self.pos),
            b'/' => Token::new(SK::Slash, start, self.pos),
            b'%' => Token::new(SK::Modulo, start, self.pos),
            b';' => Token::new(SK::Semicolon, start, self.pos),
            b':' => Token::new(SK::Colon, start, self.pos),
            b',' => Token::new(SK::Comma, start, self.pos),
            b'#' => Token::new(SK::Hash, start, self.pos),
            b'^' => Token::new(SK::Hat, start, self.pos),
            b'\n' => Token::new(SK::Newline, start, self.pos),
            b'.' => self.scan_dot(start),
            b'[' => self.scan_open_square_bracket(start),
            b'-' => self.scan_minus(start),
            b'=' => self.scan_two_char(start, b'=', SK::EqualsBoolean, SK::Assign),
            b'<' => self.scan_two_char(start, b'=', SK::LessThanOrEquals, SK::LessThan),
            b'>' => self.scan_two_char(start, b'=', SK::GreaterThanOrEquals, SK::GreaterThan),
            b'~' => self.scan_two_char(start, b'=', SK::NotEqualsBoolean, SK::Invalid),
            b'0'..=b'9' => self.scan_number(start),
            b'"' | b'\'' => self.scan_string(start, b),
            b'\r' => {
                // \r\n → single Newline
                if self.peek_byte() == Some(b'\n') { self.pos += 1; }
                Token::new(SK::Newline, start, self.pos)
            }
            b' ' | b'\t' => self.scan_whitespace(start),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.scan_identifier(start),
            _ => {
                // Non-ASCII: could be a UTF-8 identifier start or whitespace
                if b >= 0x80 {
                    self.scan_non_ascii(start)
                } else {
                    Token::new(SK::Invalid, start, self.pos)
                }
            }
        })
    }

    // ── Two-character operators ──

    #[inline]
    fn scan_two_char(&mut self, start: u32, second: u8, matched: SK, unmatched: SK) -> Token {
        if self.peek_byte() == Some(second) {
            self.pos += 1;
            Token::new(matched, start, self.pos)
        } else {
            Token::new(unmatched, start, self.pos)
        }
    }

    // ── Dot / DoubleDot / TripleDot ──

    fn scan_dot(&mut self, start: u32) -> Token {
        match self.peek_byte() {
            Some(b'.') => {
                self.pos += 1;
                if self.peek_byte() == Some(b'.') {
                    self.pos += 1;
                    Token::new(SK::TripleDot, start, self.pos)
                } else {
                    Token::new(SK::DoubleDot, start, self.pos)
                }
            }
            Some(b'0'..=b'9') => self.scan_number(start),
            _ => Token::new(SK::Dot, start, self.pos),
        }
    }

    // ── Square bracket / long bracket string ──

    fn scan_open_square_bracket(&mut self, start: u32) -> Token {
        match self.peek_byte() {
            Some(b'[') | Some(b'=') => {
                match self.scan_long_bracket_string(start) {
                    Some(tok) => tok,
                    None => Token::new(SK::LeftSquareBracket, start, start + 1),
                }
            }
            _ => Token::new(SK::LeftSquareBracket, start, self.pos),
        }
    }

    /// Try to scan a long bracket string `[[...]]` or `[=[...]=]`.
    /// Returns None if the opening bracket sequence is invalid (not a long string).
    fn scan_long_bracket_string(&mut self, start: u32) -> Option<Token> {
        let saved_pos = self.pos;
        let first = self.advance();
        let mut level = 0u32;
        if first == b'=' {
            level += 1;
            while self.pos < self.len() && self.byte(self.pos) == b'=' {
                level += 1;
                self.pos += 1;
            }
            if self.pos >= self.len() || self.byte(self.pos) != b'[' {
                // Not a valid long bracket — restore position
                self.pos = saved_pos;
                return None;
            }
            self.pos += 1; // consume the second `[`
        } else if first == b'[' {
            // level 0: `[[`
        } else {
            self.pos = saved_pos;
            return None;
        }

        // Scan until matching closing bracket
        while self.pos < self.len() {
            if self.byte(self.pos) == b']' {
                self.pos += 1;
                let mut closing_level = 0u32;
                while self.pos < self.len() && self.byte(self.pos) == b'=' {
                    closing_level += 1;
                    self.pos += 1;
                }
                if closing_level == level && self.pos < self.len() && self.byte(self.pos) == b']' {
                    self.pos += 1;
                    return Some(Token::new(SK::String, start, self.pos));
                }
                // Not matching — continue scanning
            } else {
                self.pos += 1;
            }
        }
        // Unterminated
        Some(Token::with_error(SK::String, start, self.pos, LexError::UnterminatedString))
    }

    // ── Minus / Comment ──

    fn scan_minus(&mut self, start: u32) -> Token {
        if self.peek_byte() != Some(b'-') {
            return Token::new(SK::Minus, start, self.pos);
        }
        self.pos += 1; // second `-`

        // Check for long bracket comment `--[[...]]`
        if self.peek_byte() == Some(b'[') {
            let bracket_start = self.pos;
            self.pos += 1; // consume `[`
            if let Some(tok) = self.scan_long_bracket_string(bracket_start) {
                let is_unterminated = tok.error.is_some();
                return if is_unterminated {
                    Token::with_error(SK::Comment, start, tok.end, LexError::UnterminatedComment)
                } else {
                    Token::new(SK::Comment, start, tok.end)
                };
            }
            // Not a valid long bracket — fall through to single-line comment
            self.pos = bracket_start;
        }

        // Single-line comment: consume until newline
        while self.pos < self.len() {
            let b = self.byte(self.pos);
            if b == b'\n' || b == b'\r' { break; }
            self.pos += 1;
        }
        Token::new(SK::Comment, start, self.pos)
    }

    // ── Numbers ──

    /// Scan a numeric literal. Called from two entry points:
    /// - `next_token` for `0-9` — `start` is the first digit, `self.pos` is one past it
    /// - `scan_dot` for `.5` — `start` is the `.` position, `self.pos` is one past the `.`
    ///   In this case, the digit-scanning loop below consumes the fractional digits,
    ///   and the decimal-point check at line ~270 is a no-op because `self.pos` is
    ///   already past the `.`.
    fn scan_number(&mut self, start: u32) -> Token {
        let mut valid = true;

        // Check for hex prefix: 0x
        if start < self.pos
            && self.source.get(start as usize) == Some(&b'0')
            && self.peek_byte() == Some(b'x')
        {
            self.pos += 1; // consume 'x'
            while self.pos < self.len() && self.byte(self.pos).is_ascii_hexdigit() {
                self.pos += 1;
            }
            // Check for trailing invalid chars
            while self.pos < self.len() && (self.byte(self.pos).is_ascii_alphanumeric() || self.byte(self.pos) == b'.') {
                valid = false;
                self.pos += 1;
            }
            return if valid {
                Token::new(SK::Number, start, self.pos)
            } else {
                Token::with_error(SK::Number, start, self.pos, LexError::InvalidNumber)
            };
        }

        // Decimal / integer: digits, optional dot, optional exponent
        while self.pos < self.len() && self.byte(self.pos).is_ascii_digit() {
            self.pos += 1;
        }

        // Decimal point
        if self.pos < self.len() && self.byte(self.pos) == b'.' {
            // Only consume if followed by a digit (not `..` operator)
            if self.pos + 1 < self.len() && self.source[(self.pos + 1) as usize].is_ascii_digit() {
                self.pos += 1; // consume '.'
                while self.pos < self.len() && self.byte(self.pos).is_ascii_digit() {
                    self.pos += 1;
                }
            } else if self.source.get(start as usize) == Some(&b'.') {
                // Started with `.` (e.g., `.5`), already consumed digits above
            }
        }

        // Exponent
        if self.pos < self.len() && (self.byte(self.pos) == b'e' || self.byte(self.pos) == b'E') {
            self.pos += 1;
            if self.pos < self.len() && (self.byte(self.pos) == b'+' || self.byte(self.pos) == b'-') {
                self.pos += 1;
            }
            if self.pos >= self.len() || !self.byte(self.pos).is_ascii_digit() {
                valid = false;
            }
            while self.pos < self.len() && self.byte(self.pos).is_ascii_digit() {
                self.pos += 1;
            }
        }

        // Trailing alphanumeric chars are invalid (e.g., `123abc`)
        while self.pos < self.len() && (self.byte(self.pos).is_ascii_alphanumeric() || self.byte(self.pos) == b'.') {
            valid = false;
            self.pos += 1;
        }

        if valid {
            Token::new(SK::Number, start, self.pos)
        } else {
            Token::with_error(SK::Number, start, self.pos, LexError::InvalidNumber)
        }
    }

    // ── Strings ──

    fn scan_string(&mut self, start: u32, quote: u8) -> Token {
        let mut escaped = false;
        while self.pos < self.len() {
            let b = self.byte(self.pos);
            if escaped {
                escaped = false;
                self.pos += 1;
                continue;
            }
            if b == b'\\' {
                escaped = true;
                self.pos += 1;
                continue;
            }
            if b == quote {
                self.pos += 1;
                return Token::new(SK::String, start, self.pos);
            }
            if b == b'\n' || b == b'\r' {
                return Token::with_error(SK::String, start, self.pos, LexError::UnterminatedString);
            }
            self.pos += 1;
        }
        Token::with_error(SK::String, start, self.pos, LexError::UnterminatedString)
    }

    // ── Whitespace (excludes newlines) ──

    fn scan_whitespace(&mut self, start: u32) -> Token {
        while self.pos < self.len() {
            match self.byte(self.pos) {
                b' ' | b'\t' => self.pos += 1,
                _ => break,
            }
        }
        Token::new(SK::Whitespace, start, self.pos)
    }

    // ── Identifiers + keywords ──

    fn scan_identifier(&mut self, start: u32) -> Token {
        while self.pos < self.len() {
            let b = self.byte(self.pos);
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = &self.source[start as usize..self.pos as usize];
        let kind = match text {
            b"and" => SK::AndKeyword,
            b"break" => SK::BreakKeyword,
            b"do" => SK::DoKeyword,
            b"else" => SK::ElseKeyword,
            b"elseif" => SK::ElseIfKeyword,
            b"end" => SK::EndKeyword,
            b"false" => SK::FalseKeyword,
            b"for" => SK::ForKeyword,
            b"function" => SK::FunctionKeyword,
            b"if" => SK::IfKeyword,
            b"in" => SK::InKeyword,
            b"local" => SK::LocalKeyword,
            b"nil" => SK::NilKeyword,
            b"not" => SK::NotKeyword,
            b"or" => SK::OrKeyword,
            b"repeat" => SK::RepeatKeyword,
            b"return" => SK::ReturnKeyword,
            b"then" => SK::ThenKeyword,
            b"true" => SK::TrueKeyword,
            b"until" => SK::UntilKeyword,
            b"while" => SK::WhileKeyword,
            _ => SK::Name,
        };
        Token::new(kind, start, self.pos)
    }

    // ── Non-ASCII handling ──

    fn scan_non_ascii(&mut self, start: u32) -> Token {
        // Decode the first UTF-8 character starting at `start`.
        // If the bytes are invalid UTF-8, emit an Invalid token.
        let text = &self.source[start as usize..];
        let s = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(e) => {
                let valid_len = e.valid_up_to();
                if valid_len == 0 {
                    // First byte is invalid UTF-8 — already consumed by caller
                    return Token::new(SK::Invalid, start, self.pos);
                }
                // Valid prefix exists — use it
                std::str::from_utf8(&text[..valid_len]).unwrap()
            }
        };
        let ch = s.chars().next().unwrap();
        if ch.is_alphabetic() {
            // Non-ASCII identifier — scan remaining alphanumeric/underscore chars
            self.pos = start + ch.len_utf8() as u32;
            while self.pos < self.len() {
                let b = self.byte(self.pos);
                if b.is_ascii_alphanumeric() || b == b'_' {
                    self.pos += 1;
                } else if b >= 0x80 {
                    // Try to decode another UTF-8 char
                    let rest = &self.source[self.pos as usize..];
                    if let Ok(s) = std::str::from_utf8(rest)
                        && let Some(c) = s.chars().next()
                            && c.is_alphanumeric() {
                                self.pos += c.len_utf8() as u32;
                                continue;
                            }
                    break;
                } else {
                    break;
                }
            }
            Token::new(SK::Name, start, self.pos)
        } else if ch.is_whitespace() {
            self.pos = start + ch.len_utf8() as u32;
            Token::new(SK::Whitespace, start, self.pos)
        } else {
            self.pos = start + ch.len_utf8() as u32;
            Token::new(SK::Invalid, start, self.pos)
        }
    }
}
