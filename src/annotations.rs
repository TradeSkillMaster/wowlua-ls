use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::ast::{AstNode, Block, Statement, Expression, FunctionCall};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::types::ValueType;

// ── Annotation types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationType {
    Simple(String),
    Union(Vec<AnnotationType>),
    Array(Box<AnnotationType>),                  // T[], integer[]
    Parameterized(String, Vec<AnnotationType>),  // table<K, V>
    Backtick(Box<AnnotationType>),               // `T` — infer from string literal as class name
    Fun(Vec<ParamInfo>, Vec<AnnotationType>, bool), // fun(x: T): R — params, returns, is_vararg
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    pub typ: AnnotationType,
    pub optional: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CastMode {
    Replace,  // ---@cast x string
    Add,      // ---@cast x +string
    Remove,   // ---@cast x -string
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub parents: Vec<String>,
    pub fields: Vec<(String, AnnotationType, Visibility)>,
    pub accessors: Vec<(String, Visibility)>,
    pub overloads: Vec<OverloadSig>,
    pub generics: Vec<(String, Option<String>)>,
    /// For defclass-scanned classes: maps constraint parent name → resolved type arg values.
    /// E.g. for `@generic T: Class<P>` with P=Animal → [("Class", ["Animal"])]
    pub constructor_methods: Vec<String>,
    pub constraint_type_arg_subs: Vec<(String, Vec<String>)>,
    /// Maps class field name → @built-name class name for class-level static fields.
    /// Used during inheritance to substitute parent built types with child overrides.
    /// E.g. Element: {"_STATE_SCHEMA": "ElementState"}, BaseFrame: {"_STATE_SCHEMA": "BaseFrameState"}
    pub field_built_names: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AliasDecl {
    pub name: String,
    pub typ: AnnotationType,
}

pub struct ScanResult {
    pub classes: Vec<ClassDecl>,
    pub aliases: Vec<AliasDecl>,
    pub has_meta: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationBlock {
    pub params: Vec<ParamInfo>,
    pub returns: Vec<AnnotationType>,
    pub var_type: Option<AnnotationType>,
    pub class: Option<String>,
    pub class_type_params: Vec<String>,
    pub class_parents: Vec<String>,
    pub fields: Vec<(String, AnnotationType, Visibility)>,
    pub alias: Option<(String, AnnotationType)>,
    pub alias_continuations: Vec<AnnotationType>,
    pub overloads: Vec<String>,
    pub meta: bool,
    pub deprecated: bool,
    pub nodiscard: bool,
    pub constructor: bool,
    pub constructor_methods: Vec<String>,
    pub visibility: Visibility,
    pub doc: Option<String>,
    pub generics: Vec<(String, Option<String>)>, // (name, optional constraint type name)
    pub defclass: Option<String>, // generic name that auto-creates classes from backtick inference
    pub defclass_parent: Option<String>, // generic name for the parent class (e.g. @defclass T : P)
    pub accessors: Vec<(String, Visibility)>,
    /// `@builds-field <param_idx> <type>` — builder method adds a field to the built type
    pub builds_field: Option<(usize, AnnotationType)>,
    /// `@built-name <param_idx>` — the string literal from this param becomes the built table's class name
    pub built_name: Option<usize>,
    /// `@built-extends` — the new built type inherits from the receiver's current built type
    pub built_extends: bool,
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
    let Some(first_token) = node.first_token() else { return AnnotationBlock::default(); };

    let mut annotation_lines = Vec::new();
    let mut doc_lines = Vec::new();
    let mut tok = first_token.prev_token();
    while let Some(token) = tok {
        let kind = token.kind();
        if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
            tok = token.prev_token();
            continue;
        }
        if kind == SyntaxKind::Comment {
            // Skip inline trailing comments (on the same line as code from a previous statement).
            // e.g. `local x = {} ---@class Foo` should not leak to the next statement.
            // Check if there's a non-whitespace token before this comment on the same line.
            {
                let mut prev = token.prev_token();
                let mut is_inline = false;
                while let Some(ref p) = prev {
                    if p.kind() == SyntaxKind::Whitespace {
                        prev = p.prev_token();
                        continue;
                    }
                    if p.kind() != SyntaxKind::Newline {
                        is_inline = true;
                    }
                    break;
                }
                if is_inline {
                    break; // inline trailing comment — stop collecting
                }
            }
            let text = token.text();
            if text.starts_with("---@") || text.starts_with("---|") || text.starts_with("--- @") {
                annotation_lines.push(text.to_string());
                tok = token.prev_token();
                continue;
            } else if text.starts_with("---") {
                // Plain doc comment line — strip prefix
                let content = text.strip_prefix("---").unwrap_or("");
                let content = content.strip_prefix(' ').unwrap_or(content);
                doc_lines.push(content.to_string());
                tok = token.prev_token();
                continue;
            }
        }
        // Non-trivia, non-annotation token — stop
        break;
    }

    annotation_lines.reverse();
    doc_lines.reverse();

    let mut block = parse_annotation_lines(&annotation_lines);

    // Build doc string, stripping editor-specific command: links
    let doc_lines: Vec<String> = doc_lines.iter()
        .map(|s| strip_command_links(s))
        .filter(|s| !s.is_empty())
        .collect();
    let doc_text = doc_lines.join("\n").trim().to_string();
    block.doc = if doc_text.is_empty() { None } else { Some(doc_text) };

    block
}

/// Convert `[text](command:extension.lua.doc?["path"])` links to real Lua manual URLs.
/// Other `command:` links are stripped (standalone ones become empty, inline ones keep text).
fn strip_command_links(s: &str) -> String {
    let mut result = s.to_string();
    while let Some(start) = result.find("](command:") {
        let bracket_start = result[..start].rfind('[');
        let paren_end = result[start..].find(')').map(|p| start + p + 1);
        match (bracket_start, paren_end) {
            (Some(bs), Some(pe)) => {
                let url_content = &result[start + 2..pe - 1]; // inside (...)
                // Try to convert extension.lua.doc links to real URLs
                if let Some(real_url) = convert_lua_doc_link(url_content) {
                    let link_text = &result[bs + 1..start];
                    result = format!("{}[{}]({}){}", &result[..bs], link_text, real_url, &result[pe..]);
                    continue; // re-scan in case there are more (won't match command: again)
                }
                let before = result[..bs].trim();
                let after = result[pe..].trim();
                if before.is_empty() && after.is_empty() {
                    return String::new();
                }
                let link_text = &result[bs + 1..start];
                result = format!("{}{}{}", &result[..bs], link_text, &result[pe..]);
            }
            _ => break,
        }
    }
    result.trim().to_string()
}

/// Convert `command:extension.lua.doc?["en-us/51/manual.html/pdf-table.insert"]` to a real URL.
fn convert_lua_doc_link(command_url: &str) -> Option<String> {
    let path = command_url.strip_prefix("command:extension.lua.doc?[\"")?.strip_suffix("\"]")?;
    let anchor = path.rsplit_once('/')?.1;
    Some(format!("https://www.lua.org/manual/5.1/manual.html#{}", anchor))
}

/// Scan all comments in the syntax tree for @class and @alias declarations.
pub fn scan_all_annotations(root: &SyntaxNode) -> ScanResult {
    let mut classes = Vec::new();
    let mut aliases = Vec::new();
    let mut has_meta = false;

    let mut current_group: Vec<String> = Vec::new();
    let mut prev_was_newline = false;

    for event in root.descendants_with_tokens() {
        let rowan::NodeOrToken::Token(tok) = event else { continue };
        let kind = tok.kind();
        if kind == SyntaxKind::Comment {
            let text = tok.text();
            if text.starts_with("---@") || text.starts_with("---|") || text.starts_with("--- @") {
                // If this starts a new @class or @alias and the current group already
                // contains one, flush the previous group first so each declaration
                // becomes its own group (block.alias/class is Option and would be overwritten).
                if !current_group.is_empty() {
                    let starts_new_decl = text.contains("@class ") || text.contains("@alias ");
                    let group_has_decl = starts_new_decl && current_group.iter().any(|l| l.contains("@class ") || l.contains("@alias "));
                    if group_has_decl {
                        flush_group(&current_group, &mut classes, &mut aliases, &mut has_meta);
                        current_group.clear();
                    }
                }
                current_group.push(text.to_string());
            }
            prev_was_newline = false;
        } else if kind == SyntaxKind::Newline {
            if prev_was_newline && !current_group.is_empty() {
                flush_group(&current_group, &mut classes, &mut aliases, &mut has_meta);
                current_group.clear();
            }
            prev_was_newline = true;
        } else if kind == SyntaxKind::Whitespace {
        } else {
            flush_group(&current_group, &mut classes, &mut aliases, &mut has_meta);
            current_group.clear();
            prev_was_newline = false;
        }
    }
    flush_group(&current_group, &mut classes, &mut aliases, &mut has_meta);

    ScanResult { classes, aliases, has_meta }
}

fn flush_group(
    lines: &[String],
    classes: &mut Vec<ClassDecl>,
    aliases: &mut Vec<AliasDecl>,
    has_meta: &mut bool,
) {
    if lines.is_empty() { return; }
    let block = parse_annotation_lines(lines);
    if block.meta { *has_meta = true; }
    if let Some(class_name) = block.class {
        let overloads = block.overloads.iter().filter_map(|s| parse_overload(s)).collect();
        classes.push(ClassDecl { name: class_name, type_params: block.class_type_params, parents: block.class_parents, fields: block.fields, accessors: block.accessors, overloads, generics: block.generics, constructor_methods: block.constructor_methods, constraint_type_arg_subs: Vec::new(), field_built_names: HashMap::new() });
    }
    if let Some((name, typ)) = block.alias {
        let typ = if block.alias_continuations.is_empty() {
            typ
        } else {
            // Merge base type with ---| continuation types into a union
            let mut parts = match typ {
                AnnotationType::Simple(ref s) if s == "unknown" => Vec::new(),
                AnnotationType::Union(u) => u,
                other => vec![other],
            };
            parts.extend(block.alias_continuations);
            if parts.len() == 1 { parts.pop().unwrap() } else { AnnotationType::Union(parts) }
        };
        aliases.push(AliasDecl { name, typ });
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
                // Parse type params: @class Name<S, T> → name="Name", type_params=["S","T"]
                let (class_name, type_params) = if let Some(open) = class_name.find('<') {
                    let name = &class_name[..open];
                    let params_str = class_name[open+1..].trim_end_matches('>');
                    let params: Vec<String> = params_str.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    (name.to_string(), params)
                } else {
                    (class_name.to_string(), Vec::new())
                };
                block.class = Some(class_name);
                block.class_type_params = type_params;
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
            let (vis, rest) = if let Some(r) = rest.strip_prefix("private") {
                if r.starts_with(char::is_whitespace) { (Visibility::Private, r.trim_start()) }
                else { (Visibility::Public, rest) }
            } else if let Some(r) = rest.strip_prefix("protected") {
                if r.starts_with(char::is_whitespace) { (Visibility::Protected, r.trim_start()) }
                else { (Visibility::Public, rest) }
            } else if let Some(r) = rest.strip_prefix("public") {
                if r.starts_with(char::is_whitespace) { (Visibility::Public, r.trim_start()) }
                else { (Visibility::Public, rest) }
            } else {
                (Visibility::Public, rest)
            };
            if let Some((name, type_str)) = rest.split_once(char::is_whitespace) {
                let is_optional = name.ends_with('?');
                let name = name.trim_end_matches('?');
                let type_str_trimmed = type_str.trim();
                let type_only = extract_type_prefix(type_str_trimmed);
                let typ = parse_type(type_only);
                let typ = if is_optional {
                    AnnotationType::Union(vec![typ, AnnotationType::Simple("nil".to_string())])
                } else {
                    typ
                };
                block.fields.push((name.to_string(), typ, vis));
            }
        } else if let Some(rest) = content.strip_prefix("@alias") {
            let rest = rest.trim();
            if let Some((name, type_str)) = rest.split_once(char::is_whitespace) {
                let typ = parse_type(type_str.trim());
                block.alias = Some((name.to_string(), typ));
            } else if !rest.is_empty() {
                // Name-only @alias (multi-line form, types come from ---|  lines)
                block.alias = Some((rest.to_string(), AnnotationType::Simple("unknown".to_string())));
            }
        } else if let Some(rest) = content.strip_prefix('|') {
            // ---|  continuation line — append to alias union
            let rest = rest.trim();
            // Strip trailing # comment
            let type_str = if let Some(hash_pos) = find_hash_comment(rest) {
                rest[..hash_pos].trim()
            } else {
                rest
            };
            if !type_str.is_empty() && block.alias.is_some() {
                block.alias_continuations.push(parse_type(type_str));
            }
        } else if let Some(rest) = content.strip_prefix("@param") {
            let rest = rest.trim();
            if let Some((name, type_str)) = rest.split_once(char::is_whitespace) {
                let is_optional = name.ends_with('?');
                let name = name.trim_end_matches('?');
                let type_str_trimmed = type_str.trim();
                let type_only = extract_type_prefix(type_str_trimmed);
                let typ = parse_type(type_only);
                let description = type_str_trimmed[type_only.len()..].trim().to_string();
                let description = if description.is_empty() { None } else { Some(description) };
                block.params.push(ParamInfo {
                    name: name.to_string(),
                    typ,
                    optional: is_optional,
                    description,
                });
            }
        } else if let Some(rest) = content.strip_prefix("@return") {
            let rest = rest.trim();
            for type_str in split_return_types(rest) {
                let type_str = type_str.trim();
                if !type_str.is_empty() {
                    // @return built [: Parent] — preserve the full "built : Parent" string
                    if type_str == "built" || type_str.starts_with("built ") || type_str.starts_with("built:") {
                        // Extract optional parent: "built : ReactiveState" or "built:ReactiveState"
                        let after_built = type_str["built".len()..].trim();
                        let parent_part = after_built.strip_prefix(':').map(|p| p.trim());
                        let label = if let Some(parent) = parent_part {
                            let parent_name = parent.split_whitespace().next().unwrap_or(parent);
                            format!("built:{}", parent_name)
                        } else {
                            "built".to_string()
                        };
                        block.returns.push(AnnotationType::Simple(label));
                        continue;
                    }
                    let type_only = extract_type_prefix(type_str);
                    block.returns.push(parse_type(type_only));
                }
            }
        } else if let Some(rest) = content.strip_prefix("@type") {
            let rest = rest.trim();
            if !rest.is_empty() { block.var_type = Some(parse_type(rest)); }
        } else if content.starts_with("@cast") {
            // @cast directives are handled via raw comment lines in build_ir.rs
        } else if let Some(rest) = content.strip_prefix("@enum") {
            let rest = rest.trim();
            if let Some(name) = rest.split_whitespace().next() {
                block.class = Some(name.to_string());
            }
        } else if content.starts_with("@meta") {
            block.meta = true;
        } else if let Some(rest) = content.strip_prefix("@overload") {
            let rest = rest.trim();
            if !rest.is_empty() { block.overloads.push(rest.to_string()); }
        } else if let Some(rest) = content.strip_prefix("@defclass") {
            let rest = rest.trim();
            if !rest.is_empty() {
                // Parse "T : P", "T: P", "T :P", "T:P" flexibly
                if let Some(colon_pos) = rest.find(':') {
                    let name = rest[..colon_pos].trim();
                    let parent = rest[colon_pos+1..].trim();
                    if !name.is_empty() {
                        block.defclass = Some(name.split_whitespace().next().unwrap().to_string());
                    }
                    if !parent.is_empty() {
                        block.defclass_parent = Some(parent.split_whitespace().next().unwrap().to_string());
                    }
                } else {
                    let name = rest.split_whitespace().next().unwrap();
                    block.defclass = Some(name.to_string());
                }
            }
        } else if let Some(rest) = content.strip_prefix("@builds-field") {
            let rest = rest.trim();
            if let Some((idx_str, type_str)) = rest.split_once(char::is_whitespace) {
                if let Ok(idx) = idx_str.trim().parse::<usize>() {
                    block.builds_field = Some((idx, parse_type(type_str.trim())));
                }
            }
        } else if let Some(rest) = content.strip_prefix("@built-name") {
            let rest = rest.trim();
            if let Ok(idx) = rest.parse::<usize>() {
                if idx >= 1 {
                    block.built_name = Some(idx);
                }
            }
        } else if content.starts_with("@built-extends") {
            block.built_extends = true;
        } else if content.starts_with("@deprecated") {
            block.deprecated = true;
        } else if content.starts_with("@nodiscard") {
            block.nodiscard = true;
        } else if let Some(rest) = content.strip_prefix("@constructor") {
            let rest = rest.trim();
            if rest.is_empty() {
                block.constructor = true;
            } else {
                block.constructor_methods.push(rest.split_whitespace().next().unwrap().to_string());
            }
        } else if let Some(rest) = content.strip_prefix("@generic") {
            let rest = rest.trim();
            for part in rest.split(',') {
                let part = part.trim();
                if part.is_empty() { continue; }
                if let Some((name, constraint)) = part.split_once(':') {
                    let name = name.trim();
                    let constraint = constraint.trim();
                    if !name.is_empty() {
                        block.generics.push((name.to_string(), Some(constraint.to_string())));
                    }
                } else {
                    block.generics.push((part.to_string(), None));
                }
            }
        } else if content.starts_with("@private") {
            block.visibility = Visibility::Private;
        } else if content.starts_with("@protected") {
            block.visibility = Visibility::Protected;
        } else if let Some(rest) = content.strip_prefix("@accessor") {
            let rest = rest.trim();
            if let Some((name, vis_str)) = rest.split_once(char::is_whitespace) {
                let vis = match vis_str.trim() {
                    "private" => Visibility::Private,
                    "protected" => Visibility::Protected,
                    "public" => Visibility::Public,
                    _ => continue,
                };
                block.accessors.push((name.to_string(), vis));
            } else if !rest.is_empty() {
                block.accessors.push((rest.to_string(), Visibility::Public));
            }
        }
    }

    block
}

