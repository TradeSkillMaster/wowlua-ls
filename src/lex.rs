use std::collections::HashMap;

use m_lexer::*;

use crate::syntax_kind::SyntaxKind;
use crate::syntax_kind::SyntaxKind::*;

pub fn get_lexer() -> (Lexer, HashMap<u16, SyntaxKind>) {
    let mut builder = LexerBuilder::new();
    builder = builder.error_token(TokenKind::from(NONE));
    let mut reverse: HashMap<u16, SyntaxKind> = HashMap::new();

    for (kind, re) in [
        (WHITESPACE, r"\s"),
        (PLUS, r"\+"),
        (MINUS, r"\-"),
        (ASTERISK, r"\*"),
        (SLASH, r"/"),
        (MODULO, r"%"),
        (HAT, r"\^"),
        (HASH, r"#"),
        (EQUALS, r"="),
        (LESS_THAN, r"<"),
        (GREATER_THAN, r">"),
        (L_BRACKET, r"\("),
        (R_BRACKET, r"\)"),
        (L_CURLY_BRACKET, r"\{"),
        (R_CURLY_BRACKET, r"\}"),
        (L_SQUARE_BRACKET, r"\["),
        (R_SQUARE_BRACKET, r"\]"),
        (SEMICOLON, r";"),
        (COLON, r":"),
        (COMMA, r","),
        (ONE_DOT, r"\."),
        (TILDE, r"~"),
        (AND_KEYWORD, "and"),
        (BREAK_KEYWORD, "break"),
        (DO_KEYWORD, "do"),
        (ELSE_KEYWORD, "else"),
        (ELSEIF_KEYWORD, "elseif"),
        (END_KEYWORD, "end"),
        (FALSE_KEYWORD, "false"),
        (FOR_KEYWORD, "for"),
        (FUNCTION_KEYWORD, "function"),
        (IF_KEYWORD, "if"),
        (IN_KEYWORD, "in"),
        (LOCAL_KEYWORD, "local"),
        (NIL_KEYWORD, "nil"),
        (NOT_KEYWORD, "not"),
        (OR_KEYWORD, "or"),
        (REPEAT_KEYWORD, "repeat"),
        (RETURN_KEYWORD, "return"),
        (THEN_KEYWORD, "then"),
        (TRUE_KEYWORD, "true"),
        (UNTIL_KEYWORD, "until"),
        (WHILE_KEYWORD, "while"),
        (STRING, r#"\"[^\n]-\""#),
        (STRING, r#"\[(=+)\[.-\](=+)\]"#),
        (NAME, r"[\w_]+"),
        (WHITESPACE, r"\s+")
    ] {
        let k = TokenKind::from(kind);
        builder = builder.token(k, re);
        reverse.insert(k.0, kind);
    }

    (builder.build(), reverse)
}
