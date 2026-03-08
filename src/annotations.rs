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
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: String,
    pub parents: Vec<String>,
    pub fields: Vec<(String, AnnotationType, Visibility)>,
    pub accessors: Vec<(String, Visibility)>,
    pub overloads: Vec<OverloadSig>,
    pub generics: Vec<(String, Option<String>)>,
}

#[derive(Debug, Clone)]
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
    pub class_parents: Vec<String>,
    pub fields: Vec<(String, AnnotationType, Visibility)>,
    pub alias: Option<(String, AnnotationType)>,
    pub alias_continuations: Vec<AnnotationType>,
    pub overloads: Vec<String>,
    pub meta: bool,
    pub deprecated: bool,
    pub nodiscard: bool,
    pub visibility: Visibility,
    pub doc: Option<String>,
    pub generics: Vec<(String, Option<String>)>, // (name, optional constraint type name)
    pub defclass: Option<String>, // generic name that auto-creates classes from backtick inference
    pub accessors: Vec<(String, Visibility)>,
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
            if text.starts_with("---@") || text.starts_with("---|") {
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
            if text.starts_with("---@") || text.starts_with("---|") {
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
        classes.push(ClassDecl { name: class_name, parents: block.class_parents, fields: block.fields, accessors: block.accessors, overloads, generics: block.generics });
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
                block.class = Some(class_name.to_string());
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
                let typ = parse_type(type_str.trim());
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
                let type_only = extract_type_prefix(type_str.trim());
                let typ = parse_type(type_only);
                block.params.push(ParamInfo {
                    name: name.to_string(),
                    typ,
                    optional: is_optional,
                });
            }
        } else if let Some(rest) = content.strip_prefix("@return") {
            let rest = rest.trim();
            for type_str in split_return_types(rest) {
                let type_str = type_str.trim();
                if !type_str.is_empty() {
                    // split_return_types already strips @description text and handles
                    // fun() multi-return commas. For fun() types, use the full string;
                    // for simple types, extract the type prefix (first word).
                    let type_only = if type_str.starts_with("fun(") {
                        type_str
                    } else {
                        extract_type_prefix(type_str)
                    };
                    block.returns.push(parse_type(type_only));
                }
            }
        } else if let Some(rest) = content.strip_prefix("@type") {
            let rest = rest.trim();
            if !rest.is_empty() { block.var_type = Some(parse_type(rest)); }
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
                block.defclass = Some(rest.split_whitespace().next().unwrap().to_string());
            }
        } else if content.starts_with("@deprecated") {
            block.deprecated = true;
        } else if content.starts_with("@nodiscard") {
            block.nodiscard = true;
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
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '{' => { depth += 1; after_colon = false; }
            '>' | ')' | '}' => { depth = depth.saturating_sub(1); after_colon = false; }
            ':' if depth == 0 => { after_colon = true; }
            c if c.is_whitespace() && depth == 0 && !after_colon => return &s[..i],
            _ => { after_colon = false; }
        }
    }
    s
}

fn split_at_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
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
    AnnotationType::Simple(s.to_string())
}

/// Parsed overload signature from `---@overload fun(...): ret`.
#[derive(Debug, Clone)]
pub struct OverloadSig {
    pub params: Vec<ParamInfo>,
    pub returns: Vec<AnnotationType>,
    pub is_vararg: bool,
}