/// Split `@return` type list on commas, but treat `fun(...): type, type` as a single type
/// and strip trailing `@description` text.
fn split_return_types(s: &str) -> Vec<&str> {
    // Strip trailing @description (` @word...` at depth 0)
    let s = {
        let bytes = s.as_bytes();
        let mut depth = 0usize;
        let mut end = s.len();
        for i in 0..bytes.len() {
            match bytes[i] {
                b'<' | b'(' => depth += 1,
                b'>' | b')' => depth = depth.saturating_sub(1),
                b'@' if depth == 0 && i > 0 && bytes[i - 1] == b' ' => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        s[..end].trim_end()
    };
    // Split on commas at depth 0, but after a fun() closing paren followed by `:`,
    // don't split (those commas are the function's multi-return types).
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut in_fun_ret = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' => {
                depth += 1;
                in_fun_ret = false;
            }
            '>' => depth = depth.saturating_sub(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b' ' {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b':' {
                        in_fun_ret = true;
                    }
                }
            }
            ',' if depth == 0 && !in_fun_ret => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Extract the type expression prefix from a string that may have a trailing description.
/// Splits at the first whitespace that is at bracket/paren depth 0, unless the preceding
/// non-whitespace context indicates continuation (e.g. `fun(...):` return type).
/// Find position of `#` comment suffix outside of quotes, e.g. `"ABSTRACT" # description`.
fn find_hash_comment(s: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    for (i, c) in s.char_indices() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return Some(i),
            _ => {}
        }
    }
    None
}

fn extract_type_prefix(s: &str) -> &str {
    let mut depth = 0usize;
    let mut after_colon = false;
    // Track when inside a function return type list (after `):` at depth 0).
    // Only commas set after_comma to allow the space after `,` in `fun(): T1, T2`.
    let mut in_fun_ret = false;
    let mut after_comma = false;
    let mut after_pipe = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '{' => { depth += 1; after_colon = false; in_fun_ret = false; after_comma = false; after_pipe = false; }
            '>' | ')' | '}' => {
                depth = depth.saturating_sub(1);
                after_colon = false;
                after_comma = false;
                after_pipe = false;
                if depth == 0 && c == ')' {
                    // Look ahead for `:` (possibly after spaces) to detect fun() return types
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                    if j < bytes.len() && bytes[j] == b':' {
                        in_fun_ret = true;
                    }
                }
            }
            '|' if depth == 0 => { in_fun_ret = false; after_colon = false; after_comma = false; after_pipe = true; }
            ',' if depth == 0 && in_fun_ret => { after_comma = true; after_pipe = false; }
            ':' if depth == 0 => { after_colon = true; after_pipe = false; }
            c if c.is_whitespace() && depth == 0 && !after_colon && !after_comma && !after_pipe => {
                // Look ahead: if a `|` follows (with optional spaces), this is a
                // union type like `"A" | "B"` — continue parsing instead of stopping.
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                if j < bytes.len() && bytes[j] == b'|' {
                    // skip — this space is part of a union type expression
                } else {
                    return &s[..i];
                }
            }
            _ => { after_colon = false; after_comma = false; after_pipe = false; }
        }
    }
    s
}

fn split_at_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    // Track function return context so `|` inside `fun(): T1, T2|T3` is not
    // treated as a top-level union separator (the `|` binds to T2 within the
    // function's return list).
    let mut in_fun_ret = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '{' => { depth += 1; in_fun_ret = false; }
            '>' | ')' | '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && c == ')' {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                    if j < bytes.len() && bytes[j] == b':' {
                        in_fun_ret = true;
                    }
                }
            }
            c if c == sep && depth == 0 && !in_fun_ret => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

pub(crate) fn format_annotation_type(at: &AnnotationType) -> String {
    match at {
        AnnotationType::Simple(s) => s.clone(),
        AnnotationType::Array(inner) => format!("{}[]", format_annotation_type(inner)),
        AnnotationType::Union(types) => types.iter()
            .map(|t| format_annotation_type(t))
            .collect::<Vec<_>>()
            .join(" | "),
        AnnotationType::Parameterized(name, params) => {
            let params_str = params.iter()
                .map(|t| format_annotation_type(t))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}<{}>", name, params_str)
        }
        AnnotationType::Backtick(inner) => format_annotation_type(inner),
        AnnotationType::Fun(params, returns, is_vararg) => {
            let mut args: Vec<String> = params.iter().map(|p| {
                let suffix = if p.optional { "?" } else { "" };
                format!("{}{}: {}", p.name, suffix, format_annotation_type(&p.typ))
            }).collect();
            if *is_vararg { args.push("...".to_string()); }
            let ret_str = if returns.is_empty() {
                String::new()
            } else {
                format!(": {}", returns.iter().map(|r| format_annotation_type(r)).collect::<Vec<_>>().join(", "))
            };
            format!("fun({}){}", args.join(", "), ret_str)
        }
    }
}

pub(crate) fn parse_type(s: &str) -> AnnotationType {
    let s = s.trim();
    if s.is_empty() { return AnnotationType::Simple(s.to_string()); }
    if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') {
        return AnnotationType::Backtick(Box::new(parse_type(&s[1..s.len()-1])));
    }
    if s.ends_with('?') {
        let mut depth = 0usize;
        for c in s[..s.len()-1].chars() {
            match c { '<' | '(' => depth += 1, '>' | ')' => depth = depth.saturating_sub(1), _ => {} }
        }
        if depth == 0 {
            let base_type = parse_type(&s[..s.len()-1]);
            return AnnotationType::Union(vec![base_type, AnnotationType::Simple("nil".to_string())]);
        }
    }
    let union_parts = split_at_top_level(s, '|');
    if union_parts.len() > 1 {
        let parts: Vec<AnnotationType> = union_parts.iter().map(|p| parse_type(p.trim())).collect();
        return AnnotationType::Union(parts);
    }
    // Parenthesized types: (string|number), (fun(): T)
    if s.starts_with('(') {
        let mut depth = 0i32;
        let mut close = None;
        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => { depth -= 1; if depth == 0 { close = Some(i); break; } }
                _ => {}
            }
        }
        if close == Some(s.len() - 1) {
            return parse_type(&s[1..s.len()-1]);
        }
    }
    if s.starts_with("fun(") {
        if let Some(sig) = parse_overload(s) {
            return AnnotationType::Fun(sig.params, sig.returns, sig.is_vararg);
        }
    }
    if s.ends_with("[]") {
        let base = parse_type(&s[..s.len()-2]);
        return AnnotationType::Array(Box::new(base));
    }
    if s.ends_with('>') {
        if let Some(lt_pos) = s.find('<') {
            let base = s[..lt_pos].trim();
            let args_str = &s[lt_pos+1..s.len()-1];
            let args = split_at_top_level(args_str, ',');
            let arg_types: Vec<AnnotationType> = args.iter().map(|a| parse_type(a.trim())).collect();
            return AnnotationType::Parameterized(base.to_string(), arg_types);
        }
    }
    // Inline table types: {key: type, ...} → table
    if s.starts_with('{') {
        return AnnotationType::Simple("table".to_string());
    }
    AnnotationType::Simple(s.to_string())
}

