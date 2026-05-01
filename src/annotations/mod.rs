use std::collections::HashMap;
use crate::ast::AstNode;
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::{ResolvedOverload, ValueType};
use annotation_types::{strip_return_description, find_hash_comment, extract_type_prefix};

// ── Annotation types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AnnotationType {
    Simple(String),
    Union(Vec<AnnotationType>),
    Array(Box<AnnotationType>),                  // T[], integer[]
    Parameterized(String, Vec<AnnotationType>),  // table<K, V>
    Backtick(Box<AnnotationType>),               // `T` — infer from string literal as class name
    Fun(Vec<ParamInfo>, Vec<AnnotationType>, bool), // fun(x: T): R — params, returns, is_vararg
    NonNil(Box<AnnotationType>),                 // T! — non-nil assertion / lateinit
    Intersection(Vec<AnnotationType>),            // T & U — intersection of types
    TableLiteral(Vec<(String, AnnotationType)>),  // {field: type, ...} — anonymous table shape
    VarArgs(Box<AnnotationType>),                // ...T — variadic return expansion
    /// `(T1 name1, T2 name2, ...)` — multi-value return tuple. Only valid in
    /// return position (top-level of `@return`, inside `fun(): ...`, as an
    /// `@alias` body). The optional `description` is per-case text from the
    /// trailing comment on the tuple's line (e.g. `(true, number) success`).
    /// A `Union` whose members are all `Tuple` is a correlated tuple-union
    /// (`(true, number) | (false, nil)`).
    Tuple(Vec<TuplePosition>, Option<String>),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TuplePosition {
    pub typ: AnnotationType,
    pub name: Option<String>,
}

/// Check if an annotation type is nullable (contains nil at the top level).
pub(crate) fn annotation_type_is_nullable(ann: &AnnotationType) -> bool {
    match ann {
        AnnotationType::Simple(s) => s == "nil",
        AnnotationType::Union(members) => members.iter().any(annotation_type_is_nullable),
        AnnotationType::NonNil(_) => false,
        AnnotationType::Intersection(_) => false,
        _ => false,
    }
}

/// Check if an annotation type contains a `Backtick(...)` anywhere (including inside unions).
pub(crate) fn annotation_contains_backtick(ann: &AnnotationType) -> bool {
    match ann {
        AnnotationType::Backtick(_) => true,
        AnnotationType::Union(members) => members.iter().any(annotation_contains_backtick),
        AnnotationType::Intersection(members) => members.iter().any(annotation_contains_backtick),
        AnnotationType::NonNil(inner) => annotation_contains_backtick(inner),
        AnnotationType::Tuple(positions, _) => positions.iter().any(|p| annotation_contains_backtick(&p.typ)),
        _ => false,
    }
}

pub(crate) fn value_type_to_name(vt: &ValueType, ir: &crate::analysis::Ir) -> Option<String> {
    match vt {
        ValueType::String(None) => Some("string".to_string()),
        ValueType::Number => Some("number".to_string()),
        ValueType::Boolean(None) => Some("boolean".to_string()),
        ValueType::Nil => Some("nil".to_string()),
        ValueType::Any => Some("any".to_string()),
        ValueType::Table(Some(idx)) => ir.table(*idx).class_name.clone(),
        ValueType::Table(None) => Some("table".to_string()),
        ValueType::Function(None) => Some("function".to_string()),
        _ => None,
    }
}

pub(crate) fn resolve_primitive_type_name(name: &str) -> Option<ValueType> {
    match name {
        "string" => Some(ValueType::String(None)),
        "number" | "integer" => Some(ValueType::Number),
        "boolean" | "bool" => Some(ValueType::Boolean(None)),
        "table" => Some(ValueType::Table(None)),
        "function" | "fun" => Some(ValueType::Function(None)),
        "any" | "unknown" => Some(ValueType::Any),
        "nil" => Some(ValueType::Nil),
        _ => None,
    }
}

#[derive(Debug)]
pub(crate) struct SelfFieldEntry {
    pub(crate) name: String,
    pub(crate) annotation_type: AnnotationType,
    pub(crate) byte_range: Option<(u32, u32)>,
}

