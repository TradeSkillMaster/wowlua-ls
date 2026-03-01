use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::variables::ValueType;

// ── Annotation types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AnnotationType {
    Simple(String),
    Union(Vec<AnnotationType>),
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationBlock {
    pub params: Vec<(String, AnnotationType)>,
    pub returns: Vec<AnnotationType>,
    pub var_type: Option<AnnotationType>,
    pub class: Option<String>,
    pub class_parents: Vec<String>,
    pub fields: Vec<(String, AnnotationType)>,
    pub alias: Option<(String, AnnotationType)>,
    pub overloads: Vec<String>,
    pub meta: bool,
    pub deprecated: bool,
    pub nodiscard: bool,
}

// ── Comment extraction ───────────────────────────────────────────────────────

/// Extract LuaLS-style annotations from comments preceding a syntax node.
///
/// Walks backward through the token stream from the node's start position,
/// collecting `---@` comment lines. This approach works regardless of which
/// parent node the trivia tokens are attached to (rowan attaches trailing
/// trivia to the preceding construct, so comments before a function can end
/// up inside the preceding statement's expression list).
pub fn extract_annotations(node: &SyntaxNode) -> AnnotationBlock {
    // Find the first token of our node, then walk backward through preceding tokens
    let first_token = match node.first_token() {
        Some(t) => t,
        None => return AnnotationBlock::default(),
    };

    let mut annotation_lines = Vec::new();
    let mut tok = first_token.prev_token();
    while let Some(token) = tok {
        let kind = token.kind();
        if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
            tok = token.prev_token();
            continue;
        }
        if kind == SyntaxKind::Comment {
            let text = token.text();
            if text.starts_with("---@") {
                annotation_lines.push(text.to_string());
                tok = token.prev_token();
                continue;
            } else if text.starts_with("---") {
                // Plain doc comment (no @), skip but keep looking
                tok = token.prev_token();
                continue;
            }
        }
        // Non-trivia, non-annotation token — stop
        break;
    }

    annotation_lines.reverse();
    parse_annotation_lines(&annotation_lines)
}

/// Scan all comments in the syntax tree for @class and @alias declarations.
/// Returns (class_blocks, alias_blocks, has_meta).
pub fn scan_all_annotations(root: &SyntaxNode) -> (
    Vec<(String, Vec<String>, Vec<(String, AnnotationType)>)>,
    Vec<(String, AnnotationType)>,
    bool,
) {
    let mut classes = Vec::new();
    let mut aliases = Vec::new();
    let mut has_meta = false;

    let mut current_group: Vec<String> = Vec::new();
    let mut token = root.first_token();
    let mut prev_was_newline = false;

    while let Some(tok) = token {
        let kind = tok.kind();
        if kind == SyntaxKind::Comment {
            let text = tok.text();
            if text.starts_with("---@") {
                current_group.push(text.to_string());
            }
            prev_was_newline = false;
        } else if kind == SyntaxKind::Newline {
            // Blank line (two newlines in a row) splits annotation groups
            if prev_was_newline && !current_group.is_empty() {
                flush_group(&current_group, &mut classes, &mut aliases, &mut has_meta);
                current_group.clear();
            }
            prev_was_newline = true;
        } else if kind == SyntaxKind::Whitespace {
            // Don't reset prev_was_newline for whitespace
        } else {
            // Non-trivia token — flush the current group
            flush_group(&current_group, &mut classes, &mut aliases, &mut has_meta);
            current_group.clear();
            prev_was_newline = false;
        }
        token = tok.next_token();
    }
    // Flush final group
    flush_group(&current_group, &mut classes, &mut aliases, &mut has_meta);

    (classes, aliases, has_meta)
}

fn flush_group(
    lines: &[String],
    classes: &mut Vec<(String, Vec<String>, Vec<(String, AnnotationType)>)>,
    aliases: &mut Vec<(String, AnnotationType)>,
    has_meta: &mut bool,
) {
    if lines.is_empty() {
        return;
    }
    let block = parse_annotation_lines(lines);
    if block.meta {
        *has_meta = true;
    }
    if let Some(class_name) = block.class {
        classes.push((class_name, block.class_parents, block.fields));
    }
    if let Some(alias) = block.alias {
        aliases.push(alias);
    }
}

// ── Line parsing ─────────────────────────────────────────────────────────────