/// Parse an overload string like `fun(param: type, ...): retType`.
pub fn parse_overload(s: &str) -> Option<OverloadSig> {
    let s = s.trim();
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
                params.push(ParamInfo { name, typ: ann_type, optional });
            } else {
                let optional = part.ends_with('?');
                params.push(ParamInfo {
                    name: part.trim_end_matches('?').to_string(),
                    typ: AnnotationType::Simple("any".to_string()),
                    optional,
                });
            }
        }
    }

    let returns = if let Some(ret_str) = after_paren.strip_prefix(':') {
        let ret_str = ret_str.trim();
        if ret_str.is_empty() { Vec::new() }
        else { split_params(ret_str).iter().map(|r| parse_type(r.trim())).collect() }
    } else { Vec::new() };

    Some(OverloadSig { params, returns, is_vararg })
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
pub enum FieldValueKind { String, Number, Boolean, Nil, Table, Function, FunctionCall(Vec<String>), Unknown }

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
    pub visibility: Visibility,
    pub generics: Vec<(String, Option<String>)>,
    pub defclass: Option<String>,
    pub source_path: Option<PathBuf>,
    pub def_start: u32,
    pub def_end: u32,
    /// Intermediate path components (e.g. ["__private"] for `Class.__private:Method`)
    pub intermediates: Vec<String>,
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
                    let params = if annotations.params.is_empty() {
                        if let Some(param_list) = func.params() {
                            param_list.parameters().into_iter()
                                .filter(|n| n != "self")
                                .map(|n| ParamInfo { name: n, typ: AnnotationType::Simple("any".to_string()), optional: false })
                                .collect()
                        } else { Vec::new() }
                    } else { annotations.params };
                    if names.len() == 1 {
                        globals.push(ExternalGlobal {
                            name: names[0].clone(), kind: ExternalGlobalKind::Function,
                            params, returns: annotations.returns, overloads,
                            doc: annotations.doc, deprecated: annotations.deprecated,
                            nodiscard: annotations.nodiscard, visibility: annotations.visibility,
                            generics: annotations.generics, defclass: annotations.defclass,
                            source_path: owned_path.clone(),
                            def_start, def_end, intermediates: Vec::new(),
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
                                nodiscard: annotations.nodiscard, visibility: annotations.visibility,
                                generics: annotations.generics, defclass: annotations.defclass,
                                source_path: owned_path.clone(),
                                def_start, def_end, intermediates: Vec::new(),
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
                                nodiscard: annotations.nodiscard, visibility: annotations.visibility,
                                generics: annotations.generics, defclass: annotations.defclass,
                                source_path: owned_path.clone(),
                                def_start, def_end, intermediates,
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
                            let kind = match &exprs[0] {
                                Expression::TableConstructor(_) => ExternalGlobalKind::Table,
                                Expression::Literal(lit) => {
                                    let vk = if lit.get_string().is_some() { FieldValueKind::String }
                                        else if lit.get_bool().is_some() { FieldValueKind::Boolean }
                                        else if lit.get_number().is_some() { FieldValueKind::Number }
                                        else if lit.is_nil() { FieldValueKind::Nil }
                                        else { FieldValueKind::Unknown };
                                    ExternalGlobalKind::Variable(vk)
                                }
                                Expression::Function(_) => ExternalGlobalKind::Variable(FieldValueKind::Function),
                                Expression::Identifier(ident) => {
                                    let rhs_names = ident.names();
                                    if rhs_names.len() == 2 {
                                        let table_name = local_aliases.get(&rhs_names[0])
                                            .cloned().unwrap_or_else(|| rhs_names[0].clone());
                                        ExternalGlobalKind::FieldRef(table_name, rhs_names[1].clone())
                                    } else {
                                        ExternalGlobalKind::Variable(FieldValueKind::Unknown)
                                    }
                                }
                                _ => ExternalGlobalKind::Variable(FieldValueKind::Unknown),
                            };
                            globals.push(ExternalGlobal {
                                name: names[0].clone(), kind,
                                params: Vec::new(), returns: Vec::new(), overloads: Vec::new(),
                                doc: None, deprecated: false, nodiscard: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                intermediates: Vec::new(),
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
                                        FieldValueKind::FunctionCall(callee_names)
                                    } else {
                                        FieldValueKind::Unknown
                                    }
                                }
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
                                doc: annotations.doc, deprecated: false, nodiscard: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                intermediates: Vec::new(),
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
                                doc: annotations.doc, deprecated: false, nodiscard: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                intermediates: Vec::new(),
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

/// Walk a function call chain to find the innermost call with a string literal first argument.
/// For `Y:Init("ClassName")` returns `Some("ClassName")`.
/// For `Y:From("Z"):Include("ClassName")` returns `Some("ClassName")` (outermost call's arg).
/// Returns None if no string literal first argument is found.
fn extract_string_arg_from_call_chain(call: &FunctionCall) -> Option<String> {
    // Check this call's first argument
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
    // Check nested call in the identifier (for method chains)
    let ident = call.identifier()?;
    let nested = ident.syntax().children().find_map(FunctionCall::cast)?;
    extract_string_arg_from_call_chain(&nested)
}

/// Scan for `local X = Y.func("ClassName")` calls where `Y.func` has `@defclass`.
/// Returns ClassDecl entries for discovered classes, with parent info from generic constraints.
/// `all_globals` should contain globals from ALL scanned files (not just this file).
pub fn scan_defclass_calls(root: &SyntaxNode, all_globals: &[ExternalGlobal]) -> Vec<ClassDecl> {
    use std::collections::HashMap;
    let Some(block) = Block::cast(root.clone()) else { return Vec::new() };

    // Build map of dotted function names → parent class from generic constraint
    // e.g. "LibTSMClass.DefineClass" → Some("LibTSMBaseClass") if @generic T : LibTSMBaseClass
    let mut defclass_funcs: HashMap<String, Vec<String>> = HashMap::new();
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
            .filter_map(|(_, c)| c.clone())
            .collect();
        defclass_funcs.insert(func_path, parents);
    }
    if defclass_funcs.is_empty() { return Vec::new(); }

    // Helper: walk a FunctionCall chain to find the innermost defclass call.
    // For `DefineClass("X"):AddDep("y"):AddDep("z")`, walks through the nested
    // FunctionCall nodes in the Identifier to find the one matching a defclass func.
    fn find_defclass_in_chain(
        call: &FunctionCall,
        defclass_funcs: &HashMap<String, Vec<String>>,
    ) -> Option<(String, Vec<String>)> {
        let ident = call.identifier()?;
        let func_names = ident.names();
        if func_names.is_empty() { return None; }
        let func_path = func_names.join(".");

        // Check if this call itself is a defclass function
        let matched = defclass_funcs.iter().find_map(|(dc, parents)| {
            if func_path == *dc || func_path.ends_with(&format!(".{}", dc.split('.').last().unwrap_or(""))) {
                Some(parents.clone())
            } else {
                None
            }
        });
        if let Some(parents) = matched {
            let arg_list = call.arguments()?;
            let call_args = arg_list.expressions();
            if let Some(Expression::Literal(lit)) = call_args.first() {
                if let Some(s) = lit.get_string() {
                    let name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
                    return Some((name, parents));
                }
            }
            return None;
        }

        // Not a defclass call — check if the identifier contains a nested FunctionCall (method chain)
        let nested = ident.syntax().children().find_map(FunctionCall::cast)?;
        find_defclass_in_chain(&nested, defclass_funcs)
    }

    let mut results = Vec::new();
    for stmt in block.statements() {
        // Extract the single RHS expression from local or non-local assignments
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
            _ => None,
        };
        let Some(call) = rhs_call else { continue };

        if let Some((name, parents)) = find_defclass_in_chain(&call, &defclass_funcs) {
            results.push(ClassDecl {
                name,
                parents,
                fields: Vec::new(),
                accessors: Vec::new(),
                overloads: Vec::new(),
                generics: Vec::new(),
            });
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
                "string" => return Some(ValueType::String),
                "table" => return Some(ValueType::Table(None)),
                "function" | "fun" => return Some(ValueType::Function(None)),
                "any" => return None,
                _ => {}
            }
            // fun(...) is now parsed as AnnotationType::Fun; this handles legacy Simple strings
            if name.starts_with("fun(") { return Some(ValueType::Function(None)); }
            if (name.starts_with('"') && name.ends_with('"'))
                || (name.starts_with('\'') && name.ends_with('\''))
            { return Some(ValueType::String); }
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
            "number" | "integer" => Some(ValueType::Number), "string" => Some(ValueType::String),
            "table" => Some(ValueType::Table(None)), "function" | "fun" => Some(ValueType::Function(None)),
            "any" => None, _ => None,
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