#[derive(Debug)]
pub(crate) struct TypedSelfField {
    pub(crate) class_name: String,
    pub(crate) field_name: String,
    pub(crate) annotation_type: AnnotationType,
    pub(crate) visibility: Visibility,
    pub(crate) byte_range: (u32, u32),
}

/// Expand a `Simple(name)` annotation that refers to a tuple-form alias into
/// the alias body. Also unwraps the `Simple` when it's the only member of a
/// one-element `Union`. Leaves other annotations unchanged.
pub(crate) fn expand_tuple_form_alias(
    ann: &AnnotationType,
    tuple_form_aliases: &std::collections::HashMap<String, AnnotationType>,
) -> AnnotationType {
    if let AnnotationType::Simple(name) = ann
        && let Some(body) = tuple_form_aliases.get(name) {
            return body.clone();
        }
    ann.clone()
}

/// Extract the tuple-union cases from an annotation that passed
/// `annotation_is_tuple_form`. Returns `(positions, description)` per case.
pub(crate) fn tuple_form_cases(ann: &AnnotationType) -> Vec<(Vec<TuplePosition>, Option<String>)> {
    match ann {
        AnnotationType::Tuple(positions, description) => {
            vec![(positions.clone(), description.clone())]
        }
        AnnotationType::Union(members) => members.iter().filter_map(|m| {
            if let AnnotationType::Tuple(p, d) = m {
                Some((p.clone(), d.clone()))
            } else { None }
        }).collect(),
        _ => Vec::new(),
    }
}

/// Shared tuple-union lowering. Given the parsed cases and a type resolver,
/// produces the per-position column-union `ValueType`s and raw `AnnotationType`s,
/// the label vector sourced from the first case, and one return-only
/// `ResolvedOverload` per case (empty when there's only a single case — nothing
/// to discriminate between).
pub(crate) fn lower_tuple_form_cases<F>(
    cases: &[(Vec<TuplePosition>, Option<String>)],
    mut resolve: F,
) -> (Vec<ValueType>, Vec<AnnotationType>, Vec<Option<String>>, Vec<ResolvedOverload>)
where F: FnMut(&AnnotationType) -> Option<ValueType>,
{
    // Arity is the max across cases — shorter cases are implicitly padded with
    // nil at missing positions, mirroring Lua's runtime semantics for missing
    // return values. E.g. `(number, ...any) | (nil)` gives column 1 = number|nil
    // and column 2 = any|nil.
    let arity = cases.iter().map(|(p, _)| p.len()).max().unwrap_or(0);
    let nil_ann = || AnnotationType::Simple("nil".to_string());
    let mut col_vts = Vec::with_capacity(arity);
    let mut col_raws = Vec::with_capacity(arity);
    for col in 0..arity {
        let types: Vec<AnnotationType> = cases.iter()
            .map(|(p, _)| p.get(col).map(|tp| tp.typ.clone()).unwrap_or_else(nil_ann))
            .collect();
        let raw = if types.len() == 1 { types.into_iter().next().unwrap() }
            else { AnnotationType::Union(types) };
        let vt = resolve(&raw).unwrap_or(ValueType::Any);
        col_vts.push(vt);
        col_raws.push(raw);
    }
    // Per-column label: first case that provides a name at that position wins.
    let labels: Vec<Option<String>> = (0..arity).map(|col| {
        cases.iter().find_map(|(p, _)| p.get(col).and_then(|tp| tp.name.clone()))
    }).collect();
    let overloads: Vec<ResolvedOverload> = if cases.len() > 1 {
        cases.iter().map(|(positions, description)| {
            let returns: Vec<ValueType> = positions.iter()
                .map(|tp| resolve(&tp.typ).unwrap_or(ValueType::Any))
                .collect();
            let has_vararg_tail = matches!(
                positions.last().map(|tp| &tp.typ),
                Some(AnnotationType::VarArgs(_))
            );
            ResolvedOverload {
                params: Vec::new(),
                returns,
                is_return_only: true,
                description: description.clone(),
                has_vararg_tail,
                is_vararg: false,
            }
        }).collect()
    } else {
        Vec::new()
    };
    (col_vts, col_raws, labels, overloads)
}