/// Parsed overload signature from `---@overload fun(...): ret` or `---@overload return: ret`.
#[derive(Debug, Clone, PartialEq)]
pub struct OverloadSig {
    pub params: Vec<ParamInfo>,
    pub returns: Vec<AnnotationType>,
    pub is_vararg: bool,
    pub is_return_only: bool,
}

/// Parse an overload string like `fun(param: type, ...): retType` or `return: type, type`.
pub fn parse_overload(s: &str) -> Option<OverloadSig> {
    let s = s.trim();

    // Return-only overload: `return: type1, type2`
    if let Some(ret_str) = s.strip_prefix("return:") {
        let ret_str = ret_str.trim();
        let returns = if ret_str.is_empty() { Vec::new() }
        else { split_params(ret_str).iter().map(|r| parse_type(r.trim())).collect() };
        return Some(OverloadSig { params: Vec::new(), returns, is_vararg: false, is_return_only: true });
    }

    let rest = s.strip_prefix("fun(")?;
    let mut depth = 1u32;
    let mut close = None;
    for (i, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => { depth -= 1; if depth == 0 { close = Some(i); break; } }
            _ => {}
        }
    }
    let close = close?;
    let params_str = &rest[..close];
    let after_paren = rest[close + 1..].trim();

    let mut params = Vec::new();
    let mut is_vararg = false;
    if !params_str.is_empty() {
        for part in split_params(params_str) {
            let part = part.trim();
            if part == "..." || part.starts_with("...:") {
                is_vararg = true;
                continue;
            }
            if let Some((name, type_str)) = part.split_once(':') {
                let trimmed = name.trim();
                let optional = trimmed.ends_with('?');
                let name = trimmed.trim_end_matches('?').to_string();
                let ann_type = parse_type(type_str.trim());
                params.push(ParamInfo { name, typ: ann_type, optional, description: None });
            } else {
                let optional = part.ends_with('?');
                params.push(ParamInfo {
                    name: part.trim_end_matches('?').to_string(),
                    typ: AnnotationType::Simple("any".to_string()),
                    optional,
                    description: None,
                });
            }
        }
    }

    let returns = if let Some(ret_str) = after_paren.strip_prefix(':') {
        let ret_str = ret_str.trim();
        if ret_str.is_empty() { Vec::new() }
        else { split_params(ret_str).iter().map(|r| parse_type(r.trim())).collect() }
    } else { Vec::new() };

    Some(OverloadSig { params, returns, is_vararg, is_return_only: false })
}

