use crate::{lex::get_lexer, syntax_kind};
use std::fs;

#[derive(Debug, Clone, Copy)]
pub struct GroundedToken {
    pub start: usize,
    pub end: usize,
    pub kind: syntax_kind::SyntaxKind,
}

pub fn parse_file(filename: &str) -> Result<Vec<GroundedToken>, std::io::Error> {
    let (lexer, reverse) = get_lexer();
    let text = fs::read_to_string(filename)? ;
    let tokens = lexer.tokenize(text.as_str());
    let mut res = Vec::new();
    let mut position: usize = 0;
    for t in tokens {
        res.push(GroundedToken {
            start: position,
            end: position + t.len,
            kind: reverse[&t.kind.0]
        });
        position += t.len;
    };
    Ok(res)
}