/// True if `ann` is a `Tuple` or a `Union` every member of which is a `Tuple`.
/// This is the shape produced by the new tuple-union `@return` syntax.
pub(crate) fn annotation_is_tuple_form(ann: &AnnotationType) -> bool {
    match ann {
        AnnotationType::Tuple(..) => true,
        AnnotationType::Union(members) if !members.is_empty() => {
            members.iter().all(|m| matches!(m, AnnotationType::Tuple(..)))
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParamInfo {
    pub name: String,
    pub typ: AnnotationType,
    pub optional: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
}

/// Returns `Protected` for names starting with `_` when `implicit_protected_prefix`
/// is enabled, `Public` otherwise. Used as the default visibility for runtime-discovered
/// fields (e.g. `self._foo = bar`). NOT used for explicit `@field` declarations — those
/// default to `Public` since the author had the opportunity to write `@field protected`.
pub(crate) fn default_visibility_for_name(name: &str, implicit_protected_prefix: bool) -> Visibility {
    if implicit_protected_prefix && name.starts_with('_') {
        Visibility::Protected
    } else {
        Visibility::Public
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CastMode {
    Replace,  // ---@cast x string
    Add,      // ---@cast x +string
    Remove,   // ---@cast x -string
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClassDecl {
    pub name: String,
    pub type_params: Vec<String>,
    #[serde(default)]
    pub type_param_constraints: Vec<Option<String>>,
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
    /// True when the declaration comes from `@enum` rather than `@class`
    pub is_enum: bool,
    /// `@correlated field1, field2, ...` — groups of fields always nil/non-nil together
    pub correlated_groups: Vec<Vec<String>>,
    /// Byte range of the @class comment token: (start_byte, end_byte).
    /// Set during `scan_all_annotations` when the @class comment is found.
    pub def_range: Option<(u32, u32)>,
    /// Source file path, set by the caller after scanning.
    pub def_path: Option<std::path::PathBuf>,
    /// Per-field byte ranges from `@field` annotation tokens: field name → (start, end).
    #[serde(default)]
    pub field_ranges: HashMap<String, (u32, u32)>,
    /// Per-field source file paths, for fields discovered in a different file than `def_path`.
    /// When present, overrides `def_path` for that field's location in `field_locations`.
    #[serde(default)]
    pub field_paths: HashMap<String, std::path::PathBuf>,
    /// `@see <target>` — cross-reference link(s) attached to this `@class`. Doc-only.
    #[serde(default)]
    pub see: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AliasDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub typ: AnnotationType,
    /// Byte range of the @alias comment token: (start_byte, end_byte).
    pub def_range: Option<(u32, u32)>,
    /// Source file path, set by the caller after scanning.
    pub def_path: Option<std::path::PathBuf>,
}

/// Recursive field entry from a defclass table literal.
/// Leaves have empty `children`; nested table constructors have children.
#[derive(Debug, Clone)]
pub(crate) struct DefclassFieldEntry {
    pub(crate) name: String,
    pub(crate) children: Vec<DefclassFieldEntry>,
    pub(crate) name_start: u32,
    pub(crate) name_end: u32,
}

/// Recursively extract named field entries from a table constructor.
pub(crate) fn extract_table_literal_fields(tc: &crate::ast::TableConstructor<'_>) -> Vec<DefclassFieldEntry> {
    use crate::ast::{Expression, FieldKind};
    use crate::syntax::tree::NodeOrToken;
    use crate::syntax::SyntaxKind;
    tc.fields().into_iter().filter_map(|f| {
        match f.kind() {
            Some(FieldKind::Named { name, value }) => {
                // Capture the Name token's byte range for go-to-definition
                let (name_start, name_end) = f.syntax().children_with_tokens()
                    .find_map(|n| match n {
                        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => {
                            let r = t.text_range();
                            Some((u32::from(r.start()), u32::from(r.end())))
                        }
                        _ => None,
                    })
                    .unwrap_or((0, 0));
                let children = if let Expression::TableConstructor(inner_tc) = &value {
                    let inner = extract_table_literal_fields(inner_tc);
                    if inner.is_empty() { Vec::new() } else { inner }
                } else {
                    Vec::new()
                };
                Some(DefclassFieldEntry { name, children, name_start, name_end })
            }
            _ => None,
        }
    }).collect()
}

pub struct ScanResult {
    pub classes: Vec<ClassDecl>,
    pub aliases: Vec<AliasDecl>,
    pub events: Vec<EventDecl>,
    pub has_meta: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventDecl {
    pub event_type: String,
    pub event_name: String,
    pub params: Vec<crate::pre_globals::EventPayloadParam>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AnnotationBlock {
    pub(crate) params: Vec<ParamInfo>,
    pub(crate) returns: Vec<AnnotationType>,
    pub(crate) return_names: Vec<Option<String>>,
    pub(crate) return_descriptions: Vec<Option<String>>,
    pub(crate) var_type: Option<AnnotationType>,
    pub(crate) class: Option<String>,
    pub(crate) class_type_params: Vec<String>,
    pub(crate) class_type_param_constraints: Vec<Option<String>>,
    pub(crate) class_parents: Vec<String>,
    pub(crate) fields: Vec<(String, AnnotationType, Visibility)>,
    pub(crate) alias: Option<(String, AnnotationType)>,
    pub(crate) alias_type_params: Vec<String>,
    pub(crate) alias_continuations: Vec<AnnotationType>,
    pub(crate) overloads: Vec<String>,
    pub(crate) meta: bool,
    pub(crate) deprecated: bool,
    pub(crate) nodiscard: bool,
    pub(crate) constructor: bool,
    pub(crate) constructor_methods: Vec<String>,
    pub(crate) visibility: Visibility,
    pub(crate) doc: Option<String>,
    pub(crate) generics: Vec<(String, Option<String>)>,
    pub(crate) defclass: Option<String>,
    pub(crate) defclass_parent: Option<String>,
    pub(crate) accessors: Vec<(String, Visibility)>,
    pub(crate) builds_field: Option<(usize, AnnotationType)>,
    pub(crate) built_name: Option<usize>,
    pub(crate) built_extends: bool,
    pub(crate) type_narrows: Option<(usize, usize)>,
    pub(crate) type_narrows_class: Option<String>,
    pub(crate) is_enum: bool,
    pub(crate) correlated_groups: Vec<Vec<String>>,
    pub(crate) see: Vec<String>,
    pub(crate) flavor_guard: u8,
    pub(crate) event_type: Option<String>,
    pub(crate) event_name: Option<String>,
}

// ── Comment extraction ───────────────────────────────────────────────────────

/// Extract LuaLS-style annotations from comments preceding a syntax node.
///
/// Walks backward through the token stream from the node's start position,
/// collecting `---@` comment lines. This approach works regardless of which
/// parent node the trivia tokens are attached to (rowan attaches trailing
/// trivia to the preceding construct, so comments before a function can end
/// up inside the preceding statement's expression list).
pub(crate) fn extract_annotations(node: SyntaxNode<'_>) -> AnnotationBlock {
    // Find the first token of our node, then walk backward through preceding tokens
    let Some(first_token) = node.first_token() else { return AnnotationBlock::default(); };

    let mut annotation_lines = Vec::new();
    let mut doc_lines = Vec::new();
    let mut tok = first_token.prev_token();
    let mut newlines_since_comment = 0u32;
    while let Some(token) = tok {
        let kind = token.kind();
        if kind == SyntaxKind::Whitespace {
            tok = token.prev_token();
            continue;
        }
        if kind == SyntaxKind::Newline {
            newlines_since_comment += 1;
            if newlines_since_comment >= 2 {
                break;
            }
            tok = token.prev_token();
            continue;
        }
        newlines_since_comment = 0;
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
            if is_annotation_comment(text) {
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

/// Classify a Lua line comment as an annotation comment (`---@tag`) or a
/// tuple-union continuation (`---|`). Accepts any amount of whitespace between
/// the `---` prefix and the `@` / `|` sigil so that indented continuation lines
/// (`---      | (...)`) are recognized.
fn is_annotation_comment(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("---") else { return false; };
    let rest = rest.trim_start_matches([' ', '\t']);
    rest.starts_with('@') || rest.starts_with('|')
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

/// Scan all comments in the syntax tree for @class, @alias, and @event declarations.
pub fn scan_all_annotations(root: SyntaxNode<'_>) -> ScanResult {
    let mut classes = Vec::new();
    let mut aliases = Vec::new();
    let mut events = Vec::new();
    let mut has_meta = false;

    let mut current_group: Vec<(String, u32, u32)> = Vec::new();
    let mut current_class_range: Option<(u32, u32)> = None;
    let mut current_alias_range: Option<(u32, u32)> = None;
    let mut prev_was_newline = false;

    for event in root.descendants_with_tokens() {
        let NodeOrToken::Token(tok) = event else { continue };
        let kind = tok.kind();
        if kind == SyntaxKind::Comment {
            let text = tok.text();
            if is_annotation_comment(text) {
                // If this starts a new @class, @alias, or @event and the current group already
                // contains one, flush the previous group first so each declaration
                // becomes its own group (block.alias/class/event is Option and would be overwritten).
                if !current_group.is_empty() {
                    let starts_new_decl = text.contains("@class ") || text.contains("@alias ") || text.contains("@event ");
                    let group_has_decl = starts_new_decl && current_group.iter().any(|(l, _, _)| l.contains("@class ") || l.contains("@alias ") || l.contains("@event "));
                    if group_has_decl {
                        flush_group(&current_group, current_class_range, current_alias_range, &mut classes, &mut aliases, &mut events, &mut has_meta);
                        current_group.clear();
                        current_class_range = None;
                        current_alias_range = None;
                    }
                }
                if text.contains("@class ") && current_class_range.is_none() {
                    let r = tok.text_range();
                    current_class_range = Some((u32::from(r.start()), u32::from(r.end())));
                }
                if text.contains("@alias ") && current_alias_range.is_none() {
                    let r = tok.text_range();
                    current_alias_range = Some((u32::from(r.start()), u32::from(r.end())));
                }
                let r = tok.text_range();
                current_group.push((text.to_string(), u32::from(r.start()), u32::from(r.end())));
            }
            prev_was_newline = false;
        } else if kind == SyntaxKind::Newline {
            if prev_was_newline && !current_group.is_empty() {
                flush_group(&current_group, current_class_range, current_alias_range, &mut classes, &mut aliases, &mut events, &mut has_meta);
                current_group.clear();
                current_class_range = None;
                current_alias_range = None;
            }
            prev_was_newline = true;
        } else if kind == SyntaxKind::Whitespace {
        } else {
            flush_group(&current_group, current_class_range, current_alias_range, &mut classes, &mut aliases, &mut events, &mut has_meta);
            current_group.clear();
            current_class_range = None;
            current_alias_range = None;
            prev_was_newline = false;
        }
    }
    flush_group(&current_group, current_class_range, current_alias_range, &mut classes, &mut aliases, &mut events, &mut has_meta);

    ScanResult { classes, aliases, events, has_meta }
}

fn flush_group(
    lines: &[(String, u32, u32)],
    class_range: Option<(u32, u32)>,
    alias_range: Option<(u32, u32)>,
    classes: &mut Vec<ClassDecl>,
    aliases: &mut Vec<AliasDecl>,
    events: &mut Vec<EventDecl>,
    has_meta: &mut bool,
) {
    if lines.is_empty() { return; }
    let line_strs: Vec<String> = lines.iter().map(|(s, _, _)| s.clone()).collect();
    let block = parse_annotation_lines(&line_strs);
    if block.meta { *has_meta = true; }
    if let Some(class_name) = block.class {
        // Build per-field byte ranges by matching @field lines to parsed field names
        let mut field_ranges: HashMap<String, (u32, u32)> = HashMap::new();
        for (text, start, end) in lines {
            let content = text.strip_prefix("---@").or_else(|| text.strip_prefix("--- @"));
            if let Some(content) = content
                && let Some(rest) = content.strip_prefix("field")
                    && let Some((_, name, _, _)) = parse_field_header(rest) {
                        field_ranges.insert(name.to_string(), (*start, *end));
                    }
        }
        let overloads = block.overloads.iter().filter_map(|s| parse_overload(s)).collect();
        let is_enum = block.is_enum || class_name.starts_with("Enum.");
        classes.push(ClassDecl { name: class_name, type_params: block.class_type_params, type_param_constraints: block.class_type_param_constraints, parents: block.class_parents, fields: block.fields, accessors: block.accessors, overloads, generics: block.generics, constructor_methods: block.constructor_methods, constraint_type_arg_subs: Vec::new(), field_built_names: HashMap::new(), is_enum, correlated_groups: block.correlated_groups, def_range: class_range, def_path: None, field_ranges, field_paths: HashMap::new(), see: block.see.clone() });
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
        aliases.push(AliasDecl { name, type_params: block.alias_type_params, typ, def_range: alias_range, def_path: None });
    }
    if let (Some(event_type), Some(event_name)) = (block.event_type, block.event_name) {
        let params = block.params.iter().map(|p| {
            crate::pre_globals::EventPayloadParam {
                name: p.name.clone(),
                type_name: crate::annotations::format_annotation_type(&p.typ),
                nilable: p.optional,
                description: p.description.clone(),
            }
        }).collect();
        events.push(EventDecl {
            event_type,
            event_name,
            params,
            documentation: None,
        });
    }
}

/// Parse the header of an `@field` annotation: visibility, field name, and remaining type text.
/// Input is the text after `@field` (e.g. `" private foo? number"`).
/// Returns `(visibility, name_without_?, is_optional, type_text)`.
fn parse_field_header(after_field: &str) -> Option<(Visibility, &str, bool, &str)> {
    let rest = after_field.trim();
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
    let (name, type_str) = rest.split_once(char::is_whitespace)?;
    let is_optional = name.ends_with('?');
    let name = name.trim_end_matches('?');
    Some((vis, name, is_optional, type_str))
}

// ── Line parsing ─────────────────────────────────────────────────────────────

fn parse_annotation_lines(lines: &[String]) -> AnnotationBlock {
    let mut block = AnnotationBlock::default();

    // Tracks whether the most recently parsed annotation was a new-style
    // tuple `@return` so that following `---|` continuation lines merge into
    // it (rather than the `@alias` union). Reset on any other annotation.
    let mut last_tuple_return_idx: Option<usize> = None;

    for line in lines {
        let content = line.trim_start_matches('-');
        let content = content.trim();
        // Break the `@return → ---|` continuation chain at any unrelated annotation
        if !content.starts_with("@return") && !content.starts_with('|') {
            last_tuple_return_idx = None;
        }
        if let Some(rest) = content.strip_prefix("@class") {
            let rest = rest.trim();
            // Strip class modifiers: (partial), (exact) — accepted for compatibility, no effect
            let rest = if let Some(after) = rest.strip_prefix("(partial)") {
                after.trim()
            } else if let Some(after) = rest.strip_prefix("(exact)") {
                after.trim()
            } else {
                rest
            };
            // Extract class name, handling spaces in type params: @class Name<S, T>
            let class_name_end = if let Some(open) = rest.find('<') {
                if let Some(close_offset) = rest[open..].find('>') {
                    open + close_offset + 1
                } else {
                    rest.find(char::is_whitespace).unwrap_or(rest.len())
                }
            } else {
                rest.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(rest.len())
            };
            let class_name_raw = rest[..class_name_end].trim_end_matches(':');
            if !class_name_raw.is_empty() {
                // Parse type params: @class Name<K: string|number, V> → name="Name", type_params=["K","V"], constraints=[Some("string|number"), None]
                let (class_name, type_params, type_param_constraints) = if let Some(open) = class_name_raw.find('<') {
                    let name = &class_name_raw[..open];
                    let params_str = class_name_raw[open+1..].trim_end_matches('>');
                    let mut params = Vec::new();
                    let mut constraints = Vec::new();
                    for part in params_str.split(',') {
                        let part = part.trim();
                        if part.is_empty() { continue; }
                        if let Some((pname, constraint)) = part.split_once(':') {
                            params.push(pname.trim().to_string());
                            let c = constraint.trim();
                            constraints.push(if c.is_empty() { None } else { Some(c.to_string()) });
                        } else {
                            params.push(part.to_string());
                            constraints.push(None);
                        }
                    }
                    (name.to_string(), params, constraints)
                } else {
                    (class_name_raw.to_string(), Vec::new(), Vec::new())
                };
                block.class = Some(class_name);
                block.class_type_params = type_params;
                block.class_type_param_constraints = type_param_constraints;
                // Parse parent classes from the portion after the class name
                let after_class = rest[class_name_end..].trim();
                if let Some(parents_str) = after_class.strip_prefix(':') {
                    let parents_str = parents_str.trim();
                    // Skip inline table type syntax: { [K]: V, ... }
                    if !parents_str.starts_with('{') {
                        for parent in parents_str.split(',') {
                            let parent = parent.trim();
                            if !parent.is_empty() {
                                block.class_parents.push(parent.to_string());
                            }
                        }
                    }
                }
            }
        } else if let Some(rest) = content.strip_prefix("@field") {
            if let Some((vis, name, is_optional, type_str)) = parse_field_header(rest) {
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
            // Extract alias name, handling spaces in type params: @alias Name<K, V> TYPE
            // Only search the first word for '<' to avoid matching '<' in the type body
            // e.g. `@alias BonusIdCurve table<number,number>` — the '<' is in the type, not the name
            let first_ws = rest.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(rest.len());
            let alias_name_end = if let Some(open) = rest[..first_ws].find('<') {
                if let Some(close_offset) = rest[open..].find('>') {
                    open + close_offset + 1
                } else {
                    first_ws
                }
            } else {
                first_ws
            };
            let name_raw = rest[..alias_name_end].trim_end_matches(':');
            let after_name = rest[alias_name_end..].trim();
            // Strip leading colon from type portion (for `@alias Foo<K,V>: TYPE` syntax)
            let type_str = after_name.strip_prefix(':').unwrap_or(after_name).trim();
            if !name_raw.is_empty() {
                // Parse type params: @alias Foo<K, V> TYPE → name="Foo", type_params=["K","V"]
                let (name, type_params) = if let Some(open) = name_raw.find('<') {
                    let n = &name_raw[..open];
                    let params_str = name_raw[open+1..].trim_end_matches('>');
                    let params: Vec<String> = params_str.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    (n.to_string(), params)
                } else {
                    (name_raw.to_string(), Vec::new())
                };
                if !type_str.is_empty() {
                    let typ = parse_type(type_str);
                    block.alias = Some((name, typ));
                } else {
                    // Name-only @alias (multi-line form, types come from ---|  lines)
                    block.alias = Some((name, AnnotationType::Simple("unknown".to_string())));
                }
                block.alias_type_params = type_params;
            }
        } else if let Some(rest) = content.strip_prefix('|') {
            // ---|  continuation line — merge into the active @return tuple union,
            // or fall back to the alias union.
            let rest = rest.trim();
            let rest_no_hash = if let Some(hash_pos) = find_hash_comment(rest) {
                rest[..hash_pos].trim()
            } else {
                rest
            };
            if !rest_no_hash.is_empty() {
                if let Some(idx) = last_tuple_return_idx {
                    let (typ, _name, _desc) = parse_return_line(rest_no_hash, true);
                    let existing = std::mem::replace(&mut block.returns[idx], AnnotationType::Simple(String::new()));
                    let merged = match existing {
                        AnnotationType::Union(mut members) => {
                            members.push(typ);
                            AnnotationType::Union(members)
                        }
                        other => AnnotationType::Union(vec![other, typ]),
                    };
                    block.returns[idx] = merged;
                    continue;
                }
                if block.alias.is_some() {
                    block.alias_continuations.push(parse_type(rest_no_hash));
                }
            }
        } else if let Some(rest) = content.strip_prefix("@param") {
            let rest = rest.trim();
            if let Some((name, type_str)) = rest.split_once(char::is_whitespace) {
                let is_optional = name.ends_with('?');
                let name = name.trim_end_matches('?');
                let type_str_trimmed = type_str.trim();
                let type_only = extract_type_prefix(type_str_trimmed);
                let typ = parse_type(type_only);
                let is_optional = is_optional || annotation_type_is_nullable(&typ);
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
            if !rest.is_empty() {
                // @return built [: Parent] — preserve the full "built : Parent" string
                let type_str_for_built = strip_return_description(rest);
                if type_str_for_built == "built" || type_str_for_built.starts_with("built ") || type_str_for_built.starts_with("built:") {
                    let after_built = type_str_for_built["built".len()..].trim();
                    let parent_part = after_built.strip_prefix(':').map(|p| p.trim());
                    let label = if let Some(parent) = parent_part {
                        let parent_name = parent.split_whitespace().next().unwrap_or(parent);
                        format!("built:{}", parent_name)
                    } else {
                        "built".to_string()
                    };
                    block.returns.push(AnnotationType::Simple(label));
                    block.return_names.push(None);
                    block.return_descriptions.push(None);
                    last_tuple_return_idx = None;
                } else {
                    let (typ, name, desc) = parse_return_line(rest, false);
                    let is_tuple = annotation_is_tuple_form(&typ);
                    block.returns.push(typ);
                    block.return_names.push(name);
                    block.return_descriptions.push(desc);
                    last_tuple_return_idx = if is_tuple { Some(block.returns.len() - 1) } else { None };
                }
            }
        } else if let Some(rest) = content.strip_prefix("@type-narrows") {
            let rest = rest.trim();
            if let Some((a, b)) = rest.split_once(char::is_whitespace) {
                if let (Ok(target), Ok(classname)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                    block.type_narrows = Some((target, classname));
                }
            } else if !rest.is_empty() && rest.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') {
                // @type-narrows ClassName — method-style type guard (self → ClassName)
                block.type_narrows_class = Some(rest.to_string());
            }
        } else if let Some(rest) = content.strip_prefix("@type") {
            let rest = rest.trim();
            if !rest.is_empty() { block.var_type = Some(parse_type(rest)); }
        } else if content.starts_with("@cast") {
            // @cast directives are handled via raw comment lines in build_ir.rs
        } else if let Some(rest) = content.strip_prefix("@event") {
            let rest = rest.trim();
            if let Some((type_name, event_name_raw)) = rest.split_once(char::is_whitespace) {
                let event_name = event_name_raw.trim().trim_matches(|c| c == '"' || c == '\'');
                if !type_name.is_empty() && !event_name.is_empty() {
                    block.event_type = Some(type_name.to_string());
                    block.event_name = Some(event_name.to_string());
                }
            }
        } else if let Some(rest) = content.strip_prefix("@enum") {
            let rest = rest.trim();
            if let Some(name) = rest.split_whitespace().next() {
                block.class = Some(name.to_string());
                block.is_enum = true;
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
            if let Some((idx_str, type_str)) = rest.split_once(char::is_whitespace)
                && let Ok(idx) = idx_str.trim().parse::<usize>() {
                    block.builds_field = Some((idx, parse_type(type_str.trim())));
                }
        } else if let Some(rest) = content.strip_prefix("@built-name") {
            let rest = rest.trim();
            if let Ok(idx) = rest.parse::<usize>()
                && idx >= 1 {
                    block.built_name = Some(idx);
                }
        } else if content.starts_with("@built-extends") {
            block.built_extends = true;
        } else if let Some(rest) = content.strip_prefix("@correlated") {
            let names: Vec<String> = rest.trim().split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if names.len() >= 2 {
                block.correlated_groups.push(names);
            }
        } else if let Some(rest) = content.strip_prefix("@see")
            .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
        {
            let target = rest.trim();
            if !target.is_empty() {
                block.see.push(target.to_string());
            }
        } else if let Some(rest) = content.strip_prefix("@flavor-narrows") {
            let mask = crate::flavor::parse_flavor_annotation(rest.trim());
            if mask != 0 {
                block.flavor_guard |= mask;
            }
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


pub mod annotation_types;
pub mod annotation_scanning;
pub mod scan_globals;
pub mod scan_defclass;
pub mod scan_built_name;

pub(crate) use annotation_types::{
    format_annotation_type, substitute_alias_type_params, match_projection,
    parse_type, parse_return_line,
};
pub use annotation_types::OverloadSig;
pub(crate) use annotation_types::parse_overload;

pub use annotation_scanning::{
    FieldValueKind, ExternalGlobalKind, ExternalGlobal,
    SuppressionKind, DiagnosticSuppression, scan_diagnostic_directives,
};
pub(crate) use annotation_scanning::{
    ADDON_NS_NAME,
    scan_method_typed_self_fields,
};
pub(crate) use annotation_scanning::{
    is_select_varargs,
    reduce_to_fun_alias, resolve_annotation_type,
};
pub use scan_globals::scan_file_globals;
pub(crate) use scan_globals::scan_file_globals_with_synth;
pub use scan_defclass::scan_defclass_calls;
pub use scan_built_name::scan_built_name_calls;