fn split_params(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0u32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => { parts.push(&s[start..i]); start = i + 1; }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

// ── Global declaration scanning ──────────────────────────────────────────────

pub const ADDON_NS_NAME: &str = "__addon_ns__";

#[derive(Debug, Clone, PartialEq)]
pub enum FieldValueKind { String, Number, Boolean, Nil, Table, Function, FunctionCall(Vec<String>, Option<std::string::String>), FieldRef(Vec<String>), Unknown }

#[derive(Debug, Clone, PartialEq)]
pub enum ExternalGlobalKind {
    Function,
    Method(String, bool),
    /// Method on a sub-table field: (sub_table_field, method_name, is_colon)
    NestedMethod(String, String, bool),
    Table,
    TableField(String, FieldValueKind),
    Variable(FieldValueKind),
    /// Reference to a field on another table (e.g. `strmatch = str.match` where `str` = `string`)
    FieldRef(String, String),
}

#[derive(Debug, Clone)]
pub struct ExternalGlobal {
    pub name: String,
    pub kind: ExternalGlobalKind,
    pub params: Vec<ParamInfo>,
    pub returns: Vec<AnnotationType>,
    pub overloads: Vec<OverloadSig>,
    pub doc: Option<String>,
    pub deprecated: bool,
    pub nodiscard: bool,
    pub constructor: bool,
    pub visibility: Visibility,
    pub generics: Vec<(String, Option<String>)>,
    pub defclass: Option<String>,
    pub defclass_parent: Option<String>,
    pub source_path: Option<PathBuf>,
    pub def_start: u32,
    pub def_end: u32,
    /// Intermediate path components (e.g. ["__private"] for `Class.__private:Method`)
    pub intermediates: Vec<String>,
    /// `@builds-field` annotation: (param_index_1based, field_type)
    pub builds_field: Option<(usize, AnnotationType)>,
    /// `@built-name` annotation: param_index (1-based) whose string literal names the built type
    pub built_name: Option<usize>,
    /// `@built-extends` annotation: new built type inherits from receiver's current built type
    pub built_extends: bool,
    /// For string literal assignments, the raw string value (e.g. `"hello"`)
    pub string_value: Option<String>,
    /// For number literal assignments, the raw number value (e.g. `"42"`)
    pub number_value: Option<String>,
}

/// Check if an expression is `select(N, ...)` and return N.
pub(crate) fn is_select_varargs(expr: &Expression) -> Option<usize> {
    if let Expression::FunctionCall(call) = expr {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() == 1 && names[0] == "select" {
            let args = call.arguments()?.expressions();
            if args.len() == 2 {
                if let (Expression::Literal(lit), Expression::VarArgs(_)) = (&args[0], &args[1]) {
                    let n_str = lit.get_number()?;
                    return n_str.parse::<usize>().ok();
                }
            }
        }
    }
    None
}

pub fn scan_file_globals(root: &SyntaxNode, source_path: Option<&Path>) -> Vec<ExternalGlobal> {
    let owned_path = source_path.map(|p| p.to_path_buf());
    let Some(block) = Block::cast(root.clone()) else { return Vec::new(); };

    let mut addon_ns_var: Option<String> = None;
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt {
            if let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                if names.len() >= 2 && exprs.len() == 1 && matches!(exprs[0], Expression::VarArgs(_)) {
                    addon_ns_var = Some(names[1].clone());
                    break;
                }
                // local ns = select(2, ...)
                if names.len() >= 1 && exprs.len() == 1 {
                    if let Some(n) = is_select_varargs(&exprs[0]) {
                        if n == 2 {
                            addon_ns_var = Some(names[0].clone());
                            break;
                        }
                    }
                }
            }
        }
    }

    // Track local aliases to known tables (e.g. `local str = string`, `local tab = table`)
    let mut local_aliases: HashMap<String, String> = HashMap::new();
    // Track local variables assigned table constructors (e.g. `local Locale = {}`)
    let mut local_tables: HashSet<String> = HashSet::new();
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt {
            if let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                if names.len() == 1 && exprs.len() == 1 {
                    if let Expression::Identifier(ident) = &exprs[0] {
                        let rhs_names = ident.names();
                        if rhs_names.len() == 1 {
                            local_aliases.insert(names[0].clone(), rhs_names[0].clone());
                        }
                    }
                    if matches!(&exprs[0], Expression::TableConstructor(_)) {
                        local_tables.insert(names[0].clone());
                    }
                }
            }
        }
    }

    // Track local variables annotated with @class (e.g. local LibTSMCore = {} ---@class LibTSMCore)
    // Checks both preceding annotations and inline trailing comments within the statement
    let mut class_vars: HashMap<String, String> = HashMap::new();
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt {
            let annotations = extract_annotations(assign.syntax());
            let class_name = annotations.class.or_else(|| {
                // Scan tokens in the statement for an inline ---@class comment
                // Only consider comments before the first newline (same line as the code)
                let mut past_assign = false;
                for token in assign.syntax().descendants_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = token {
                        if t.kind() == SyntaxKind::Assign { past_assign = true; continue; }
                        if !past_assign { continue; }
                        if t.kind() == SyntaxKind::Newline { break; }
                        if t.kind() == SyntaxKind::Comment {
                            let text = t.text();
                            let content = text.trim_start_matches('-').trim();
                            if let Some(rest) = content.strip_prefix("@class") {
                                let rest = rest.trim();
                                return rest.split_whitespace().next()
                                    .map(|s| s.trim_end_matches(':').to_string());
                            }
                        }
                    }
                }
                None
            });
            if let Some(class_name) = class_name {
                if let Some(name_list) = assign.name_list() {
                    let names = name_list.names();
                    if names.len() == 1 {
                        class_vars.insert(names[0].clone(), class_name);
                    }
                }
            }
        }
    }

    // Also populate class_vars from defclass-style calls:
    // `local X = Y:Init("ClassName")` or chained `local X = Y:From("Z"):Include("ClassName")`
    // Walk the call chain to find the innermost call with a string literal first argument.
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt {
            if let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                if names.len() == 1 && exprs.len() == 1 && !class_vars.contains_key(&names[0]) {
                    if let Expression::FunctionCall(call) = &exprs[0] {
                        if let Some(class_name) = extract_string_arg_from_call_chain(&call) {
                            class_vars.insert(names[0].clone(), class_name);
                        }
                    }
                }
            }
        }
    }

    let mut globals = Vec::new();
    // Track field names assigned on the addon table in this file (e.g. ns.LibTSMApp = ...)
    // Used to gate 3-part chains so we don't inject fields onto unrelated external classes
    let mut addon_assigned_fields: HashSet<String> = HashSet::new();
    // Buffer methods defined on local tables (e.g. function Locale.GetTable())
    // so they can be emitted when the local table is assigned to the addon ns
    let mut local_table_methods: HashMap<String, Vec<ExternalGlobal>> = HashMap::new();
    // Map local table var name → addon field name (e.g. "Locale" → "Locale" from ns.Locale = Locale)
    let mut local_table_to_addon_field: HashMap<String, String> = HashMap::new();

    for stmt in block.statements() {
        match &stmt {
            Statement::FunctionDefinition(func) => {
                if let Some(ident) = func.identifier() {
                    let names = ident.names();
                    let annotations = extract_annotations(func.syntax());
                    let overloads: Vec<OverloadSig> = annotations.overloads.iter()
                        .filter_map(|s| parse_overload(s)).collect();
                    let range = func.syntax().text_range();
                    let def_start = u32::from(range.start());
                    let def_end = u32::from(range.end());
                    // If no @param annotations, fill from actual parameter names
                    let is_colon = ident.is_call_to_self();
                    let params = if annotations.params.is_empty() {
                        if let Some(param_list) = func.params() {
                            let mut ps: Vec<ParamInfo> = param_list.parameters().into_iter()
                                .filter(|n| !is_colon || n != "self")
                                .map(|n| ParamInfo { name: n, typ: AnnotationType::Simple(String::new()), optional: false, description: None })
                                .collect();
                            if param_list.ellipsis() {
                                ps.push(ParamInfo { name: "...".to_string(), typ: AnnotationType::Simple(String::new()), optional: false, description: None });
                            }
                            ps
                        } else { Vec::new() }
                    } else { annotations.params };
                    if names.len() == 1 {
                        globals.push(ExternalGlobal {
                            name: names[0].clone(), kind: ExternalGlobalKind::Function,
                            params, returns: annotations.returns, overloads,
                            doc: annotations.doc, deprecated: annotations.deprecated,
                            nodiscard: annotations.nodiscard, constructor: annotations.constructor,
                            visibility: annotations.visibility,
                            generics: annotations.generics, defclass: annotations.defclass, defclass_parent: annotations.defclass_parent,
                            source_path: owned_path.clone(),
                            def_start, def_end, intermediates: Vec::new(),
                            builds_field: annotations.builds_field.clone(),
                            built_name: annotations.built_name,
                            built_extends: annotations.built_extends,
                            string_value: None, number_value: None,
                        });
                    } else if names.len() >= 2 {
                        let root_name = &names[0];
                        let method_name = &names[names.len() - 1];
                        let is_colon = ident.is_call_to_self();
                        // Buffer methods on local tables for later emission
                        if names.len() == 2 && local_tables.contains(root_name) && !class_vars.contains_key(root_name) && addon_ns_var.as_deref() != Some(root_name.as_str()) {
                            local_table_methods.entry(root_name.clone()).or_default().push(ExternalGlobal {
                                name: String::new(), // placeholder, set when flushed
                                kind: ExternalGlobalKind::Method(method_name.clone(), is_colon),
                                params, returns: annotations.returns, overloads,
                                doc: annotations.doc, deprecated: annotations.deprecated,
                                nodiscard: annotations.nodiscard, constructor: annotations.constructor,
                                visibility: annotations.visibility,
                                generics: annotations.generics, defclass: annotations.defclass, defclass_parent: annotations.defclass_parent,
                                source_path: owned_path.clone(),
                                def_start, def_end, intermediates: Vec::new(),
                                builds_field: annotations.builds_field.clone(),
                                built_name: annotations.built_name,
                                built_extends: annotations.built_extends,
                                string_value: None, number_value: None,
                            });
                        } else {
                            let canonical_name = if addon_ns_var.as_deref() == Some(root_name.as_str()) {
                                ADDON_NS_NAME.to_string()
                            } else if let Some(class_name) = class_vars.get(root_name) {
                                class_name.clone()
                            } else { root_name.clone() };
                            let intermediates: Vec<String> = names[1..names.len()-1].to_vec();
                            let kind = if names.len() == 3 && addon_ns_var.as_deref() == Some(root_name.as_str()) {
                                ExternalGlobalKind::NestedMethod(names[1].clone(), method_name.clone(), is_colon)
                            } else {
                                ExternalGlobalKind::Method(method_name.clone(), is_colon)
                            };
                            globals.push(ExternalGlobal {
                                name: canonical_name, kind,
                                params, returns: annotations.returns, overloads,
                                doc: annotations.doc, deprecated: annotations.deprecated,
                                nodiscard: annotations.nodiscard, constructor: annotations.constructor,
                                visibility: annotations.visibility,
                                generics: annotations.generics, defclass: annotations.defclass, defclass_parent: annotations.defclass_parent,
                                source_path: owned_path.clone(),
                                def_start, def_end, intermediates,
                                builds_field: annotations.builds_field.clone(),
                                built_name: annotations.built_name,
                                built_extends: annotations.built_extends,
                                string_value: None, number_value: None,
                            });
                        }
                    }
                }
            }
            Statement::Assign(assign) => {
                if let (Some(var_list), Some(expr_list)) = (assign.variable_list(), assign.expression_list()) {
                    let idents = var_list.identifiers();
                    let exprs = expr_list.expressions();
                    if idents.len() == 1 && exprs.len() == 1 {
                        let names = idents[0].names();
                        if names.len() == 1 {
                            let range = assign.syntax().text_range();
                            let (kind, string_value, number_value) = match &exprs[0] {
                                Expression::TableConstructor(_) => (ExternalGlobalKind::Table, None, None),
                                Expression::Literal(lit) => {
                                    let sv = lit.get_string().map(|s| {
                                        let stripped = s.trim_matches(|c| c == '"' || c == '\'');
                                        stripped.to_string()
                                    });
                                    let nv = lit.get_number();
                                    let vk = if lit.get_string().is_some() { FieldValueKind::String }
                                        else if lit.get_bool().is_some() { FieldValueKind::Boolean }
                                        else if lit.get_number().is_some() { FieldValueKind::Number }
                                        else if lit.is_nil() { FieldValueKind::Nil }
                                        else { FieldValueKind::Unknown };
                                    (ExternalGlobalKind::Variable(vk), sv, nv)
                                }
                                Expression::Function(_) => (ExternalGlobalKind::Variable(FieldValueKind::Function), None, None),
                                Expression::Identifier(ident) => {
                                    let rhs_names = ident.names();
                                    if rhs_names.len() == 2 {
                                        let table_name = local_aliases.get(&rhs_names[0])
                                            .cloned().unwrap_or_else(|| rhs_names[0].clone());
                                        (ExternalGlobalKind::FieldRef(table_name, rhs_names[1].clone()), None, None)
                                    } else {
                                        (ExternalGlobalKind::Variable(FieldValueKind::Unknown), None, None)
                                    }
                                }
                                _ => (ExternalGlobalKind::Variable(FieldValueKind::Unknown), None, None),
                            };
                            globals.push(ExternalGlobal {
                                name: names[0].clone(), kind,
                                params: Vec::new(), returns: Vec::new(), overloads: Vec::new(),
                                doc: None, deprecated: false, nodiscard: false, constructor: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                intermediates: Vec::new(),
                                builds_field: None, built_name: None, built_extends: false,
                                string_value, number_value,
                            });
                        } else if names.len() == 2 {
                            let root_name = &names[0];
                            let field_name = &names[1];
                            // Canonicalize root name (same as method definitions)
                            let canonical_name = if addon_ns_var.as_deref() == Some(root_name.as_str()) {
                                ADDON_NS_NAME.to_string()
                            } else if let Some(class_name) = class_vars.get(root_name) {
                                class_name.clone()
                            } else { root_name.clone() };
                            let annotations = extract_annotations(assign.syntax());
                            let value_kind = match &exprs[0] {
                                Expression::Literal(lit) => {
                                    if lit.get_string().is_some() { FieldValueKind::String }
                                    else if lit.get_bool().is_some() { FieldValueKind::Boolean }
                                    else if lit.get_number().is_some() { FieldValueKind::Number }
                                    else if lit.is_nil() { FieldValueKind::Nil }
                                    else { FieldValueKind::Unknown }
                                }
                                Expression::TableConstructor(_) => FieldValueKind::Table,
                                Expression::Function(_) => FieldValueKind::Function,
                                Expression::FunctionCall(call) => {
                                    if let Some(ident) = call.identifier() {
                                        let mut callee_names = ident.names();
                                        // Canonicalize root of callee chain
                                        if !callee_names.is_empty() {
                                            if addon_ns_var.as_deref() == Some(callee_names[0].as_str()) {
                                                callee_names[0] = ADDON_NS_NAME.to_string();
                                            } else if let Some(class_name) = class_vars.get(&callee_names[0]) {
                                                callee_names[0] = class_name.clone();
                                            }
                                        }
                                        // Extract first string literal argument (for defclass resolution)
                                        // For method chains like a.b("x"):c("y"), use the innermost
                                        // call's arg ("x") instead of the outermost ("y")
                                        let first_string_arg = {
                                            let innermost_args = ident.syntax().descendants()
                                                .filter(|n| n.kind() == SyntaxKind::FunctionCall)
                                                .last()
                                                .and_then(crate::ast::FunctionCall::cast)
                                                .and_then(|fc| fc.arguments());
                                            let arg_list = innermost_args.or_else(|| call.arguments());
                                            arg_list.and_then(|al| {
                                                let args = al.expressions();
                                                if let Some(Expression::Literal(lit)) = args.first() {
                                                    lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                                } else {
                                                    None
                                                }
                                            })
                                        };
                                        FieldValueKind::FunctionCall(callee_names, first_string_arg)
                                    } else {
                                        FieldValueKind::Unknown
                                    }
                                }
                                Expression::Identifier(ident) => {
                                    let mut rhs_names = ident.names();
                                    if rhs_names.len() == 1 && local_tables.contains(&rhs_names[0]) {
                                        FieldValueKind::Table
                                    } else if rhs_names.len() >= 2 {
                                        // Canonicalize root for field references (e.g. Util.FRAME → Banking.Util.FRAME)
                                        if addon_ns_var.as_deref() == Some(rhs_names[0].as_str()) {
                                            rhs_names[0] = ADDON_NS_NAME.to_string();
                                        } else if let Some(cn) = class_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = cn.clone();
                                        }
                                        FieldValueKind::FieldRef(rhs_names)
                                    } else {
                                        FieldValueKind::Unknown
                                    }
                                }
                                _ => FieldValueKind::Unknown,
                            };
                            let returns = if let Some(ref var_type) = annotations.var_type {
                                vec![var_type.clone()]
                            } else if let Expression::Identifier(ident) = &exprs[0] {
                                let rhs_names = ident.names();
                                if rhs_names.len() == 1 {
                                    if let Some(class_name) = class_vars.get(&rhs_names[0]) {
                                        vec![AnnotationType::Simple(class_name.clone())]
                                    } else { Vec::new() }
                                } else { Vec::new() }
                            } else { Vec::new() };
                            let range = assign.syntax().text_range();
                            globals.push(ExternalGlobal {
                                name: canonical_name,
                                kind: ExternalGlobalKind::TableField(field_name.clone(), value_kind),
                                params: Vec::new(), returns, overloads: Vec::new(),
                                doc: annotations.doc, deprecated: false, nodiscard: false, constructor: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                intermediates: Vec::new(),
                                builds_field: None, built_name: None, built_extends: false,
                                string_value: None, number_value: None,
                            });
                            if addon_ns_var.as_deref() == Some(root_name.as_str()) {
                                addon_assigned_fields.insert(field_name.clone());
                                // Record mapping so methods defined later can be flushed post-loop
                                // e.g. ns.Locale = Locale → "Locale" maps to addon field "Locale"
                                if let Expression::Identifier(rhs_ident) = &exprs[0] {
                                    let rhs_names = rhs_ident.names();
                                    if rhs_names.len() == 1 && local_tables.contains(&rhs_names[0]) {
                                        local_table_to_addon_field.insert(rhs_names[0].clone(), field_name.clone());
                                    }
                                }
                            }
                        } else if names.len() == 3 && addon_ns_var.as_deref() == Some(names[0].as_str())
                            && addon_assigned_fields.contains(&names[1])
                        {
                            // ADDON_TABLE.LibTSMApp.Locale = expr → emit as TableField on names[1]
                            // Only when names[1] was assigned on the addon table earlier in this file,
                            // to avoid injecting fields onto unrelated external classes (e.g. Frame)
                            let intermediate = &names[1];
                            let field_name = &names[2];
                            let annotations = extract_annotations(assign.syntax());
                            let value_kind = match &exprs[0] {
                                Expression::Literal(lit) => {
                                    if lit.get_string().is_some() { FieldValueKind::String }
                                    else if lit.get_bool().is_some() { FieldValueKind::Boolean }
                                    else if lit.get_number().is_some() { FieldValueKind::Number }
                                    else if lit.is_nil() { FieldValueKind::Nil }
                                    else { FieldValueKind::Unknown }
                                }
                                Expression::TableConstructor(_) => FieldValueKind::Table,
                                Expression::Function(_) => FieldValueKind::Function,
                                Expression::Identifier(ident) => {
                                    let rhs_names = ident.names();
                                    if rhs_names.len() == 1 && local_tables.contains(&rhs_names[0]) {
                                        FieldValueKind::Table
                                    } else {
                                        FieldValueKind::Unknown
                                    }
                                }
                                _ => FieldValueKind::Unknown,
                            };
                            let returns = if let Some(ref var_type) = annotations.var_type {
                                vec![var_type.clone()]
                            } else { Vec::new() };
                            let range = assign.syntax().text_range();
                            globals.push(ExternalGlobal {
                                name: intermediate.clone(),
                                kind: ExternalGlobalKind::TableField(field_name.clone(), value_kind),
                                params: Vec::new(), returns, overloads: Vec::new(),
                                doc: annotations.doc, deprecated: false, nodiscard: false, constructor: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                intermediates: Vec::new(),
                                builds_field: None, built_name: None, built_extends: false,
                                string_value: None, number_value: None,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Flush buffered local table methods onto their addon namespace sub-tables
    // Handles cases where methods are defined after the ns.X = LocalTable assignment
    for (local_name, addon_field) in &local_table_to_addon_field {
        if let Some(methods) = local_table_methods.remove(local_name) {
            for mut m in methods {
                m.name = ADDON_NS_NAME.to_string();
                if let ExternalGlobalKind::Method(ref mname, is_colon) = m.kind {
                    m.kind = ExternalGlobalKind::NestedMethod(addon_field.clone(), mname.clone(), is_colon);
                }
                globals.push(m);
            }
        }
    }

    globals
}

/// Walk a function call chain to find the outermost colon-call with a string literal first argument.
/// For `Y:Init("ClassName")` returns `Some("ClassName")`.
/// For `Y:From("Z"):Include("ClassName")` returns `Some("ClassName")` (outermost call's arg).
/// Only matches colon-calls (`:Method(...)`) to avoid false positives on dot-calls like `Enum.New(...)`.
/// Returns None if no matching call is found.
fn extract_string_arg_from_call_chain(call: &FunctionCall) -> Option<String> {
    // Check if this call uses colon syntax (method call)
    let ident = call.identifier()?;
    let is_colon = ident.is_call_to_self();
    if is_colon {
        if let Some(arg_list) = call.arguments() {
            let args = arg_list.expressions();
            if let Some(Expression::Literal(lit)) = args.first() {
                if let Some(s) = lit.get_string() {
                    let name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
            }
        }
    }
    // Check nested call in the identifier (for method chains)
    let nested = ident.syntax().children().find_map(FunctionCall::cast)?;
    extract_string_arg_from_call_chain(&nested)
}

/// Scan for `local X = Y.func("ClassName")` calls where `Y.func` has `@defclass`.
/// Returns ClassDecl entries for discovered classes, with parent info from generic constraints.
/// `all_globals` should contain globals from ALL scanned files (not just this file).
pub fn scan_defclass_calls(root: &SyntaxNode, all_globals: &[ExternalGlobal], all_classes: &[ClassDecl]) -> Vec<ClassDecl> {
    use std::collections::{HashMap, HashSet};
    let Some(block) = Block::cast(root.clone()) else { return Vec::new() };

    // Build map of class name → index signature type from @field [string] Type
    let class_index_sigs: HashMap<&str, &AnnotationType> = all_classes.iter()
        .filter_map(|c| {
            c.fields.iter()
                .find(|(name, _, _)| name == "[string]" || name == "[number]")
                .map(|(_, typ, _)| (c.name.as_str(), typ))
        })
        .collect();

    // Build map of dotted function names → defclass function info
    struct DefclassFuncInfo {
        parents: Vec<String>,
        parent_param_idx: Option<usize>,
        /// Index of the param whose type is the defclass generic (for table literal absorption)
        values_param_idx: Option<usize>,
        /// For each constraint parent: (base_name, [type_arg_generic_names])
        /// e.g. for `@generic T: Class<P>` → [("Class", ["P"])]
        constraint_type_args: Vec<(String, Vec<String>)>,
        /// The name of the parent generic (e.g. "P" from `@defclass T : P`)
        parent_generic_name: Option<String>,
        /// Index signature type from parent class (e.g. EnumValue from @field [string] EnumValue)
        index_sig_type: Option<AnnotationType>,
    }
    let mut defclass_funcs: HashMap<String, DefclassFuncInfo> = HashMap::new();
    for g in all_globals.iter().filter(|g| g.defclass.is_some()) {
        let func_path = match &g.kind {
            ExternalGlobalKind::Function => g.name.clone(),
            ExternalGlobalKind::Method(method_name, _) => {
                format!("{}.{}", g.name, method_name)
            }
            _ => continue,
        };
        let defclass_name = g.defclass.as_ref().unwrap();
        let parents: Vec<String> = g.generics.iter()
            .filter(|(n, _)| n == defclass_name)
            .filter_map(|(_, c)| c.as_ref().map(|s| s.split('<').next().unwrap_or(s).to_string()))
            .collect();
        // Extract constraint type args: for `T: Class<P>` → [("Class", ["P"])]
        let constraint_type_args: Vec<(String, Vec<String>)> = g.generics.iter()
            .filter(|(n, _)| n == defclass_name)
            .filter_map(|(_, c)| {
                let c = c.as_ref()?;
                let open = c.find('<')?;
                let close = c.rfind('>')?;
                let base = c[..open].to_string();
                let args: Vec<String> = c[open+1..close].split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if args.is_empty() { None } else { Some((base, args)) }
            })
            .collect();
        // Find which param index holds the parent class generic
        let parent_param_idx = g.defclass_parent.as_ref().and_then(|parent_name| {
            g.params.iter()
                .filter(|p| p.name != "...")
                .position(|p| match &p.typ {
                    AnnotationType::Simple(name) => name == parent_name,
                    AnnotationType::Backtick(inner) => matches!(inner.as_ref(), AnnotationType::Simple(name) if name == parent_name),
                    _ => false,
                })
        });
        let parent_generic_name = g.defclass_parent.clone();
        // Find param index whose annotation is Simple(defclass_name) — for table literal absorption
        let values_param_idx = g.params.iter()
            .filter(|p| p.name != "...")
            .position(|p| matches!(&p.typ, AnnotationType::Simple(name) if name == defclass_name));
        // Look up index signature type from constraint parent class
        let index_sig_type = parents.iter()
            .find_map(|p| class_index_sigs.get(p.as_str()).copied().cloned());
        defclass_funcs.insert(func_path, DefclassFuncInfo {
            parents, parent_param_idx, values_param_idx, constraint_type_args, parent_generic_name, index_sig_type,
        });
    }
    if defclass_funcs.is_empty() { return Vec::new(); }

    // Result from find_defclass_in_chain: class name, parents, constraint type arg subs, and table literal fields
    struct DefclassCallResult {
        name: String,
        parents: Vec<String>,
        constraint_type_arg_subs: Vec<(String, Vec<String>)>,
        /// Field entries extracted from a table literal argument: (name, optional nested sub-fields)
        table_literal_fields: Vec<(String, Option<Vec<String>>)>,
        /// Index signature type from parent class (for typing absorbed fields)
        index_sig_type: Option<AnnotationType>,
    }

    // Helper: walk a FunctionCall chain to find the innermost defclass call.
    // For `DefineClass("X"):AddDep("y"):AddDep("z")`, walks through the nested
    // FunctionCall nodes in the Identifier to find the one matching a defclass func.
    fn find_defclass_in_chain(
        call: &FunctionCall,
        defclass_funcs: &HashMap<String, DefclassFuncInfo>,
    ) -> Option<DefclassCallResult> {
        let ident = call.identifier()?;
        let func_names = ident.names();
        if func_names.is_empty() { return None; }
        let func_path = func_names.join(".");

        // Check if this call itself is a defclass function
        let matched = defclass_funcs.iter().find_map(|(dc, info)| {
            if func_path == *dc || func_path.ends_with(&format!(".{}", dc.split('.').last().unwrap_or(""))) {
                Some(info)
            } else {
                None
            }
        });
        if let Some(info) = matched {
            let arg_list = call.arguments()?;
            let call_args = arg_list.expressions();
            if let Some(Expression::Literal(lit)) = call_args.first() {
                if let Some(s) = lit.get_string() {
                    let name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
                    let mut parents = info.parents.clone();
                    let mut constraint_type_arg_subs = Vec::new();
                    // Extract specific parent from the call argument
                    if let Some(idx) = info.parent_param_idx {
                        if let Some(parent_name) = call_args.get(idx).and_then(|arg| {
                            match arg {
                                Expression::Identifier(ident) => {
                                    let names = ident.names();
                                    if names.len() == 1 { Some(names[0].clone()) } else { None }
                                }
                                Expression::Literal(lit) => {
                                    lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                }
                                _ => None,
                            }
                        }) {
                            // Add the specific parent (variable name or class name string)
                            if !parents.contains(&parent_name) {
                                parents.push(parent_name.clone());
                            }
                            // Build constraint_type_arg_subs: resolve each type arg generic
                            // to the actual parent class name
                            for (base, type_arg_generics) in &info.constraint_type_args {
                                let resolved: Vec<String> = type_arg_generics.iter().map(|g| {
                                    if info.parent_generic_name.as_deref() == Some(g) {
                                        parent_name.clone()
                                    } else {
                                        g.clone() // unresolved, keep as-is
                                    }
                                }).collect();
                                constraint_type_arg_subs.push((base.clone(), resolved));
                            }
                        }
                    }
                    // Extract field names from table literal argument, detecting nested table constructors
                    let table_literal_fields = info.values_param_idx
                        .and_then(|idx| call_args.get(idx))
                        .map(|arg| {
                            if let Expression::TableConstructor(tc) = arg {
                                tc.fields().into_iter().filter_map(|f| {
                                    match f.kind() {
                                        Some(crate::ast::FieldKind::Named { name, value }) => {
                                            let nested = if let Expression::TableConstructor(inner_tc) = &value {
                                                let sub: Vec<String> = inner_tc.fields().into_iter().filter_map(|sf| {
                                                    match sf.kind() {
                                                        Some(crate::ast::FieldKind::Named { name: sub_name, .. }) => Some(sub_name),
                                                        _ => None,
                                                    }
                                                }).collect();
                                                if sub.is_empty() { None } else { Some(sub) }
                                            } else {
                                                None
                                            };
                                            Some((name, nested))
                                        }
                                        _ => None,
                                    }
                                }).collect()
                            } else {
                                Vec::new()
                            }
                        })
                        .unwrap_or_default();
                    return Some(DefclassCallResult { name, parents, constraint_type_arg_subs, table_literal_fields, index_sig_type: info.index_sig_type.clone() });
                }
            }
            return None;
        }

        // Not a defclass call — check if the identifier contains a nested FunctionCall (method chain)
        let nested = ident.syntax().children().find_map(FunctionCall::cast)?;
        find_defclass_in_chain(&nested, defclass_funcs)
    }

    // Collect all known constructor method names from external classes
    let mut constructor_names: HashSet<&str> = HashSet::new();
    for class in all_classes {
        for cname in &class.constructor_methods {
            constructor_names.insert(cname.as_str());
        }
    }

    let mut results: Vec<ClassDecl> = Vec::new();
    // Map local variable name → index in results (for matching constructor definitions)
    let mut var_to_result: HashMap<String, usize> = HashMap::new();
    let stmts = block.statements();

    for stmt in &stmts {
        // Extract the single RHS expression from local or non-local assignments
        let (rhs_call, lhs_var_name) = match stmt {
            Statement::LocalAssign(la) => {
                let var_name = la.name_list().and_then(|nl| nl.names().into_iter().next());
                let call = la.expression_list().and_then(|el| {
                    let exprs = el.expressions();
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(c.clone()) } else { None } } else { None }
                });
                (call, var_name)
            }
            Statement::Assign(a) => {
                let var_name = a.variable_list().and_then(|vl| {
                    let idents = vl.identifiers();
                    if idents.len() == 1 { idents[0].names().into_iter().next() } else { None }
                });
                let call = a.expression_list().and_then(|el| {
                    let exprs = el.expressions();
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(c.clone()) } else { None } } else { None }
                });
                (call, var_name)
            }
            _ => (None, None),
        };
        let Some(call) = rhs_call else { continue };

        if let Some(mut result) = find_defclass_in_chain(&call, &defclass_funcs) {
            // Resolve variable parent names to actual class names via var_to_result.
            // E.g. DefineClass("Child", ParentVar) records parent as "ParentVar";
            // resolve it to the class name "ParentClass" from the earlier assignment.
            for parent in &mut result.parents {
                if let Some(&parent_result_idx) = var_to_result.get(parent.as_str()) {
                    if parent_result_idx < results.len() {
                        *parent = results[parent_result_idx].name.clone();
                    }
                }
            }
            for (_, resolved_args) in &mut result.constraint_type_arg_subs {
                for arg in resolved_args {
                    if let Some(&parent_result_idx) = var_to_result.get(arg.as_str()) {
                        if parent_result_idx < results.len() {
                            *arg = results[parent_result_idx].name.clone();
                        }
                    }
                }
            }
            // Convert table literal field entries to ClassDecl fields, using index signature type if available.
            // For nested table constructors, create synthetic sub-classes.
            let default_type = result.index_sig_type.unwrap_or_else(|| AnnotationType::Simple("any".to_string()));
            let mut fields: Vec<(String, AnnotationType, Visibility)> = Vec::new();
            let mut nested_classes: Vec<ClassDecl> = Vec::new();
            for (name, nested) in result.table_literal_fields {
                if let Some(sub_field_names) = nested {
                    // Create a synthetic class for this nested group
                    let synthetic_name = format!("{}_{}", result.name, name);
                    let sub_fields: Vec<(String, AnnotationType, Visibility)> = sub_field_names.into_iter()
                        .map(|n| (n, default_type.clone(), Visibility::Public))
                        .collect();
                    // Inherit from the index sig value type (e.g. EnumValue) so the
                    // nested group can also be used as that type.
                    let nested_parents = if let AnnotationType::Simple(ref type_name) = default_type {
                        if type_name != "any" { vec![type_name.clone()] } else { Vec::new() }
                    } else { Vec::new() };
                    nested_classes.push(ClassDecl {
                        name: synthetic_name.clone(),
                        type_params: Vec::new(),
                        parents: nested_parents,
                        fields: sub_fields,
                        accessors: Vec::new(),
                        overloads: Vec::new(),
                        generics: Vec::new(),
                        constructor_methods: Vec::new(),
                        constraint_type_arg_subs: Vec::new(),
                        field_built_names: HashMap::new(),
                    });
                    fields.push((name, AnnotationType::Simple(synthetic_name), Visibility::Public));
                } else {
                    fields.push((name, default_type.clone(), Visibility::Public));
                }
            }
            // Push synthetic nested classes first so they're registered before the parent
            results.extend(nested_classes);
            let idx = results.len();
            if let Some(var_name) = lhs_var_name {
                var_to_result.insert(var_name, idx);
            }
            results.push(ClassDecl {
                name: result.name,
                type_params: Vec::new(),
                parents: result.parents,
                fields,
                accessors: Vec::new(),
                overloads: Vec::new(),
                generics: Vec::new(),
                constructor_methods: Vec::new(),
                constraint_type_arg_subs: result.constraint_type_arg_subs,
                field_built_names: HashMap::new(),
            });
        }
    }

    // Second pass: scan for constructor method definitions and extract self.X = ... fields
    if !results.is_empty() && !constructor_names.is_empty() {
        // Build lookup: func_path → return types for resolving function call RHS in constructors
        let mut global_returns: HashMap<String, Vec<AnnotationType>> = HashMap::new();
        for g in all_globals {
            let func_path = match &g.kind {
                ExternalGlobalKind::Function => g.name.clone(),
                ExternalGlobalKind::Method(method_name, _) => format!("{}.{}", g.name, method_name),
                ExternalGlobalKind::NestedMethod(sub, method_name, _) => format!("{}.{}.{}", g.name, sub, method_name),
                _ => continue,
            };
            if !g.returns.is_empty() {
                global_returns.insert(func_path, g.returns.clone());
            }
        }

        // Build @built-name lookup: func_path → param_index for extracting built table names
        let mut built_name_funcs: HashMap<String, usize> = HashMap::new();
        for g in all_globals.iter().filter(|g| g.built_name.is_some()) {
            let func_path = match &g.kind {
                ExternalGlobalKind::Function => g.name.clone(),
                ExternalGlobalKind::Method(method_name, _) => format!("{}.{}", g.name, method_name),
                ExternalGlobalKind::NestedMethod(sub, method_name, _) => format!("{}.{}.{}", g.name, sub, method_name),
                _ => continue,
            };
            built_name_funcs.insert(func_path, g.built_name.unwrap());
        }
        // Propagate @built-name through wrapper functions: if a function returns a class
        // whose method (e.g. __init) has @built-name, treat the wrapper as having @built-name too.
        let mut class_init_built_name: HashMap<String, usize> = HashMap::new();
        for g in all_globals.iter().filter(|g| g.built_name.is_some()) {
            if matches!(&g.kind, ExternalGlobalKind::Method(_, _) | ExternalGlobalKind::NestedMethod(_, _, _)) {
                class_init_built_name.insert(g.name.clone(), g.built_name.unwrap());
            }
        }
        if !class_init_built_name.is_empty() {
            for g in all_globals.iter().filter(|g| g.built_name.is_none()) {
                let returns_class = g.returns.first().and_then(|rt| {
                    if let AnnotationType::Simple(name) = rt {
                        if class_init_built_name.contains_key(name) { Some(name.clone()) } else { None }
                    } else {
                        None
                    }
                });
                if let Some(schema_class) = returns_class {
                    let param_idx = class_init_built_name[&schema_class];
                    let func_path = match &g.kind {
                        ExternalGlobalKind::Function => g.name.clone(),
                        ExternalGlobalKind::Method(method_name, _) => format!("{}.{}", g.name, method_name),
                        ExternalGlobalKind::NestedMethod(sub, method_name, _) => format!("{}.{}.{}", g.name, sub, method_name),
                        _ => continue,
                    };
                    built_name_funcs.entry(func_path).or_insert(param_idx);
                }
            }
        }

        // Scan class-level field assignments (ClassName.field = expr) to build per-class field type maps.
        // This allows constructor scanning to resolve self._X:Method() by knowing _X's type.
        // Also tracks @built-name for fields whose RHS chain contains a @built-name call.
        let mut class_field_types: HashMap<usize, HashMap<String, AnnotationType>> = HashMap::new();
        let mut class_field_built_names: HashMap<usize, HashMap<String, String>> = HashMap::new();
        for stmt in &stmts {
            let Statement::Assign(assign) = stmt else { continue };
            let Some(vl) = assign.variable_list() else { continue };
            let idents = vl.identifiers();
            if idents.len() != 1 { continue; }
            let names = idents[0].names();
            // Match ClassName.fieldName = expr (2 names) or ClassName.__sub.fieldName = expr (3+ names)
            if names.len() < 2 { continue; }
            let root_var = &names[0];
            let Some(&result_idx) = var_to_result.get(root_var) else { continue; };
            let field_name = &names[names.len() - 1];
            // Infer field type from the RHS expression
            if let Some(el) = assign.expression_list() {
                let exprs = el.expressions();
                if let Some(expr) = exprs.first() {
                    let field_type = extract_type_annotation_for_assign(assign.syntax())
                        .unwrap_or_else(|| infer_type_from_expression(expr, &global_returns, &HashMap::new(), &HashMap::new()));
                    if !matches!(&field_type, AnnotationType::Simple(s) if s == "any") {
                        class_field_types.entry(result_idx)
                            .or_default()
                            .insert(field_name.clone(), field_type);
                    }
                    // Extract @built-name from the call chain if the RHS is a function call
                    if let Expression::FunctionCall(call) = expr {
                        if let Some((built_name, _)) = extract_built_name_from_chain(call, &built_name_funcs) {
                            class_field_built_names.entry(result_idx)
                                .or_default()
                                .insert(field_name.clone(), built_name);
                        }
                    }
                }
            }
        }

        // Scan expression statements like ClassName._FIELD:MethodWithBuiltName("NewName"):...:Commit()
        // These override a parent's @built-name for the same field (e.g. _STATE_SCHEMA).
        for stmt in &stmts {
            let Statement::FunctionCall(call) = stmt else { continue };
            // Extract @built-name from the chain
            if let Some((built_name, _)) = extract_built_name_from_chain(call, &built_name_funcs) {
                // Find the root identifier: ClassName._FIELD:Method(...)
                // Walk down the chain to find the deepest identifier with 2+ names
                fn find_root_field(call: &FunctionCall) -> Option<(String, String)> {
                    let ident = call.identifier()?;
                    // Check if the identifier has a nested FunctionCall (chained call)
                    if let Some(nested) = ident.syntax().children().find_map(FunctionCall::cast) {
                        return find_root_field(&nested);
                    }
                    // This is the innermost call — check if identifier is ClassName.field
                    let names = ident.names();
                    if names.len() >= 2 {
                        Some((names[0].clone(), names[1].clone()))
                    } else {
                        None
                    }
                }
                if let Some((root_var, field_name)) = find_root_field(call) {
                    if let Some(&result_idx) = var_to_result.get(&root_var) {
                        class_field_built_names.entry(result_idx)
                            .or_default()
                            .insert(field_name, built_name);
                    }
                }
            }
        }

        // Add class-level static fields to ClassDecl so they're visible cross-file
        for (&result_idx, fields) in &class_field_types {
            let existing: HashSet<String> = results[result_idx].fields.iter()
                .map(|(name, _, _)| name.clone()).collect();
            for (field_name, field_type) in fields {
                if !existing.contains(field_name) {
                    results[result_idx].fields.push((
                        field_name.clone(),
                        field_type.clone(),
                        Visibility::Public,
                    ));
                }
            }
        }

        for stmt in &stmts {
            let Statement::FunctionDefinition(func) = stmt else { continue };
            let Some(ident) = func.identifier() else { continue };
            let names = ident.names();
            // Match patterns like ClassName:__init or ClassName.__private:__init
            if names.len() < 2 { continue; }
            let root_var = &names[0];
            let method_name = &names[names.len() - 1];
            if !constructor_names.contains(method_name.as_str()) { continue; }
            let Some(&result_idx) = var_to_result.get(root_var) else { continue; };

            // Walk the constructor body for self.X = ... assignments
            if let Some(body) = func.block() {
                let existing_fields: HashSet<String> = results[result_idx].fields.iter()
                    .map(|(name, _, _)| name.clone()).collect();
                let field_types = class_field_types.get(&result_idx).cloned().unwrap_or_default();
                let field_built_names = class_field_built_names.get(&result_idx).cloned().unwrap_or_default();
                let ctor_fields = extract_self_fields(&body, &global_returns, &field_types, &field_built_names);
                for (field_name, field_type) in ctor_fields {
                    if !existing_fields.contains(&field_name) {
                        results[result_idx].fields.push((
                            field_name,
                            field_type,
                            Visibility::Public,
                        ));
                    }
                }
            }
        }

        // Copy class_field_built_names into each ClassDecl for cross-file substitution
        for (&result_idx, names) in &class_field_built_names {
            if result_idx < results.len() {
                results[result_idx].field_built_names = names.clone();
            }
        }
    }

    results
}

/// Extract field names and inferred types from `self.X = ...` assignments in a block (recursively).
/// `field_types` maps known self-field names to their types (from class-level assignments and
/// previously-discovered constructor fields), enabling resolution of `self._X:Method()` calls.
/// `field_built_names` maps field names to their @built-name class names for built table resolution.
fn extract_self_fields(block: &Block, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>) -> Vec<(String, AnnotationType)> {
    let mut fields = Vec::new();
    let mut seen = HashSet::new();
    let mut field_types = field_types.clone();
    extract_self_fields_inner(block, &mut fields, &mut seen, global_returns, &mut field_types, field_built_names);
    fields
}

/// Infer an `AnnotationType` from a constructor RHS expression.
fn infer_type_from_expression(expr: &Expression, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>) -> AnnotationType {
    match expr {
        Expression::Literal(lit) => {
            if lit.get_string().is_some() {
                AnnotationType::Simple("string".to_string())
            } else if lit.get_number().is_some() {
                AnnotationType::Simple("number".to_string())
            } else if lit.get_bool().is_some() {
                AnnotationType::Simple("boolean".to_string())
            } else {
                // nil or unknown literal — keep as any
                AnnotationType::Simple("any".to_string())
            }
        }
        Expression::TableConstructor(_) => AnnotationType::Simple("table".to_string()),
        Expression::Function(_) => AnnotationType::Simple("function".to_string()),
        Expression::FunctionCall(call) => {
            match resolve_funcall_return_type(call, global_returns, field_types, field_built_names) {
                Some(resolved) => {
                    // Prefer @built-name class name over the chain type for field assignment
                    if let Some(name) = resolved.built_name {
                        AnnotationType::Simple(name)
                    } else {
                        resolved.chain_type
                    }
                }
                None => AnnotationType::Simple("any".to_string()),
            }
        }
        _ => AnnotationType::Simple("any".to_string()),
    }
}

/// Walk a FunctionCall chain to find a @built-name call and extract the class name.
fn extract_built_name_from_chain(
    call: &FunctionCall,
    built_name_funcs: &HashMap<String, usize>,
) -> Option<(String, String)> {
    let ident = call.identifier()?;
    let func_names = ident.names();
    if func_names.is_empty() { return None; }
    let func_path = func_names.join(".");

    let matched = built_name_funcs.iter().find_map(|(path, idx)| {
        if func_path == *path || func_path.ends_with(&format!(".{}", path.split('.').last().unwrap_or(""))) {
            Some((*idx, path.clone()))
        } else {
            None
        }
    });
    if let Some((param_idx, matched_path)) = matched {
        let arg_list = call.arguments()?;
        let call_args = arg_list.expressions();
        if let Some(Expression::Literal(lit)) = call_args.get(param_idx - 1) {
            if let Some(s) = lit.get_string() {
                return Some((s.trim_matches(|c| c == '"' || c == '\'').to_string(), matched_path));
            }
        }
        return None;
    }

    // Not a built-name call — check nested chain
    let nested = ident.syntax().children().find_map(FunctionCall::cast)?;
    extract_built_name_from_chain(&nested, built_name_funcs)
}

/// Pick the first usable return type from a function's return list.
/// Resolves `@return self` using the receiver class name, and
/// `@return built:ClassName` to the parent class name.
fn pick_effective_return(returns: &[AnnotationType], receiver_class: Option<&str>) -> Option<AnnotationType> {
    for rt in returns {
        match rt {
            AnnotationType::Simple(s) if s == "self" => {
                if let Some(cls) = receiver_class {
                    return Some(AnnotationType::Simple(cls.to_string()));
                }
                // No receiver context — skip
                continue;
            }
            AnnotationType::Simple(s) if s == "built" => continue,
            AnnotationType::Simple(s) if s.starts_with("built:") => {
                if let Some(parent) = s.strip_prefix("built:") {
                    return Some(AnnotationType::Simple(parent.to_string()));
                }
                continue;
            }
            other => return Some(other.clone()),
        }
    }
    None
}

/// Like `pick_effective_return`, but when encountering `@return built` or `@return built:X`,
/// uses the provided built_name if available (from `@built-name` on the entry function).

/// Resolved function call return type, carrying both the effective type for method lookups
/// (chain_type) and an optional @built-name override for the final field type.
struct ResolvedReturn {
    /// The type to use for method lookups in chained calls (the actual class where methods are defined)
    chain_type: AnnotationType,
    /// Optional @built-name class name that overrides chain_type for the final field assignment
    built_name: Option<String>,
}

/// Resolve a FunctionCall expression to its return type using the global returns map.
/// Handles simple calls (Class.Method()), chained calls (a:M1():M2()),
/// self-field method calls (self._X:Method()), and @return self.
/// `field_built_names` maps self-field names to their @built-name class names,
/// used to resolve `@return built` to the actual built table name.
fn resolve_funcall_return_type(
    call: &FunctionCall,
    global_returns: &HashMap<String, Vec<AnnotationType>>,
    field_types: &HashMap<String, AnnotationType>,
    field_built_names: &HashMap<String, String>,
) -> Option<ResolvedReturn> {
    let ident = call.identifier()?;

    // Check for chained calls: the identifier contains a nested FunctionCall
    if let Some(nested_call) = ident.syntax().children().find_map(FunctionCall::cast) {
        // Resolve the inner call to get the receiver type
        let inner = resolve_funcall_return_type(&nested_call, global_returns, field_types, field_built_names)?;

        // The outer method name is the last name token in the identifier
        let names = ident.names();
        let method_name = names.last()?;

        // Use chain_type for method lookup (where methods are actually defined)
        if let AnnotationType::Simple(class_name) = &inner.chain_type {
            let chain_path = format!("{}.{}", class_name, method_name);
            if let Some(returns) = global_returns.get(&chain_path) {
                let resolved = pick_effective_return(returns, Some(class_name))?;
                // Propagate built_name through @return self chains
                return Some(ResolvedReturn {
                    chain_type: resolved,
                    built_name: inner.built_name,
                });
            }
        }
        return None;
    }

    // Simple call: join names and look up
    let names = ident.names();
    if names.is_empty() { return None; }

    // Self-field method call: self._X:Method() → names = ["self", "_X", "Method"]
    if names.len() >= 3 && names[0] == "self" {
        let field_name = &names[1];
        if let Some(AnnotationType::Simple(field_class)) = field_types.get(field_name.as_str()) {
            let method_name = &names[names.len() - 1];
            let method_path = format!("{}.{}", field_class, method_name);
            if let Some(returns) = global_returns.get(&method_path) {
                let built_name = field_built_names.get(field_name.as_str()).cloned();
                if let Some(chain_type) = pick_effective_return(returns, Some(field_class)) {
                    return Some(ResolvedReturn { chain_type, built_name });
                }
                // @return built without a resolved chain type — use built_name directly
                if let Some(ref name) = built_name {
                    let has_built_return = returns.iter().any(|r| matches!(r, AnnotationType::Simple(s) if s == "built" || s.starts_with("built:")));
                    if has_built_return {
                        return Some(ResolvedReturn {
                            chain_type: AnnotationType::Simple(name.clone()),
                            built_name,
                        });
                    }
                }
            }
        }
        return None;
    }

    let func_path = names.join(".");
    if let Some(returns) = global_returns.get(&func_path) {
        // For method calls (2+ names), the receiver class is names[0]
        let receiver = if names.len() >= 2 { Some(names[0].as_str()) } else { None };
        let chain_type = pick_effective_return(returns, receiver)?;
        return Some(ResolvedReturn { chain_type, built_name: None });
    }

    None
}

/// Try to extract a `---@type X` annotation from the comments preceding an assignment statement.
/// Only considers standalone annotation comments (on their own line), not inline trailing comments.
fn extract_type_annotation_for_assign(node: &SyntaxNode) -> Option<AnnotationType> {
    let first_token = node.first_token()?;
    let mut tok = first_token.prev_token();
    while let Some(token) = tok {
        let kind = token.kind();
        if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
            tok = token.prev_token();
            continue;
        }
        if kind == SyntaxKind::Comment {
            // Skip inline trailing comments (on the same line as code from a previous statement)
            let mut prev = token.prev_token();
            let mut is_inline = false;
            while let Some(ref p) = prev {
                if p.kind() == SyntaxKind::Whitespace {
                    prev = p.prev_token();
                    continue;
                }
                if p.kind() != SyntaxKind::Newline {
                    is_inline = true;
                }
                break;
            }
            if is_inline {
                break;
            }
            let text = token.text().to_string();
            if let Some(rest) = text.strip_prefix("---@type ").or_else(|| text.strip_prefix("---@type\t")) {
                let trimmed = rest.trim();
                if !trimmed.is_empty() {
                    return Some(parse_type(trimmed));
                }
            }
        }
        break;
    }
    None
}

fn extract_self_fields_inner(block: &Block, fields: &mut Vec<(String, AnnotationType)>, seen: &mut HashSet<String>, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &mut HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Assign(assign) => {
                if let Some(vl) = assign.variable_list() {
                    let exprs = assign.expression_list().map(|el| el.expressions()).unwrap_or_default();
                    for (i, ident) in vl.identifiers().iter().enumerate() {
                        let names = ident.names();
                        if names.len() == 2 && names[0] == "self" {
                            let field_name = &names[1];
                            if seen.insert(field_name.clone()) {
                                // Try @type annotation first, then infer from expression
                                let ann_type = extract_type_annotation_for_assign(assign.syntax())
                                    .unwrap_or_else(|| {
                                        exprs.get(i)
                                            .map(|e| infer_type_from_expression(e, global_returns, field_types, field_built_names))
                                            .unwrap_or_else(|| AnnotationType::Simple("any".to_string()))
                                    });
                                // Track non-any types so later fields can reference them
                                if !matches!(&ann_type, AnnotationType::Simple(s) if s == "any") {
                                    field_types.insert(field_name.clone(), ann_type.clone());
                                }
                                fields.push((field_name.clone(), ann_type));
                            }
                        }
                    }
                }
            }
            // Recurse into nested blocks
            Statement::If(if_chain) => {
                for child in if_chain.syntax().children() {
                    if let Some(b) = Block::cast(child) {
                        extract_self_fields_inner(&b, fields, seen, global_returns, field_types, field_built_names);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(&b, fields, seen, global_returns, field_types, field_built_names);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(&b, fields, seen, global_returns, field_types, field_built_names);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(&b, fields, seen, global_returns, field_types, field_built_names);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(&b, fields, seen, global_returns, field_types, field_built_names);
                }
            }
            _ => {}
        }
    }
}

/// Scan a file for calls to functions with `@built-name`, extracting the class name
/// from the specified string literal argument. Returns empty `ClassDecl` entries so the
/// name is registered in `PreResolvedGlobals` for cross-file annotation resolution.
pub fn scan_built_name_calls(root: &SyntaxNode, all_globals: &[ExternalGlobal]) -> Vec<ClassDecl> {
    use std::collections::HashMap;
    let Some(block) = Block::cast(root.clone()) else { return Vec::new() };

    // Build map of function paths → param index for @built-name
    let mut built_name_funcs: HashMap<String, usize> = HashMap::new();
    // Also track which schema class each func_path belongs to
    let mut func_path_to_schema: HashMap<String, String> = HashMap::new();
    for g in all_globals.iter().filter(|g| g.built_name.is_some()) {
        let (func_path, schema_class) = match &g.kind {
            ExternalGlobalKind::Function => (g.name.clone(), g.name.clone()),
            ExternalGlobalKind::Method(method_name, _) => (format!("{}.{}", g.name, method_name), g.name.clone()),
            _ => continue,
        };
        func_path_to_schema.insert(func_path.clone(), schema_class);
        built_name_funcs.insert(func_path, g.built_name.unwrap());
    }

    // Propagate @built-name through wrapper functions: if a function returns a class
    // whose method (e.g. __init) has @built-name, treat the wrapper as having @built-name too.
    let mut class_init_built_name: HashMap<String, usize> = HashMap::new();
    for g in all_globals.iter().filter(|g| g.built_name.is_some()) {
        if matches!(&g.kind, ExternalGlobalKind::Method(_, _)) {
            class_init_built_name.insert(g.name.clone(), g.built_name.unwrap());
        }
    }
    if !class_init_built_name.is_empty() {
        for g in all_globals.iter().filter(|g| g.built_name.is_none()) {
            let returns_class = g.returns.first().and_then(|rt| {
                if let AnnotationType::Simple(name) = rt {
                    if class_init_built_name.contains_key(name) { Some(name.clone()) } else { None }
                } else {
                    None
                }
            });
            if let Some(schema_class) = returns_class {
                let param_idx = class_init_built_name[&schema_class];
                let func_path = match &g.kind {
                    ExternalGlobalKind::Function => g.name.clone(),
                    ExternalGlobalKind::Method(method_name, _) => format!("{}.{}", g.name, method_name),
                    _ => continue,
                };
                func_path_to_schema.entry(func_path.clone()).or_insert(schema_class);
                built_name_funcs.entry(func_path).or_insert(param_idx);
            }
        }
    }

    if built_name_funcs.is_empty() { return Vec::new(); }

    // Build map: "{ClassName}.{MethodName}" → builds-field info for @builds-field methods
    struct BuildsFieldInfo {
        param_idx: usize,
        field_type: AnnotationType,
        generics: Vec<(String, Option<String>)>,
        params: Vec<ParamInfo>,
    }
    let mut builds_field_funcs: HashMap<String, BuildsFieldInfo> = HashMap::new();
    for g in all_globals.iter().filter(|g| g.builds_field.is_some()) {
        let method_path = match &g.kind {
            ExternalGlobalKind::Method(method_name, _) => format!("{}.{}", g.name, method_name),
            ExternalGlobalKind::NestedMethod(sub, method_name, _) => format!("{}.{}.{}", g.name, sub, method_name),
            _ => continue,
        };
        let (param_idx, field_type) = g.builds_field.clone().unwrap();
        builds_field_funcs.insert(method_path, BuildsFieldInfo {
            param_idx,
            field_type,
            generics: g.generics.clone(),
            params: g.params.clone(),
        });
    }

    // Build map: schema class → parent from @return built : Parent methods
    let mut schema_built_parent: HashMap<String, String> = HashMap::new();
    for g in all_globals {
        let class_name = match &g.kind {
            ExternalGlobalKind::Method(_, _) => &g.name,
            _ => continue,
        };
        for rt in &g.returns {
            if let AnnotationType::Simple(s) = rt {
                if let Some(parent) = s.strip_prefix("built:") {
                    schema_built_parent.entry(class_name.clone()).or_insert_with(|| parent.to_string());
                }
            }
        }
    }

    // Helper: walk a FunctionCall chain to find a @built-name call
    // Returns (class_name, matched_func_path_key)
    fn find_built_name_in_chain(
        call: &FunctionCall,
        built_name_funcs: &HashMap<String, usize>,
    ) -> Option<(String, String)> {
        let ident = call.identifier()?;
        let func_names = ident.names();
        if func_names.is_empty() { return None; }
        let func_path = func_names.join(".");

        let matched = built_name_funcs.iter().find_map(|(path, idx)| {
            if func_path == *path || func_path.ends_with(&format!(".{}", path.split('.').last().unwrap_or(""))) {
                Some((*idx, path.clone()))
            } else {
                None
            }
        });
        if let Some((param_idx, matched_path)) = matched {
            let arg_list = call.arguments()?;
            let call_args = arg_list.expressions();
            if let Some(Expression::Literal(lit)) = call_args.get(param_idx - 1) {
                if let Some(s) = lit.get_string() {
                    return Some((s.trim_matches(|c| c == '"' || c == '\'').to_string(), matched_path));
                }
            }
            return None;
        }

        // Not a built-name call — check nested chain
        let nested = ident.syntax().children().find_map(FunctionCall::cast)?;
        find_built_name_in_chain(&nested, &built_name_funcs)
    }

    // Helper: walk a FunctionCall chain and extract fields from @builds-field methods.
    // Returns Vec<(field_name, field_type, Visibility)> for all builder calls in the chain.
    fn extract_built_fields_from_chain(
        call: &FunctionCall,
        schema_class: &str,
        builds_field_funcs: &HashMap<String, BuildsFieldInfo>,
    ) -> Vec<(String, AnnotationType, Visibility)> {
        let mut fields = Vec::new();
        collect_built_fields(call, schema_class, builds_field_funcs, &mut fields);
        fields
    }

    fn collect_built_fields(
        call: &FunctionCall,
        schema_class: &str,
        builds_field_funcs: &HashMap<String, BuildsFieldInfo>,
        fields: &mut Vec<(String, AnnotationType, Visibility)>,
    ) {
        let Some(ident) = call.identifier() else { return };

        // Check if this call is a @builds-field method
        let names = ident.names();
        if let Some(method_name) = names.last() {
            let method_path = format!("{}.{}", schema_class, method_name);
            if let Some(info) = builds_field_funcs.get(&method_path) {
                // Extract field name from string literal at param_idx - 1
                if let Some(arg_list) = call.arguments() {
                    let args = arg_list.expressions();
                    if let Some(Expression::Literal(lit)) = args.get(info.param_idx - 1) {
                        if let Some(s) = lit.get_string() {
                            let field_name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
                            // Resolve generic type params from backtick call arguments
                            let field_type = resolve_builds_field_generics(
                                &info.field_type, &info.generics, &info.params, &args,
                            );
                            fields.push((field_name, field_type, Visibility::Public));
                        }
                    }
                }
            }
        }

        // Recurse into nested FunctionCall in the identifier (inner chain call)
        if let Some(nested) = ident.syntax().children().find_map(FunctionCall::cast) {
            collect_built_fields(&nested, schema_class, builds_field_funcs, fields);
        }
    }

    /// Resolve generic type params in a @builds-field type using call arguments.
    /// For each generic T, find the backtick param position (`T`) and extract
    /// the class name from the string literal argument at that position.
    fn resolve_builds_field_generics(
        field_type: &AnnotationType,
        generics: &[(String, Option<String>)],
        params: &[ParamInfo],
        call_args: &[Expression],
    ) -> AnnotationType {
        if generics.is_empty() {
            return field_type.clone();
        }
        // Build substitution map: generic_name → class_name from backtick params
        let mut subs: HashMap<String, String> = HashMap::new();
        for (gen_name, _) in generics {
            // Find param with Backtick(Simple(gen_name)) type
            for (i, param) in params.iter().enumerate() {
                if let AnnotationType::Backtick(inner) = &param.typ {
                    if let AnnotationType::Simple(name) = inner.as_ref() {
                        if name == gen_name {
                            // Get the string literal at this arg position
                            if let Some(Expression::Literal(lit)) = call_args.get(i) {
                                if let Some(s) = lit.get_string() {
                                    subs.insert(gen_name.clone(), s.trim_matches(|c| c == '"' || c == '\'').to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        if subs.is_empty() {
            return field_type.clone();
        }
        substitute_annotation_generics(field_type, &subs)
    }

    /// Substitute generic type param names in an AnnotationType.
    fn substitute_annotation_generics(at: &AnnotationType, subs: &HashMap<String, String>) -> AnnotationType {
        match at {
            AnnotationType::Simple(name) => {
                if let Some(replacement) = subs.get(name) {
                    AnnotationType::Simple(replacement.clone())
                } else {
                    at.clone()
                }
            }
            AnnotationType::Union(types) => {
                AnnotationType::Union(types.iter().map(|t| substitute_annotation_generics(t, subs)).collect())
            }
            AnnotationType::Array(inner) => {
                AnnotationType::Array(Box::new(substitute_annotation_generics(inner, subs)))
            }
            AnnotationType::Parameterized(name, args) => {
                AnnotationType::Parameterized(name.clone(), args.iter().map(|t| substitute_annotation_generics(t, subs)).collect())
            }
            _ => at.clone(),
        }
    }

    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for stmt in block.statements() {
        let rhs_call = match &stmt {
            Statement::LocalAssign(la) => {
                la.expression_list().and_then(|el| {
                    let exprs = el.expressions();
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(c.clone()) } else { None } } else { None }
                })
            }
            Statement::Assign(a) => {
                a.expression_list().and_then(|el| {
                    let exprs = el.expressions();
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(c.clone()) } else { None } } else { None }
                })
            }
            // Expression statements: ClassName._FIELD:Extend("Name"):...:Commit()
            Statement::FunctionCall(c) => Some(c.clone()),
            _ => None,
        };
        let Some(call) = rhs_call else { continue };

        if let Some((name, matched_path)) = find_built_name_in_chain(&call, &built_name_funcs) {
            if seen.insert(name.clone()) {
                // Look up parent from @return built : Parent on the schema class
                let schema_class = func_path_to_schema.get(&matched_path);
                let parents: Vec<String> = schema_class
                    .and_then(|schema| schema_built_parent.get(schema))
                    .cloned()
                    .into_iter()
                    .collect();
                // Extract built fields from @builds-field methods in the chain
                let fields = schema_class
                    .map(|sc| extract_built_fields_from_chain(&call, sc, &builds_field_funcs))
                    .unwrap_or_default();
                results.push(ClassDecl {
                    name,
                    type_params: Vec::new(),
                    parents,
                    fields,
                    accessors: Vec::new(),
                    overloads: Vec::new(),
                    generics: Vec::new(),
                    constructor_methods: Vec::new(),
                    constraint_type_arg_subs: Vec::new(),
                    field_built_names: HashMap::new(),
                });
            }
        }
    }
    results
}

// ── Type conversion ──────────────────────────────────────────────────────────

pub(crate) fn resolve_annotation_type(
    at: &AnnotationType, generics: &[(String, Option<String>)],
    classes: &std::collections::HashMap<String, usize>,
    aliases: &std::collections::HashMap<String, ValueType>,
) -> Option<ValueType> {
    match at {
        AnnotationType::Simple(name) => {
            if generics.iter().any(|(g, _)| g == name) { return Some(ValueType::TypeVariable(name.clone())); }
            match name.as_str() {
                "nil" => return Some(ValueType::Nil),
                "boolean" | "bool" => return Some(ValueType::Boolean(None)),
                "true" => return Some(ValueType::Boolean(Some(true))),
                "false" => return Some(ValueType::Boolean(Some(false))),
                "number" | "integer" => return Some(ValueType::Number),
                "string" => return Some(ValueType::String(None)),
                "table" => return Some(ValueType::Table(None)),
                "function" | "fun" => return Some(ValueType::Function(None)),
                "any" => return Some(ValueType::Any),
                "userdata" => return Some(ValueType::Userdata),
                "thread" => return Some(ValueType::Thread),
                _ => {}
            }
            // fun(...) is now parsed as AnnotationType::Fun; this handles legacy Simple strings
            if name.starts_with("fun(") { return Some(ValueType::Function(None)); }
            if (name.starts_with('"') && name.ends_with('"'))
                || (name.starts_with('\'') && name.ends_with('\''))
            {
                let stripped = name.trim_matches(|c| c == '"' || c == '\'');
                return Some(ValueType::String(Some(stripped.to_string())));
            }
            if let Some(&table_idx) = classes.get(name.as_str()) { return Some(ValueType::Table(Some(table_idx))); }
            if let Some(vt) = aliases.get(name.as_str()) { return Some(vt.clone()); }
            None
        }
        AnnotationType::Union(parts) => {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| resolve_annotation_type(p, generics, classes, aliases)).collect();
            match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => {
                    let mut iter = converted.into_iter();
                    let mut result = iter.next().unwrap();
                    for vt in iter { result = ValueType::union(result, vt); }
                    Some(result)
                }
            }
        }
        AnnotationType::Array(_inner) => Some(ValueType::Table(None)),
        AnnotationType::Parameterized(base, _args) => {
            resolve_annotation_type(&AnnotationType::Simple(base.clone()), generics, classes, aliases)
        }
        AnnotationType::Backtick(inner) => resolve_annotation_type(inner, generics, classes, aliases),
        AnnotationType::Fun(..) => Some(ValueType::Function(None)),
    }
}

#[allow(dead_code)]
pub fn annotation_type_to_value_type(at: &AnnotationType) -> Option<ValueType> {
    match at {
        AnnotationType::Simple(name) => match name.as_str() {
            "nil" => Some(ValueType::Nil), "boolean" | "bool" => Some(ValueType::Boolean(None)),
            "true" => Some(ValueType::Boolean(Some(true))), "false" => Some(ValueType::Boolean(Some(false))),
            "number" | "integer" => Some(ValueType::Number), "string" => Some(ValueType::String(None)),
            "table" => Some(ValueType::Table(None)), "function" | "fun" => Some(ValueType::Function(None)),
            "any" => Some(ValueType::Any),
            "userdata" => Some(ValueType::Userdata), "thread" => Some(ValueType::Thread),
            _ => None,
        },
        AnnotationType::Union(parts) => {
            let converted: Vec<ValueType> = parts.iter().filter_map(annotation_type_to_value_type).collect();
            match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => {
                    let mut iter = converted.into_iter();
                    let mut result = iter.next().unwrap();
                    for vt in iter { result = ValueType::union(result, vt); }
                    Some(result)
                }
            }
        }
        AnnotationType::Array(_) => Some(ValueType::Table(None)),
        AnnotationType::Parameterized(base, _) => annotation_type_to_value_type(&AnnotationType::Simple(base.clone())),
        AnnotationType::Backtick(inner) => annotation_type_to_value_type(inner),
        AnnotationType::Fun(..) => Some(ValueType::Function(None)),
    }
}

// ── Diagnostic suppression scanning ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum SuppressionKind { Disable, Enable, DisableLine, DisableNextLine }

#[derive(Debug, Clone)]
pub struct DiagnosticSuppression {
    pub kind: SuppressionKind,
    pub line: u32,
    pub codes: Vec<String>,
}

pub fn scan_diagnostic_directives(root: &SyntaxNode) -> Vec<DiagnosticSuppression> {
    let source = root.text().to_string();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(source.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i + 1))
        .collect();

    let mut suppressions = Vec::new();
    for element in root.descendants_with_tokens() {
        let rowan::NodeOrToken::Token(tok) = element else { continue };
        if tok.kind() != SyntaxKind::Comment { continue; }
        let text = tok.text();
        if let Some(rest) = text.strip_prefix("---@diagnostic") {
            let rest = rest.trim();
            let offset = u32::from(tok.text_range().start()) as usize;
            let line_num = line_starts.partition_point(|&start| start <= offset) as u32 - 1;
            if let Some(directive) = parse_diagnostic_directive(rest, line_num) {
                suppressions.push(directive);
            }
        }
    }
    suppressions
}

fn parse_diagnostic_directive(rest: &str, line: u32) -> Option<DiagnosticSuppression> {
    let (keyword, codes_str) = if let Some((kw, cs)) = rest.split_once(':') {
        (kw.trim(), Some(cs.trim()))
    } else { (rest.trim(), None) };
    let kind = match keyword {
        "disable" => SuppressionKind::Disable, "enable" => SuppressionKind::Enable,
        "disable-line" => SuppressionKind::DisableLine,
        "disable-next-line" => SuppressionKind::DisableNextLine,
        _ => return None,
    };
    let codes = codes_str
        .map(|cs| cs.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect())
        .unwrap_or_default();
    Some(DiagnosticSuppression { kind, line, codes })
}
