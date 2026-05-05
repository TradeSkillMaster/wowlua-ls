use crate::analysis::AnalysisResult;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, message: String, start: usize, end: usize) {
    super::MALFORMED_ANNOTATION.emit(diags, message, start, end);
}

/// What the scanner just passed over — tracks whether whitespace should end the type expression.
#[derive(Clone, Copy, PartialEq)]
enum After { None, Colon, Comma, Pipe, Ampersand }

fn has_top_level_comma(s: &str) -> bool {
    let mut depth = 0usize;
    let mut in_fun_ret = false;
    let mut after = After::None;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                if !in_double_quote { after = After::None; }
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                if !in_single_quote { after = After::None; }
            }
            _ if in_single_quote || in_double_quote => {}
            '<' | '(' | '{' => { depth += 1; in_fun_ret = false; after = After::None; }
            '>' | '}' => { depth = depth.saturating_sub(1); after = After::None; }
            ')' => {
                depth = depth.saturating_sub(1);
                after = After::None;
                if depth == 0 {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                    if j < bytes.len() && bytes[j] == b':' {
                        in_fun_ret = true;
                    }
                }
            }
            '|' if depth == 0 => { after = After::Pipe; }
            '&' if depth == 0 => { after = After::Ampersand; }
            ':' if depth == 0 => { after = After::Colon; }
            ',' if depth == 0 && !in_fun_ret => return true,
            ',' if depth == 0 => { after = After::Comma; }
            c if c.is_whitespace() && depth == 0 && after == After::None => {
                // End of the type expression — description follows.
                // Unless the next non-space char continues the type (| or &).
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                if j >= bytes.len() || (bytes[j] != b'|' && bytes[j] != b'&') {
                    return false;
                }
            }
            _ => { after = After::None; }
        }
    }
    false
}

const KNOWN_TAGS: &[&str] = &[
    "class", "field", "alias", "param", "return", "type", "enum",
    "meta", "overload", "defclass", "deprecated", "nodiscard", "constructor",
    "generic", "private", "protected", "accessor", "diagnostic",
    "builds-field", "built-name", "built-extends", "type-narrows",
    "correlated", "flavor-narrows", "event",
    "see", "vararg", "as", "cast", "operator", "module", "source",
    "version", "package", "async", "nodoc", "public",
];

pub(crate) struct MalformedAnnotation;

impl DiagnosticPass for MalformedAnnotation {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let mut current_class: Option<&str> = None;

        for event in SyntaxNode::new_root(tree).descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment {
                // Reset class tracking when we leave a comment block
                if tok.kind() != SyntaxKind::Whitespace && tok.kind() != SyntaxKind::Newline {
                    current_class = None;
                }
                continue;
            }
            let text = tok.text();
            let Some(after_at) = text.strip_prefix("---@") else { continue };
            // Skip @diagnostic — handled by unknown_diag_code::run
            if after_at.starts_with("diagnostic") { continue; }

            let r = tok.text_range();
            let tok_start = u32::from(r.start()) as usize;
            let tok_end = u32::from(r.end()) as usize;

            let tag = after_at.split(|c: char| c.is_whitespace()).next().unwrap_or("");
            if tag.is_empty() { continue; }

            if !KNOWN_TAGS.contains(&tag) {
                let tag_start = tok_start + 4;
                let tag_end = tag_start + tag.len();
                check(diags, format!("unknown annotation '@{}'", tag), tag_start, tag_end);
                continue;
            }

            let rest = after_at[tag.len()..].trim();

            // Track the current @class/@enum for @correlated field validation
            if (tag == "class" || tag == "enum") && !rest.is_empty() {
                let name = rest.split(|c: char| c.is_whitespace() || c == '<' || c == ':').next().unwrap_or("");
                if !name.is_empty() {
                    current_class = Some(name);
                }
            }

