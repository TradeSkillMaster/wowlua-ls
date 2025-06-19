#![allow(bad_style, unused)]
#[repr(u16)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SyntaxKind {
    NONE,
    WHITESPACE,
    //Punctuation
    PLUS,
    MINUS,
    ASTERISK,
    SLASH,
    MODULO,
    HAT,
    HASH,
    EQUALS,
    LESS_THAN,
    GREATER_THAN,
    L_BRACKET,
    R_BRACKET,
    L_CURLY_BRACKET,
    R_CURLY_BRACKET,
    L_SQUARE_BRACKET,
    R_SQUARE_BRACKET,
    SEMICOLON,
    COLON,
    COMMA,
    ONE_DOT,
    TILDE,
    //Combined punctuation
    EQUALS_BOOLEAN,
    NOT_EQUALS_BOOLEAN,
    LESS_THAN_EQUALS,
    GREATER_THAN_EQUALS,
    TWO_DOTS,
    THREE_DOTS,
    // Keywords
    AND_KEYWORD,
    BREAK_KEYWORD,
    DO_KEYWORD,
    ELSE_KEYWORD,
    ELSEIF_KEYWORD,
    END_KEYWORD,
    FALSE_KEYWORD,
    FOR_KEYWORD,
    FUNCTION_KEYWORD,
    IF_KEYWORD,
    IN_KEYWORD,
    LOCAL_KEYWORD,
    NIL_KEYWORD,
    NOT_KEYWORD,
    OR_KEYWORD,
    REPEAT_KEYWORD,
    RETURN_KEYWORD,
    THEN_KEYWORD,
    TRUE_KEYWORD,
    UNTIL_KEYWORD,
    WHILE_KEYWORD,
    //Literals
    STRING,
    NUMBER,
    TABLE_INIT,
    //System
    CHUNK,
    STAT,
    VAR_LIST,
    EXP_LIST,
    ASSIGN_STAT,
    LOCAL_STAT,
    WHILE_STAT,
    REPEAT_STAT,
    IF_STAT,
    RETURN_STAT,
    BREAK_STAT,
    FOR_STAT_COUNTER,
    FOR_STAT_ITER,
    FUNCTION_CALL_STAT,
    //Expressions
    EXP,
    PREFIX_EXP,
    //Functions
    FUNCTION_CALL,
    FUNCTION_CALL_STRING,
    FUNCTION_CALL_TABLE_INIT,
    FUNCTION,
    FUNCTION_BODY,
    GLOBAL_FUNCTION_STAT,
    LOCALE_FUNCTION_STAT,
    FUNCTION_NAME,
    //Other
    NAME,
}

use self::SyntaxKind::*;
impl SyntaxKind {
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            AND_KEYWORD |
            BREAK_KEYWORD |
            DO_KEYWORD |
            ELSE_KEYWORD |
            ELSEIF_KEYWORD |
            END_KEYWORD |
            FALSE_KEYWORD |
            FOR_KEYWORD |
            FUNCTION_KEYWORD |
            IF_KEYWORD |
            IN_KEYWORD |
            LOCAL_KEYWORD |
            NIL_KEYWORD |
            NOT_KEYWORD |
            OR_KEYWORD |
            REPEAT_KEYWORD |
            RETURN_KEYWORD |
            THEN_KEYWORD |
            TRUE_KEYWORD |
            UNTIL_KEYWORD |
            WHILE_KEYWORD
        )
    }
    pub fn is_punct(self) -> bool {
        matches!(
            self,
            PLUS |
            MINUS |
            ASTERISK |
            SLASH |
            MODULO |
            HAT |
            HASH |
            TILDE |
            LESS_THAN |
            GREATER_THAN |
            EQUALS |
            L_BRACKET |
            R_BRACKET |
            L_CURLY_BRACKET |
            R_CURLY_BRACKET |
            L_SQUARE_BRACKET |
            R_SQUARE_BRACKET |
            SEMICOLON |
            COLON |
            COMMA |
            ONE_DOT
        )
    }
    pub fn is_literal(self) -> bool {
        matches!(self,
            STRING |
            NUMBER
        )
    }
    pub fn from_keyword(kw: &str) -> Option<SyntaxKind> {
        let res = match kw {
            "and" => AND_KEYWORD,
            "break" => BREAK_KEYWORD,
            "do" => DO_KEYWORD,
            "else" => ELSE_KEYWORD,
            "elseif" => ELSEIF_KEYWORD,
            "end" => END_KEYWORD,
            "false" => FALSE_KEYWORD,
            "for" => FOR_KEYWORD,
            "function" => FUNCTION_KEYWORD,
            "if" => IF_KEYWORD,
            "in" => IN_KEYWORD,
            "local" => LOCAL_KEYWORD,
            "nil" => NIL_KEYWORD,
            "not" => NOT_KEYWORD,
            "or" => OR_KEYWORD,
            "repeat" => REPEAT_KEYWORD,
            "return" => RETURN_KEYWORD,
            "then" => THEN_KEYWORD,
            "true" => TRUE_KEYWORD,
            "until" => UNTIL_KEYWORD,
            "while" => WHILE_KEYWORD,
            _ => return None,
        };
        Some(res)
    }
    pub fn from_operator(op: &str) -> Option<SyntaxKind> {
        let res = match op {
            "+" => PLUS,
            "-" => MINUS,
            "*" => ASTERISK,
            "/" => SLASH,
            "%" => MODULO,
            "^" => HAT,
            "#" => HASH,
            "~" => TILDE,
            "<" => LESS_THAN,
            ">" => GREATER_THAN,
            "=" => EQUALS,
            "(" => L_BRACKET,
            ")" => R_BRACKET,
            "{" => L_CURLY_BRACKET,
            "}" => R_CURLY_BRACKET,
            "[" => L_SQUARE_BRACKET,
            "]" => R_SQUARE_BRACKET,
            ";" => SEMICOLON,
            ":" => COLON,
            "," => COMMA,
            "." => ONE_DOT,
            _ => return None
        };
        Some(res)
    }
}

pub fn from_string(s: &str) -> SyntaxKind {
    match SyntaxKind::from_keyword(s).or(SyntaxKind::from_operator(s)) {
        Some(s) => s,
        None => NONE,
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

impl From<SyntaxKind> for m_lexer::TokenKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}