fn parse_annotation_lines(lines: &[String]) -> AnnotationBlock {
    let mut block = AnnotationBlock::default();

    for line in lines {
        let content = line.trim_start_matches('-');
        let content = content.trim();
        if let Some(rest) = content.strip_prefix("@class") {
            let rest = rest.trim();
            if let Some(class_name) = rest.split_whitespace().next() {
                let class_name = class_name.trim_end_matches(':');
                block.class = Some(class_name.to_string());
                // Parse parents after ":"  e.g. "@class Frame : Region, ScriptObject"
                if let Some((_, parents_str)) = rest.split_once(':') {
                    for parent in parents_str.split(',') {
                        let parent = parent.trim();
                        if !parent.is_empty() {
                            block.class_parents.push(parent.to_string());
                        }
                    }
                }
            }
        } else if let Some(rest) = content.strip_prefix("@field") {
            let rest = rest.trim();
            if let Some((name, type_str)) = rest.split_once(char::is_whitespace) {
                let typ = parse_type(type_str.trim());
                block.fields.push((name.to_string(), typ));
            }
        } else if let Some(rest) = content.strip_prefix("@alias") {
            let rest = rest.trim();
            if let Some((name, type_str)) = rest.split_once(char::is_whitespace) {
                let typ = parse_type(type_str.trim());
                block.alias = Some((name.to_string(), typ));
            }
        } else if let Some(rest) = content.strip_prefix("@param") {
            let rest = rest.trim();
            if let Some((name, type_str)) = rest.split_once(char::is_whitespace) {
                let name = name.trim_end_matches('?'); // strip optional marker
                let typ = parse_type(type_str.trim());
                block.params.push((name.to_string(), typ));
            }
        } else if let Some(rest) = content.strip_prefix("@return") {
            let rest = rest.trim();
            for type_str in rest.split(',') {
                let type_str = type_str.trim();
                if !type_str.is_empty() {
                    // Take first token as type, rest is optional return name
                    let type_only = type_str.split_whitespace().next().unwrap_or(type_str);
                    block.returns.push(parse_type(type_only));
                }
            }
        } else if let Some(rest) = content.strip_prefix("@type") {
            let rest = rest.trim();
            if !rest.is_empty() {
                block.var_type = Some(parse_type(rest));
            }
        } else if let Some(rest) = content.strip_prefix("@enum") {
            // Treat @enum as a class — fields come from the table constructor
            let rest = rest.trim();
            if let Some(name) = rest.split_whitespace().next() {
                block.class = Some(name.to_string());
            }
        } else if content.starts_with("@meta") {
            block.meta = true;
        } else if let Some(rest) = content.strip_prefix("@overload") {
            let rest = rest.trim();
            if !rest.is_empty() {
                block.overloads.push(rest.to_string());
            }
        } else if content.starts_with("@deprecated") {
            block.deprecated = true;
        } else if content.starts_with("@nodiscard") {
            block.nodiscard = true;
        }
    }

    block
}

fn parse_type(s: &str) -> AnnotationType {
    let s = s.trim();

    if let Some(base) = s.strip_suffix('?') {
        let base_type = parse_type(base);
        return AnnotationType::Union(vec![base_type, AnnotationType::Simple("nil".to_string())]);
    }

    // Handle unions: string|number
    if s.contains('|') {
        let parts: Vec<AnnotationType> = s.split('|')
            .map(|p| parse_type(p.trim()))
            .collect();
        if parts.len() == 1 {
            return parts.into_iter().next().unwrap();
        }
        return AnnotationType::Union(parts);
    }

    AnnotationType::Simple(s.to_string())
}

// ── Type conversion ──────────────────────────────────────────────────────────

/// Convert an annotation type to a ValueType (primitives only).
/// For class/alias-aware resolution, use Variables::resolve_annotation_type instead.
pub fn annotation_type_to_value_type(at: &AnnotationType) -> Option<ValueType> {
    match at {
        AnnotationType::Simple(name) => match name.as_str() {
            "nil" => Some(ValueType::Nil),
            "boolean" | "bool" => Some(ValueType::Boolean(None)),
            "number" | "integer" => Some(ValueType::Number),
            "string" => Some(ValueType::String),
            "table" => Some(ValueType::Table(None)),
            "function" | "fun" => Some(ValueType::Function(None)),
            "any" => None,
            _ => None,
        },
        AnnotationType::Union(parts) => {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(annotation_type_to_value_type)
                .collect();
            match converted.len() {
                0 => None,
                1 => converted.into_iter().next(),
                _ => {
                    let mut iter = converted.into_iter();
                    let mut result = iter.next().unwrap();
                    for vt in iter {
                        result = ValueType::union(result, vt);
                    }
                    Some(result)
                }
            }
        }
    }
}