            let msg = match tag {
                "class" | "enum" if rest.is_empty() || rest.split_whitespace().next().is_none() =>
                    Some(format!("@{} requires a name", tag)),
                "class" | "enum" => {
                    // Check for text after class name without a colon separator
                    // e.g. `@class Foo table<K,V>` instead of `@class Foo : table<K,V>`
                    let r = rest.strip_prefix("(partial)").map(|s| s.trim_start())
                        .or_else(|| rest.strip_prefix("(exact)").map(|s| s.trim_start()))
                        .unwrap_or(rest);
                    // Find end of class name, handling type params like `Name<K,V>`
                    let name_end = if let Some(open) = r.find('<') {
                        let first_sep = r.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(usize::MAX);
                        if open < first_sep {
                            // `<` belongs to the class name's type params
                            if let Some(close) = r[open..].find('>') {
                                open + close + 1
                            } else {
                                r.find(char::is_whitespace).unwrap_or(r.len())
                            }
                        } else {
                            first_sep.min(r.len())
                        }
                    } else {
                        r.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(r.len())
                    };
                    let class_name = r[..name_end].trim_end_matches(':');
                    if class_name.contains("[]") {
                        Some(format!("@{} name '{}' looks like a type expression; did you mean '@type {}'?", tag, class_name, class_name))
                    } else {
                        let after = r[name_end..].trim();
                        if !after.is_empty() && !after.starts_with(':') {
                            Some(format!("@{} parent type requires ':' separator (e.g. @{} Name : Parent)", tag, tag))
                        } else {
                            None
                        }
                    }
                }
                "param" if rest.is_empty() =>
                    Some("@param requires a name and type".to_string()),
                "param" if !rest.contains(char::is_whitespace) =>
                    Some("@param requires a type after the parameter name".to_string()),
                "field" => {
                    let rest = rest.strip_prefix("private").map(|r| r.trim_start())
                        .or_else(|| rest.strip_prefix("protected").map(|r| r.trim_start()))
                        .or_else(|| rest.strip_prefix("public").map(|r| r.trim_start()))
                        .unwrap_or(rest);
                    if rest.is_empty() {
                        Some("@field requires a name and type".to_string())
                    } else if !rest.contains(char::is_whitespace) {
                        Some("@field requires a type after the field name".to_string())
                    } else {
                        None
                    }
                }
                "alias" if rest.is_empty() =>
                    Some("@alias requires a name and type".to_string()),
                "alias" if !rest.contains(char::is_whitespace) => {
                    let has_continuation = {
                        let mut next = tok.next_token();
                        let mut found = false;
                        while let Some(ref t) = next {
                            let k = t.kind();
                            if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline {
                                next = t.next_token();
                                continue;
                            }
                            if k == SyntaxKind::Comment && t.text().starts_with("---|") {
                                found = true;
                            }
                            break;
                        }
                        found
                    };
                    if has_continuation { None }
                    else { Some("@alias requires a type after the alias name".to_string()) }
                }
                "cast" if rest.is_empty() =>
                    Some("@cast requires a variable name and type".to_string()),
                "cast" if !rest.contains(char::is_whitespace) =>
                    Some("@cast requires a type after the variable name".to_string()),
                "type" if rest.is_empty() =>
                    Some("@type requires a type".to_string()),
                "return" if rest.is_empty() =>
                    Some("@return requires a type".to_string()),
                "return" if has_top_level_comma(rest) =>
                    Some("comma-separated return types are not supported; use separate @return lines for each return value".to_string()),
                "overload" if rest.is_empty() =>
                    Some("@overload requires a 'fun(...)' signature".to_string()),
                "overload" if !rest.starts_with("fun(") =>
                    Some("@overload requires a 'fun(...)' signature".to_string()),
                "builds-field" => {
                    if rest.is_empty() {
                        Some("@builds-field requires a parameter index and type (e.g. @builds-field 1 string)".to_string())
                    } else if !rest.contains(char::is_whitespace) {
                        if rest.parse::<usize>().is_err() {
                            Some("@builds-field requires a numeric parameter index (e.g. @builds-field 1 string)".to_string())
                        } else {
                            Some("@builds-field requires a type after the parameter index (e.g. @builds-field 1 string)".to_string())
                        }
                    } else {
                        let idx_str = rest.split_whitespace().next().unwrap_or("");
                        if idx_str.parse::<usize>().is_err() {
                            Some("@builds-field requires a numeric parameter index (e.g. @builds-field 1 string)".to_string())
                        } else if idx_str == "0" {
                            Some("@builds-field parameter index must be >= 1 (1-based)".to_string())
                        } else {
                            None
                        }
                    }
                }
                "built-name" => {
                    if rest.is_empty() {
                        Some("@built-name requires a parameter index (e.g. @built-name 1)".to_string())
                    } else if let Ok(idx) = rest.trim().parse::<usize>() {
                        if idx == 0 {
                            Some("@built-name parameter index must be >= 1 (1-based)".to_string())
                        } else {
                            None
                        }
                    } else {
                        Some("@built-name requires a numeric parameter index (e.g. @built-name 1)".to_string())
                    }
                }
                "correlated" => {
                    if rest.is_empty() {
                        Some("@correlated requires at least two field names (e.g. @correlated field1, field2)".to_string())
                    } else {
                        let names: Vec<&str> = rest.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
                        if names.len() < 2 {
                            Some("@correlated requires at least two field names (e.g. @correlated field1, field2)".to_string())
                        } else {
                            None
                        }
                    }
                }
                "flavor-narrows" => {
                    if rest.is_empty() {
                        Some("@flavor-narrows requires one or more flavor names (e.g. @flavor-narrows retail, classic)".to_string())
                    } else {
                        let unknown: Vec<&str> = rest.split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty() && crate::flavor::parse_flavor_name(s).is_none())
                            .collect();
                        if !unknown.is_empty() {
                            Some(format!("@flavor-narrows has unknown flavor name(s): {}", unknown.join(", ")))
                        } else {
                            None
                        }
                    }
                }
                "event" => {
                    if rest.is_empty() {
                        Some("@event requires a type and event name (e.g. @event MyEvent \"EVENT_NAME\")".to_string())
                    } else if !rest.contains(char::is_whitespace) {
                        Some("@event requires an event name after the type (e.g. @event MyEvent \"EVENT_NAME\")".to_string())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(message) = msg {
                let tag_start = tok_start + 4;
                let tag_end = tag_start + tag.len();
                check(diags, message, tag_start, std::cmp::min(tag_end, tok_end));
            } else if tag == "correlated"
                && let Some(class_name) = current_class
                && let Some(&table_idx) = analysis.ir.classes.get(class_name)
            {
                let rest_offset = tok_start + 4 + tag.len() + (after_at[tag.len()..].len() - rest.len());
                for segment in rest.split(',') {
                    let field_name = segment.trim();
                    if field_name.is_empty() { continue; }
                    if !analysis.class_has_field(table_idx, field_name) {
                        let seg_start_in_rest = segment.as_ptr() as usize - rest.as_ptr() as usize;
                        let trim_offset = segment.len() - segment.trim_start().len();
                        let field_start = rest_offset + seg_start_in_rest + trim_offset;
                        let field_end = field_start + field_name.len();
                        check(
                            diags,
                            format!("@correlated references unknown field '{}' on class '{}'", field_name, class_name),
                            field_start, field_end,
                        );
                    }
                }
            }
        }
    }
}
