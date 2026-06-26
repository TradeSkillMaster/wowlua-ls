use crate::analysis::AnalysisResult;
use crate::types::SymbolIdentifier;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::syntax::tree::{SyntaxToken, SyntaxTree};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, message: String, start: usize, end: usize) {
    super::MALFORMED_ANNOTATION.emit(diags, message, start, end);
}

/// Check whether the next meaningful token after `tok` is a `---|` continuation comment.
/// Skips whitespace, newlines, and regular (non-doc) comments so that e.g.
/// `---@event Foo` / `-- description` / `---|"A"` still detects the continuation.
fn next_token_is_continuation(tok: &SyntaxToken<'_>) -> bool {
    let mut next = tok.next_token();
    while let Some(ref t) = next {
        let k = t.kind();
        if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline {
            next = t.next_token();
            continue;
        }
        if k == SyntaxKind::Comment {
            let text = t.text();
            if text.starts_with("---|") {
                return true;
            }
            // Skip regular comments (not doc annotations) between the tag and ---|
            if !text.starts_with("---") {
                next = t.next_token();
                continue;
            }
        }
        return false;
    }
    false
}

const KNOWN_TAGS: &[&str] = &[
    "class", "field", "alias", "param", "return", "type", "enum",
    "meta", "overload", "defclass", "deprecated", "nodiscard", "constructor",
    "generic", "private", "protected", "accessor", "diagnostic",
    "builds-field", "built-name", "built-extends", "type-narrows", "narrows-arg",
    "creates-global", "generates-events", "callback-event-arg", "correlated", "flavor-narrows", "event", "requires",
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
                let r = crate::annotations::strip_class_modifier(rest);
                let name = r.split(|c: char| c.is_whitespace() || c == '<' || c == ':').next().unwrap_or("");
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
                    let r = crate::annotations::strip_class_modifier(rest);
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
                    if next_token_is_continuation(&tok) { None }
                    else { Some("@alias requires a type after the alias name".to_string()) }
                }
                "cast" if rest.is_empty() =>
                    Some("@cast requires a variable name and type".to_string()),
                "cast" if !rest.contains(char::is_whitespace) =>
                    Some("@cast requires a type after the variable name".to_string()),
                "type" if rest.is_empty() =>
                    Some("@type requires a type".to_string()),
                "return" if rest.is_empty() => {
                    if next_token_is_continuation(&tok) { None }
                    else { Some("@return requires a type".to_string()) }
                }
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
                "narrows-arg" => {
                    if rest.is_empty() {
                        Some("@narrows-arg requires a parameter index (e.g. @narrows-arg 1)".to_string())
                    } else if let Ok(idx) = rest.trim().parse::<usize>() {
                        if idx == 0 {
                            Some("@narrows-arg parameter index must be >= 1 (1-based)".to_string())
                        } else {
                            None
                        }
                    } else {
                        Some("@narrows-arg requires a numeric parameter index (e.g. @narrows-arg 1)".to_string())
                    }
                }
                "creates-global" => {
                    // `@creates-global N` — N (1-based) names the param whose string
                    // literal becomes the created global. The type is inferred from
                    // the call, so there is no second token to validate.
                    match rest.split_whitespace().next() {
                        None => Some("@creates-global requires a parameter index (e.g. @creates-global 2)".to_string()),
                        Some(tok) => match tok.parse::<usize>() {
                            Ok(0) => Some("@creates-global parameter index must be >= 1 (1-based)".to_string()),
                            Ok(_) => None,
                            Err(_) => Some("@creates-global requires a numeric parameter index (e.g. @creates-global 2)".to_string()),
                        },
                    }
                }
                "callback-event-arg" => {
                    match rest.split_whitespace().next() {
                        None => Some("@callback-event-arg requires a 1-based argument index (e.g. @callback-event-arg 1)".to_string()),
                        Some(tok) => match tok.parse::<usize>() {
                            Ok(0) => Some("@callback-event-arg argument index must be >= 1 (1-based)".to_string()),
                            Ok(_) => None,
                            Err(_) => Some("@callback-event-arg requires a numeric argument index (e.g. @callback-event-arg 1)".to_string()),
                        },
                    }
                }
                "generates-events" => {
                    // `@generates-events N [Field]` — N (1-based) is the call argument
                    // holding the event array; the optional Field (default `Event`) is
                    // the synthesized table name.
                    match rest.split_whitespace().next() {
                        None => Some("@generates-events requires a parameter index (e.g. @generates-events 1)".to_string()),
                        Some(tok) => match tok.parse::<usize>() {
                            Ok(0) => Some("@generates-events parameter index must be >= 1 (1-based)".to_string()),
                            Ok(_) => None,
                            Err(_) => Some("@generates-events requires a numeric parameter index (e.g. @generates-events 1)".to_string()),
                        },
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
                "requires" => {
                    if rest.is_empty() {
                        Some("@requires requires a type parameter and constraint (e.g. @requires T: boolean)".to_string())
                    } else {
                        let bad = rest.split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .any(|part| match part.split_once(':') {
                                Some((n, c)) => n.trim().is_empty() || c.trim().is_empty(),
                                None => true,
                            });
                        if bad {
                            Some("@requires entries must have the form 'T: Constraint' (e.g. @requires T: boolean)".to_string())
                        } else {
                            None
                        }
                    }
                }
                "event" => {
                    if rest.is_empty() {
                        Some("@event requires a type and event name (e.g. @event MyEvent \"EVENT_NAME\")".to_string())
                    } else if !rest.contains(char::is_whitespace) {
                        // Batch form: @event TypeName followed by ---| lines
                        if next_token_is_continuation(&tok) { None }
                        else { Some("@event requires an event name after the type (e.g. @event MyEvent \"EVENT_NAME\")".to_string()) }
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
            } else if tag == "correlated" && current_class.is_none() && !rest.is_empty() {
                // Validate local variable names for standalone @correlated
                if let Some(scope_idx) = analysis.scope_at_offset(tok_start as u32) {
                    let rest_offset = tok_start + 4 + tag.len() + (after_at[tag.len()..].len() - rest.len());
                    for segment in rest.split(',') {
                        let var_name = segment.trim();
                        if var_name.is_empty() { continue; }
                        let id = SymbolIdentifier::Name(var_name.to_string());
                        if analysis.get_symbol(&id, scope_idx).is_none() {
                            let seg_start_in_rest = segment.as_ptr() as usize - rest.as_ptr() as usize;
                            let trim_offset = segment.len() - segment.trim_start().len();
                            let field_start = rest_offset + seg_start_in_rest + trim_offset;
                            let field_end = field_start + var_name.len();
                            check(
                                diags,
                                format!("@correlated references unknown variable '{}'", var_name),
                                field_start, field_end,
                            );
                        }
                    }
                }
            }
        }
    }
}
