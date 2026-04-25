use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::ast::{AstNode, Block, Statement, Expression, FunctionCall};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::{ResolvedOverload, ValueType};

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
pub fn annotation_type_is_nullable(ann: &AnnotationType) -> bool {
    match ann {
        AnnotationType::Simple(s) => s == "nil",
        AnnotationType::Union(members) => members.iter().any(annotation_type_is_nullable),
        AnnotationType::NonNil(_) => false,
        AnnotationType::Intersection(_) => false,
        _ => false,
    }
}

/// Check if an annotation type contains a `Backtick(...)` anywhere (including inside unions).
pub fn annotation_contains_backtick(ann: &AnnotationType) -> bool {
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

pub fn resolve_primitive_type_name(name: &str) -> Option<ValueType> {
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
pub struct TypedSelfField {
    pub(crate) class_name: String,
    pub(crate) field_name: String,
    pub(crate) annotation_type: AnnotationType,
    pub(crate) visibility: Visibility,
    pub(crate) byte_range: (u32, u32),
}

/// Expand a `Simple(name)` annotation that refers to a tuple-form alias into
/// the alias body. Also unwraps the `Simple` when it's the only member of a
/// one-element `Union`. Leaves other annotations unchanged.
pub fn expand_tuple_form_alias(
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
pub fn tuple_form_cases(ann: &AnnotationType) -> Vec<(Vec<TuplePosition>, Option<String>)> {
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
            }
        }).collect()
    } else {
        Vec::new()
    };
    (col_vts, col_raws, labels, overloads)
}

/// True if `ann` is a `Tuple` or a `Union` every member of which is a `Tuple`.
/// This is the shape produced by the new tuple-union `@return` syntax.
pub fn annotation_is_tuple_form(ann: &AnnotationType) -> bool {
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

/// Returns `Protected` for names starting with `_`, `Public` otherwise.
/// Used as the default visibility for runtime-discovered fields (e.g. `self._foo = bar`).
/// NOT used for explicit `@field` declarations — those default to `Public` since the
/// author had the opportunity to write `@field protected`.
pub fn default_visibility_for_name(name: &str) -> Visibility {
    if name.starts_with('_') {
        Visibility::Protected
    } else {
        Visibility::Public
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CastMode {
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
pub struct DefclassFieldEntry {
    pub name: String,
    pub children: Vec<DefclassFieldEntry>,
    /// Byte range of the field name token in the source file.
    pub name_start: u32,
    pub name_end: u32,
}

/// Recursively extract named field entries from a table constructor.
pub fn extract_table_literal_fields(tc: &crate::ast::TableConstructor<'_>) -> Vec<DefclassFieldEntry> {
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
    pub has_meta: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationBlock {
    pub params: Vec<ParamInfo>,
    pub returns: Vec<AnnotationType>,
    /// Legacy `@return T name` — name per return line (parallel to `returns`).
    /// Always `None` for tuple-form returns (names live inside `TuplePosition`).
    pub return_names: Vec<Option<String>>,
    /// Legacy `@return T @description` — description per return line.
    /// Always `None` for tuple-form returns (descriptions live inside the `Tuple`).
    pub return_descriptions: Vec<Option<String>>,
    pub var_type: Option<AnnotationType>,
    pub class: Option<String>,
    pub class_type_params: Vec<String>,
    pub class_type_param_constraints: Vec<Option<String>>,
    pub class_parents: Vec<String>,
    pub fields: Vec<(String, AnnotationType, Visibility)>,
    pub alias: Option<(String, AnnotationType)>,
    pub alias_type_params: Vec<String>,
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
    /// `@type-narrows <target_param> <classname_param>` — type guard that narrows target to the class named by classname param
    pub type_narrows: Option<(usize, usize)>,
    /// `@type-narrows ClassName` — method-style type guard that narrows self to ClassName
    pub type_narrows_class: Option<String>,
    /// True when the declaration comes from `@enum` rather than `@class`
    pub is_enum: bool,
    /// `@correlated field1, field2, ...` — fields that are always nil/non-nil together
    pub correlated_groups: Vec<Vec<String>>,
    /// `@see <target>` — cross-reference link(s) to related symbol(s) or URL(s). Doc-only.
    pub see: Vec<String>,
    /// `@flavor retail, wrath` — bitmask of flavors this function guards.
    /// Non-zero marks the annotated function as a flavor guard.
    pub flavor_guard: u8,
}

// ── Comment extraction ───────────────────────────────────────────────────────

/// Extract LuaLS-style annotations from comments preceding a syntax node.
///
/// Walks backward through the token stream from the node's start position,
/// collecting `---@` comment lines. This approach works regardless of which
/// parent node the trivia tokens are attached to (rowan attaches trailing
/// trivia to the preceding construct, so comments before a function can end
/// up inside the preceding statement's expression list).
pub fn extract_annotations(node: SyntaxNode<'_>) -> AnnotationBlock {
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

/// Scan all comments in the syntax tree for @class and @alias declarations.
pub fn scan_all_annotations(root: SyntaxNode<'_>) -> ScanResult {
    let mut classes = Vec::new();
    let mut aliases = Vec::new();
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
                // If this starts a new @class or @alias and the current group already
                // contains one, flush the previous group first so each declaration
                // becomes its own group (block.alias/class is Option and would be overwritten).
                if !current_group.is_empty() {
                    let starts_new_decl = text.contains("@class ") || text.contains("@alias ");
                    let group_has_decl = starts_new_decl && current_group.iter().any(|(l, _, _)| l.contains("@class ") || l.contains("@alias "));
                    if group_has_decl {
                        flush_group(&current_group, current_class_range, current_alias_range, &mut classes, &mut aliases, &mut has_meta);
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
                flush_group(&current_group, current_class_range, current_alias_range, &mut classes, &mut aliases, &mut has_meta);
                current_group.clear();
                current_class_range = None;
                current_alias_range = None;
            }
            prev_was_newline = true;
        } else if kind == SyntaxKind::Whitespace {
        } else {
            flush_group(&current_group, current_class_range, current_alias_range, &mut classes, &mut aliases, &mut has_meta);
            current_group.clear();
            current_class_range = None;
            current_alias_range = None;
            prev_was_newline = false;
        }
    }
    flush_group(&current_group, current_class_range, current_alias_range, &mut classes, &mut aliases, &mut has_meta);

    ScanResult { classes, aliases, has_meta }
}

fn flush_group(
    lines: &[(String, u32, u32)],
    class_range: Option<(u32, u32)>,
    alias_range: Option<(u32, u32)>,
    classes: &mut Vec<ClassDecl>,
    aliases: &mut Vec<AliasDecl>,
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
        classes.push(ClassDecl { name: class_name, type_params: block.class_type_params, type_param_constraints: block.class_type_param_constraints, parents: block.class_parents, fields: block.fields, accessors: block.accessors, overloads, generics: block.generics, constructor_methods: block.constructor_methods, constraint_type_arg_subs: Vec::new(), field_built_names: HashMap::new(), is_enum: block.is_enum, correlated_groups: block.correlated_groups, def_range: class_range, def_path: None, field_ranges, field_paths: HashMap::new(), see: block.see.clone() });
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

/// Strip trailing `@description` text from a `@return` type string.
/// Multiple returns must use separate `@return` lines.
fn strip_return_description(s: &str) -> &str {
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
    let mut after_ampersand = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '{' => { depth += 1; after_colon = false; in_fun_ret = false; after_comma = false; after_pipe = false; after_ampersand = false; }
            '>' | ')' | '}' => {
                depth = depth.saturating_sub(1);
                after_colon = false;
                after_comma = false;
                after_pipe = false;
                after_ampersand = false;
                if depth == 0 && c == ')' {
                    // Look ahead for `:` (possibly after spaces) to detect fun() return types
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                    if j < bytes.len() && bytes[j] == b':' {
                        in_fun_ret = true;
                    }
                }
            }
            '|' if depth == 0 => { after_colon = false; after_comma = false; after_pipe = true; after_ampersand = false; }
            '&' if depth == 0 => { after_colon = false; after_comma = false; after_pipe = false; after_ampersand = true; }
            ',' if depth == 0 && in_fun_ret => { after_comma = true; after_pipe = false; after_ampersand = false; }
            ':' if depth == 0 => { after_colon = true; after_pipe = false; after_ampersand = false; }
            c if c.is_whitespace() && depth == 0 && !after_colon && !after_comma && !after_pipe && !after_ampersand => {
                // Look ahead: if a `|` or `&` follows (with optional spaces), this is a
                // union/intersection type like `"A" | "B"` or `T & U` — continue parsing.
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                if j < bytes.len() && (bytes[j] == b'|' || bytes[j] == b'&') {
                    // skip — this space is part of a union/intersection type expression
                } else {
                    return &s[..i];
                }
            }
            _ => { after_colon = false; after_comma = false; after_pipe = false; after_ampersand = false; }
        }
    }
    s
}

fn split_at_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    // Track function return context so `|` inside `fun(): T1 | T2` is not
    // treated as a top-level union separator (the `|` binds to the return list
    // within the function). Once set, `in_fun_ret` persists across nested
    // parens (e.g. `fun(): (A, B) | (C, D)` — the outer `|` is part of the
    // fun's return union).
    let mut in_fun_ret = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '{' => { depth += 1; }
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

/// Gap 4: match `params<F>` / `returns<F>` utility-type projection shape and
/// extract the projection kind + referenced generic name. Returns `None` for
/// anything else.
pub(crate) fn match_projection(
    at: &AnnotationType,
    generic_names: &[String],
) -> Option<crate::types::ProjectionKind> {
    if let AnnotationType::Parameterized(base, args) = at {
        if args.len() != 1 { return None; }
        let name = match &args[0] {
            AnnotationType::Simple(n) if generic_names.iter().any(|g| g == n) => n.clone(),
            _ => return None,
        };
        return match base.as_str() {
            "params" => Some(crate::types::ProjectionKind::Params(name)),
            "returns" => Some(crate::types::ProjectionKind::Return(name)),
            _ => None,
        };
    }
    None
}

pub(crate) fn format_annotation_type(at: &AnnotationType) -> String {
    match at {
        AnnotationType::Simple(s) => s.clone(),
        AnnotationType::Array(inner) => format!("{}[]", format_annotation_type(inner)),
        AnnotationType::Union(types) => types.iter()
            .map(format_annotation_type)
            .collect::<Vec<_>>()
            .join(" | "),
        AnnotationType::Parameterized(name, params) => {
            let params_str = params.iter()
                .map(format_annotation_type)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}<{}>", name, params_str)
        }
        AnnotationType::Backtick(inner) => format_annotation_type(inner),
        AnnotationType::NonNil(inner) => format!("{}!", format_annotation_type(inner)),
        AnnotationType::Intersection(types) => types.iter()
            .map(format_annotation_type)
            .collect::<Vec<_>>()
            .join(" & "),
        AnnotationType::Fun(params, returns, is_vararg) => {
            let mut args: Vec<String> = params.iter().map(|p| {
                let suffix = if p.optional { "?" } else { "" };
                format!("{}{}: {}", p.name, suffix, format_annotation_type(&p.typ))
            }).collect();
            if *is_vararg { args.push("...".to_string()); }
            let ret_str = if returns.is_empty() {
                String::new()
            } else {
                format!(": {}", returns.iter().map(format_annotation_type).collect::<Vec<_>>().join(", "))
            };
            format!("fun({}){}", args.join(", "), ret_str)
        }
        AnnotationType::TableLiteral(fields) => {
            let parts: Vec<String> = fields.iter().map(|(name, typ)| {
                format!("{}: {}", name, format_annotation_type(typ))
            }).collect();
            format!("{{{}}}", parts.join(", "))
        }
        AnnotationType::VarArgs(inner) => {
            format!("...{}", format_annotation_type(inner))
        }
        AnnotationType::Tuple(positions, description) => {
            let parts: Vec<String> = positions.iter().map(|p| {
                match &p.name {
                    Some(n) => format!("{} {}", format_annotation_type(&p.typ), n),
                    None => format_annotation_type(&p.typ),
                }
            }).collect();
            match description {
                Some(d) => format!("({}) {}", parts.join(", "), d),
                None => format!("({})", parts.join(", ")),
            }
        }
    }
}

/// Substitute type parameter names in a parameterized alias body with concrete annotation types.
/// E.g. for alias body `V[]` with type_params=["K","V"] and args=[Simple("string"), Simple("number")],
/// replaces V → Simple("number") to produce Array(Simple("number")).
pub(crate) fn substitute_alias_type_params(
    body: &AnnotationType,
    type_params: &[String],
    args: &[AnnotationType],
) -> AnnotationType {
    match body {
        AnnotationType::Simple(name) => {
            if let Some(pos) = type_params.iter().position(|p| p == name) {
                args[pos].clone()
            } else {
                body.clone()
            }
        }
        AnnotationType::Union(parts) => {
            AnnotationType::Union(parts.iter().map(|p| substitute_alias_type_params(p, type_params, args)).collect())
        }
        AnnotationType::Array(inner) => {
            AnnotationType::Array(Box::new(substitute_alias_type_params(inner, type_params, args)))
        }
        AnnotationType::Parameterized(base, pargs) => {
            AnnotationType::Parameterized(
                base.clone(),
                pargs.iter().map(|a| substitute_alias_type_params(a, type_params, args)).collect(),
            )
        }
        AnnotationType::Fun(params, returns, is_vararg) => {
            let new_params = params.iter().map(|p| ParamInfo {
                name: p.name.clone(),
                typ: substitute_alias_type_params(&p.typ, type_params, args),
                optional: p.optional,
                description: p.description.clone(),
            }).collect();
            let new_returns = returns.iter().map(|r| substitute_alias_type_params(r, type_params, args)).collect();
            AnnotationType::Fun(new_params, new_returns, *is_vararg)
        }
        AnnotationType::NonNil(inner) => {
            AnnotationType::NonNil(Box::new(substitute_alias_type_params(inner, type_params, args)))
        }
        AnnotationType::Intersection(parts) => {
            AnnotationType::Intersection(parts.iter().map(|p| substitute_alias_type_params(p, type_params, args)).collect())
        }
        AnnotationType::TableLiteral(fields) => {
            AnnotationType::TableLiteral(fields.iter().map(|(n, t)| {
                (n.clone(), substitute_alias_type_params(t, type_params, args))
            }).collect())
        }
        AnnotationType::Backtick(inner) => {
            AnnotationType::Backtick(Box::new(substitute_alias_type_params(inner, type_params, args)))
        }
        AnnotationType::VarArgs(inner) => {
            AnnotationType::VarArgs(Box::new(substitute_alias_type_params(inner, type_params, args)))
        }
        AnnotationType::Tuple(positions, description) => {
            AnnotationType::Tuple(
                positions.iter().map(|p| TuplePosition {
                    typ: substitute_alias_type_params(&p.typ, type_params, args),
                    name: p.name.clone(),
                }).collect(),
                description.clone(),
            )
        }
    }
}

pub(crate) fn parse_type(s: &str) -> AnnotationType {
    let s = s.trim();
    if s.is_empty() { return AnnotationType::Simple(s.to_string()); }
    if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') {
        return AnnotationType::Backtick(Box::new(parse_type(&s[1..s.len()-1])));
    }
    if let Some(without_bang) = s.strip_suffix('!') {
        let mut depth = 0usize;
        let is_fun_type = without_bang.starts_with("fun(") || without_bang.starts_with("async fun(");
        let mut found_return_colon = false;
        for c in without_bang.chars() {
            match c {
                '<' | '(' => depth += 1,
                '>' | ')' => depth = depth.saturating_sub(1),
                ':' if depth == 0 && is_fun_type => found_return_colon = true,
                _ => {}
            }
        }
        if depth == 0 && !found_return_colon {
            let base_type = parse_type(without_bang);
            return AnnotationType::NonNil(Box::new(base_type));
        }
    }
    if let Some(without_q) = s.strip_suffix('?') {
        let mut depth = 0usize;
        let is_fun_type = without_q.starts_with("fun(") || without_q.starts_with("async fun(");
        let mut found_return_colon = false;
        for c in without_q.chars() {
            match c {
                '<' | '(' => depth += 1,
                '>' | ')' => depth = depth.saturating_sub(1),
                // For function types, a `:` at depth 0 marks the return type separator.
                // The trailing `?` belongs to the return type, not the function itself.
                ':' if depth == 0 && is_fun_type => found_return_colon = true,
                _ => {}
            }
        }
        if depth == 0 && !found_return_colon {
            let base_type = parse_type(without_q);
            return AnnotationType::Union(vec![base_type, AnnotationType::Simple("nil".to_string())]);
        }
    }
    // ...T — variadic type (used in @return ...any, etc.)
    // Bare `...` (no following type) is treated as `...any`.
    if let Some(inner) = s.strip_prefix("...") {
        let inner_type = if inner.is_empty() {
            AnnotationType::Simple("any".to_string())
        } else {
            parse_type(inner)
        };
        return AnnotationType::VarArgs(Box::new(inner_type));
    }
    let union_parts = split_at_top_level(s, '|');
    if union_parts.len() > 1 {
        let parts: Vec<AnnotationType> = union_parts.iter().map(|p| parse_type(p.trim())).collect();
        return AnnotationType::Union(parts);
    }
    let intersection_parts = split_at_top_level(s, '&');
    if intersection_parts.len() > 1 {
        let parts: Vec<AnnotationType> = intersection_parts.iter().map(|p| parse_type(p.trim())).collect();
        return AnnotationType::Intersection(parts);
    }
    // Parenthesized types: (string|number), (fun(): T), or tuple (A, B name).
    // A tuple must have a top-level comma inside `(...)`; `(T)` is plain grouping.
    // Per-case descriptions are only captured at line level (see parse_return_line).
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
            let inner = &s[1..s.len() - 1];
            let parts = split_at_top_level(inner, ',');
            if parts.len() > 1 {
                return AnnotationType::Tuple(parse_tuple_positions(&parts), None);
            }
            return parse_type(inner);
        }
    }
    // Strip `async` prefix — e.g. stubs use `async fun(...):...`; treat as plain fun
    let fun_str = s.strip_prefix("async ").unwrap_or(s);
    if fun_str.starts_with("fun(")
        && let Some(sig) = parse_overload(fun_str) {
            return AnnotationType::Fun(sig.params, sig.returns, sig.is_vararg);
        }
    if let Some(without_brackets) = s.strip_suffix("[]") {
        let base = parse_type(without_brackets);
        return AnnotationType::Array(Box::new(base));
    }
    if s.ends_with('>')
        && let Some(lt_pos) = s.find('<') {
            let base = s[..lt_pos].trim();
            let args_str = &s[lt_pos+1..s.len()-1];
            let args = split_at_top_level(args_str, ',');
            let arg_types: Vec<AnnotationType> = args.iter().map(|a| parse_type(a.trim())).collect();
            return AnnotationType::Parameterized(base.to_string(), arg_types);
        }
    // Inline table types: {key: type, ...} → anonymous table shape
    if s.starts_with('{') && s.ends_with('}') {
        let inner = s[1..s.len()-1].trim();
        if inner.is_empty() {
            return AnnotationType::Simple("table".to_string());
        }
        let field_parts = split_at_top_level(inner, ',');
        let mut fields = Vec::new();
        for part in &field_parts {
            let part = part.trim();
            if part.is_empty() { continue; }
            // field: type  or  field?: type
            if let Some(colon_pos) = part.find(':') {
                let name = part[..colon_pos].trim();
                let type_str = part[colon_pos+1..].trim();
                let (name, optional) = if let Some(stripped) = name.strip_suffix('?') {
                    (stripped, true)
                } else {
                    (name, false)
                };
                if !name.is_empty() && !type_str.is_empty() {
                    let mut field_type = parse_type(type_str);
                    if optional {
                        field_type = AnnotationType::Union(vec![field_type, AnnotationType::Simple("nil".to_string())]);
                    }
                    fields.push((name.to_string(), field_type));
                }
            }
        }
        if fields.is_empty() {
            return AnnotationType::Simple("table".to_string());
        }
        return AnnotationType::TableLiteral(fields);
    }
    // Variadic type syntax: ...any, ...string, ...T → strip prefix, parse inner type
    if let Some(inner) = s.strip_prefix("...")
        && !inner.is_empty() {
            return parse_type(inner);
        }
    AnnotationType::Simple(s.to_string())
}

/// Parsed overload signature from `---@overload fun(...): ret`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OverloadSig {
    pub params: Vec<ParamInfo>,
    pub returns: Vec<AnnotationType>,
    pub is_vararg: bool,
    pub is_return_only: bool,
}

/// Parse an overload string like `fun(param: type, ...): retType`.
/// The legacy `@overload return:` form has been removed — use a tuple-union
/// `@return (A, B) | (C, D)` annotation instead.
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

/// Parse the comma-separated body of a tuple annotation: each part is `type [name]`.
/// The name is captured from any trailing identifier after the type expression.
fn parse_tuple_positions(parts: &[&str]) -> Vec<TuplePosition> {
    parts.iter().filter_map(|part| {
        let part = part.trim();
        if part.is_empty() { return None; }
        let type_text = extract_type_prefix(part);
        let name = extract_trailing_ident(part[type_text.len()..].trim());
        Some(TuplePosition { typ: parse_type(type_text), name })
    }).collect()
}

/// Parse a `@return` or `---|` continuation line body. Returns
/// `(type, legacy_name, description)`:
/// - `type` is `Tuple(..)` for new-style tuple form (with names carried inline
///   on each `TuplePosition`), or a legacy single-type parse otherwise.
/// - `legacy_name` is the trailing identifier from `@return T name description`
///   — always `None` for tuple form (names live on `TuplePosition`).
/// - `description` is the trailing `@desc` (legacy) or the text after `)`
///   (tuple form).
///
/// `force_tuple` — when true, a single-position `(T) [desc]` also parses as
/// `Tuple([T])`. Used by `---|` continuation lines where the surrounding
/// `@return` has already committed to tuple-union form (so `(nil)` is a
/// 1-position case, not a grouping). Base `@return` lines use
/// `force_tuple=false` and fall back to tuple only when the trailing text is
/// empty, preserving the legacy `@return (string|number) name` form.
pub(crate) fn parse_return_line(s: &str, force_tuple: bool) -> (AnnotationType, Option<String>, Option<String>) {
    let s = s.trim();
    // New-style tuple: starts with `(` and has a matched closing `)` somewhere;
    // any content after the `)` is the case description. Multiple tuples may
    // be chained on one line with `|` (e.g. `(A, B) | (C, D)`) — in that case
    // we return a `Union` of `Tuple`s, parallel to the `---|` continuation form.
    if s.starts_with('(') {
        let mut cases: Vec<(Vec<TuplePosition>, Option<String>)> = Vec::new();
        let mut first_trailing: Option<&str> = None;
        let mut rem = s;
        loop {
            if !rem.starts_with('(') { break; }
            let mut depth = 0i32;
            let mut close_idx = None;
            for (i, c) in rem.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => { depth -= 1; if depth == 0 { close_idx = Some(i); break; } }
                    _ => {}
                }
            }
            let Some(end) = close_idx else { break; };
            let inner = &rem[1..end];
            let after = rem[end + 1..].trim_start();
            let parts = split_at_top_level(inner, ',');
            let positions = parse_tuple_positions(&parts);
            // Split `after` at the next `|(` (which starts the next tuple case):
            // text before is this case's trailing description, remainder after
            // `|` continues the chain.
            let (case_trailing, next_rem) = {
                let mut split = None;
                let bytes = after.as_bytes();
                for (i, &b) in bytes.iter().enumerate() {
                    if b == b'|' {
                        let rest = after[i + 1..].trim_start();
                        if rest.starts_with('(') { split = Some((i, rest)); break; }
                    }
                }
                match split {
                    Some((i, next)) => (after[..i].trim(), Some(next)),
                    None => (after, None),
                }
            };
            if cases.is_empty() { first_trailing = Some(after); }
            let desc = {
                let t = case_trailing.strip_prefix('@').unwrap_or(case_trailing).trim();
                if t.is_empty() { None } else { Some(t.to_string()) }
            };
            cases.push((positions, desc));
            match next_rem {
                Some(next) => rem = next,
                None => break,
            }
        }
        // Multi-case: always a tuple-union (no single-element ambiguity).
        if cases.len() >= 2 {
            let members: Vec<AnnotationType> = cases.into_iter()
                .map(|(p, d)| AnnotationType::Tuple(p, d))
                .collect();
            return (AnnotationType::Union(members), None, None);
        }
        // Single case: preserve existing single-tuple rules so legacy
        // `@return (string|number) name` still parses as a grouped single type.
        if let Some((positions, desc)) = cases.into_iter().next() {
            let trailing = first_trailing.unwrap_or("").trim();
            let is_tuple = positions.len() > 1
                || (!positions.is_empty() && (force_tuple || trailing.is_empty()));
            if is_tuple {
                return (AnnotationType::Tuple(positions, desc), None, None);
            }
        }
    }
    // Legacy: `type [name] [@description]` — single pass to split body from desc,
    // then extract name from what remains after the type prefix.
    let (body, description) = split_legacy_desc(s);
    let type_only = extract_type_prefix(body);
    let name = extract_trailing_ident(body[type_only.len()..].trim());
    (parse_type(type_only), name, description)
}

/// Split a legacy `@return` body into `(body_without_desc, Some(desc))` at the
/// first ` @` at paren depth 0, or `(body, None)` if no description is present.
fn split_legacy_desc(s: &str) -> (&str, Option<String>) {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut at_pos: Option<usize> = None;
    for i in 0..bytes.len() {
        match bytes[i] {
            b'<' | b'(' => depth += 1,
            b'>' | b')' => depth = depth.saturating_sub(1),
            b'@' if depth == 0 && i > 0 && bytes[i - 1] == b' ' => {
                at_pos = Some(i);
                break;
            }
            _ => {}
        }
    }
    match at_pos {
        Some(p) => {
            let body = s[..p].trim_end();
            let desc = s[p + 1..].trim();
            let desc = if desc.is_empty() { None } else { Some(desc.to_string()) };
            (body, desc)
        }
        None => (s, None),
    }
}

/// Extract the first whitespace-delimited token as an identifier name, or `None`
/// if the token isn't a valid identifier or the input is empty.
fn extract_trailing_ident(s: &str) -> Option<String> {
    let first = s.split_whitespace().next().unwrap_or("");
    if first.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && first.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        Some(first.to_string())
    } else {
        None
    }
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

/// Build a dotted path string for a method/function global.
/// Returns the fully qualified dotted name: `root.int1.int2.method` for methods
/// or just `root` for top-level functions. Returns None for non-method/function
/// variants (TableField, Variable, etc.).
pub fn func_path(g: &ExternalGlobal) -> Option<String> {
    match &g.kind {
        ExternalGlobalKind::Function => Some(g.name.clone()),
        ExternalGlobalKind::Method(path, method_name, _) => {
            let mut s = g.name.clone();
            for seg in path { s.push('.'); s.push_str(seg); }
            s.push('.'); s.push_str(method_name);
            Some(s)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FieldValueKind { String, Number, Boolean, Nil, Table, Function, FunctionCall(Vec<String>, Option<std::string::String>), FieldRef(Vec<String>), Unknown }

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExternalGlobalKind {
    Function,
    /// Method on a path: (intermediate_path, method_name, is_colon).
    ///
    /// The meaning of `intermediate_path` depends on the root `name`:
    ///   - **Addon-ns root** (`name == ADDON_NS_NAME`): intermediates are a
    ///     sub-table chain. `pre_globals` walks them (auto-creating missing
    ///     tables) and lands the method on the innermost sub-table.
    ///   - **Any other root** (a class or non-class table): intermediates are
    ///     accessor names used purely for visibility lookup in the class's
    ///     `accessors` map. The method lives on the root table itself, not on
    ///     a sub-table for each accessor.
    ///
    /// Examples:
    ///   - `function Class:Method()`     → Method([], "Method", true), name="Class"
    ///   - `function Class.__p:Method()` → Method(["__p"], "Method", true), name="Class"
    ///     (accessor: `__p`'s visibility applied to `Method` on `Class`)
    ///   - `function ns:Init()`          → Method([], "Init", true),   name=ADDON_NS_NAME
    ///   - `function ns.A.B.C:Method()`  → Method(["A","B","C"], "Method", true), name=ADDON_NS_NAME
    ///     (sub-table chain: `Method` lands on `ns.A.B.C`)
    Method(Vec<String>, String, bool),
    Table,
    /// Field on a path: (intermediate_path, field_name, value_kind).
    ///
    /// Chains of 3+ parts are only emitted for addon-ns roots — non-addon
    /// deep writes like `FrameClass.Inner.x = 1` are silently ignored by the
    /// scanner to avoid fabricating sub-tables on unrelated external classes.
    ///
    /// Examples:
    ///   - `Class.x = val`    → TableField([], "x", kind),      name="Class"
    ///   - `ns.x = val`       → TableField([], "x", kind),      name=ADDON_NS_NAME
    ///   - `ns.A.x = val`     → TableField(["A"], "x", kind),   name=ADDON_NS_NAME
    ///   - `ns.A.B.x = val`   → TableField(["A","B"], "x", kind), name=ADDON_NS_NAME
    TableField(Vec<String>, String, FieldValueKind),
    Variable(FieldValueKind),
    /// Reference to a field on another table (e.g. `strmatch = str.match` where `str` = `string`)
    FieldRef(String, String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// `@builds-field` annotation: (param_index_1based, field_type)
    pub builds_field: Option<(usize, AnnotationType)>,
    /// `@built-name` annotation: param_index (1-based) whose string literal names the built type
    pub built_name: Option<usize>,
    /// `@built-extends` annotation: new built type inherits from receiver's current built type
    pub built_extends: bool,
    /// `@type-narrows` annotation: (target_param, classname_param) — type guard function
    pub type_narrows: Option<(usize, usize)>,
    /// `@type-narrows ClassName` — method-style type guard narrowing self to ClassName
    pub type_narrows_class: Option<String>,
    /// For string literal assignments, the raw string value (e.g. `"hello"`)
    pub string_value: Option<String>,
    /// For number literal assignments, the raw number value (e.g. `"42"`)
    pub number_value: Option<String>,
    /// Global from `stubs/overrides/` — takes priority over vendor definitions
    pub is_override: bool,
    /// `@see <target>` — cross-reference link(s) to related symbols or URLs. Doc-only.
    #[serde(default)]
    pub see: Vec<String>,
    /// WoW flavor availability bitmask (from `@flavor` or stub gen data).
    /// A value of 0 means "no data" and is treated as available everywhere.
    #[serde(default)]
    pub flavors: u8,
    /// When non-zero, calling this function acts as a flavor guard — the
    /// then-branch narrows the active flavor set to this mask.
    #[serde(default)]
    pub flavor_guard: u8,
}

/// Check if an expression is `select(N, ...)` and return N.
pub(crate) fn is_select_varargs(expr: &Expression<'_>) -> Option<usize> {
    if let Expression::FunctionCall(call) = expr {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() == 1 && names[0] == "select" {
            let args = call.arguments()?.expressions();
            if args.len() == 2
                && let (Expression::Literal(lit), Expression::VarArgs(_)) = (&args[0], &args[1]) {
                    let n_str = lit.get_number()?;
                    return n_str.parse::<usize>().ok();
                }
        }
    }
    None
}

/// Coarse synthesized return-position type. Mirrors
/// `Analysis::synthesized_return_type` in `build_ir.rs`: literals normalize to
/// their generic type (no literal unions), nil stays nil, everything else
/// becomes `any`.
fn synth_coarse_return_type(expr: &Expression<'_>) -> AnnotationType {
    if let Expression::Literal(lit) = expr {
        if lit.is_nil() { return AnnotationType::Simple("nil".to_string()); }
        if lit.get_string().is_some() { return AnnotationType::Simple("string".to_string()); }
        if lit.get_number().is_some() { return AnnotationType::Simple("number".to_string()); }
        if lit.get_bool().is_some() { return AnnotationType::Simple("boolean".to_string()); }
    }
    AnnotationType::Simple("any".to_string())
}

/// AST-only mirror of `Analysis::block_always_exits` in `checks.rs`. Used by
/// workspace-scan synthesis to decide whether the body falls through (implying
/// an implicit all-nil return case).
fn synth_block_always_exits(block: &Block<'_>) -> bool {
    let mut ends_with_break = false;
    for child in block.syntax().children_with_tokens() {
        match &child {
            NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BreakKeyword => {
                ends_with_break = true;
            }
            NodeOrToken::Token(tok) if matches!(
                tok.kind(),
                SyntaxKind::Whitespace | SyntaxKind::Newline | SyntaxKind::Comment
            ) => {}
            _ => {
                ends_with_break = false;
            }
        }
    }
    if ends_with_break {
        return true;
    }
    let statements = block.statements();
    let Some(last) = statements.last() else { return false };
    match last {
        Statement::Return(_) => true,
        Statement::FunctionCall(call) => {
            if let Some(ident) = call.identifier() {
                let names = ident.names();
                names.len() == 1 && names[0] == "error"
            } else {
                false
            }
        }
        // `while true do ... end` / `repeat ... until false` with no escaping
        // break never falls through. Mirrors `is_infinite_loop_stmt` in
        // checks.rs — without this, a workspace-scanned function ending in an
        // infinite loop would spuriously gain an implicit-nil tuple that the
        // per-file IR synthesizer wouldn't.
        Statement::While(_) | Statement::Repeat(_) => synth_is_infinite_loop_stmt(last),
        Statement::If(if_chain) => {
            let branches = if_chain.if_branches();
            let else_branch = if_chain.else_branch();
            if else_branch.is_none() {
                return false;
            }
            for branch in &branches {
                if let Some(block) = branch.block() {
                    if !synth_block_always_exits(&block) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            if let Some(eb) = &else_branch {
                if let Some(block) = eb.block() {
                    synth_block_always_exits(&block)
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

/// AST-only mirror of `Analysis::is_infinite_loop_stmt` in `checks.rs`.
fn synth_is_infinite_loop_stmt(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::While(wl) => {
            let Some(cond) = wl.condition() else { return false };
            if !synth_expression_is_literal_bool(&cond, true) { return false; }
            let Some(block) = wl.block() else { return false };
            !synth_node_has_escaping_break(block.syntax())
        }
        Statement::Repeat(rl) => {
            let Some(cond) = rl.condition() else { return false };
            if !synth_expression_is_literal_bool(&cond, false) { return false; }
            let Some(block) = rl.block() else { return false };
            !synth_node_has_escaping_break(block.syntax())
        }
        _ => false,
    }
}

fn synth_expression_is_literal_bool(expr: &Expression<'_>, value: bool) -> bool {
    match expr {
        Expression::Literal(lit) => lit.get_bool() == Some(value),
        Expression::GroupedExpression(g) => g
            .get_expression()
            .as_ref()
            .is_some_and(|inner| synth_expression_is_literal_bool(inner, value)),
        _ => false,
    }
}

/// Look for a `break` whose target is `node`'s containing loop. Stops at
/// nested loops and function bodies (their breaks belong to the inner loop /
/// don't escape this loop).
fn synth_node_has_escaping_break(node: SyntaxNode<'_>) -> bool {
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => {
                if tok.kind() == SyntaxKind::BreakKeyword {
                    return true;
                }
            }
            NodeOrToken::Node(sub) => match sub.kind() {
                SyntaxKind::WhileLoop
                | SyntaxKind::RepeatUntilLoop
                | SyntaxKind::ForCountLoop
                | SyntaxKind::ForInLoop
                | SyntaxKind::FunctionDefinition => {}
                _ => {
                    if synth_node_has_escaping_break(sub) {
                        return true;
                    }
                }
            },
        }
    }
    false
}

/// Walk a function body and collect per-return entries `(Vec<return_expr_types>, is_bare)`.
/// Descends into control-flow blocks (if/else/do/while/for/repeat) but NOT into
/// nested function definitions. Mirrors the per-return collection that
/// `synthesize_correlated_return_overloads` performs against `func.rets`.
fn synth_collect_returns(block: &Block<'_>, out: &mut Vec<(Vec<AnnotationType>, bool)>) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Return(ret) => {
                let exprs: Vec<AnnotationType> = ret.expression_list()
                    .map(|el| el.expressions().iter().map(synth_coarse_return_type).collect())
                    .unwrap_or_default();
                let is_bare = exprs.is_empty();
                out.push((exprs, is_bare));
            }
            Statement::If(chain) => {
                for branch in chain.if_branches() {
                    if let Some(inner) = branch.block() { synth_collect_returns(&inner, out); }
                }
                if let Some(eb) = chain.else_branch()
                    && let Some(inner) = eb.block() { synth_collect_returns(&inner, out); }
            }
            Statement::Do(g) => {
                if let Some(inner) = g.block() { synth_collect_returns(&inner, out); }
            }
            Statement::While(w) => {
                if let Some(inner) = w.block() { synth_collect_returns(&inner, out); }
            }
            Statement::Repeat(r) => {
                if let Some(inner) = r.block() { synth_collect_returns(&inner, out); }
            }
            Statement::ForCountLoop(f) => {
                if let Some(inner) = f.block() { synth_collect_returns(&inner, out); }
            }
            Statement::ForInLoop(f) => {
                if let Some(inner) = f.block() { synth_collect_returns(&inner, out); }
            }
            // Do not descend into nested functions or non-control statements —
            // their returns belong to a different function.
            _ => {}
        }
    }
}

/// AST-only mirror of `Analysis::synthesize_correlated_return_overloads` in
/// `build_ir.rs`. Walks the given function body and returns synthesized
/// return-only overloads when the body matches the all-set-or-all-nil pattern.
///
/// This runs during workspace scanning (before IR construction) so that
/// cross-file method calls — which resolve through `PreResolvedGlobals` to an
/// external `Function` — pick up the synthesized overloads alongside the
/// per-file IR-level synthesis. Without this, a call like `self:_Helper()` in
/// a `DefineClassType`-registered class resolves to the external function,
/// which has no overloads and therefore no sibling narrowing.
///
/// Precondition: caller has already verified `annotations.returns.is_empty()`
/// (no `@return` annotations) and that no existing overloads have
/// `is_return_only` set.
pub(crate) fn synthesize_return_only_overloads_for_body(body: &Block<'_>) -> Vec<OverloadSig> {
    let mut returns: Vec<(Vec<AnnotationType>, bool)> = Vec::new();
    synth_collect_returns(body, &mut returns);

    // Split explicit multi-value returns from bare / empty returns.
    let implicit_nil = returns.iter().any(|(_, is_bare)| *is_bare)
        || !synth_block_always_exits(body);

    // `is_bare` <=> `exprs.is_empty()` (set together in `synth_collect_returns`),
    // so dropping bare returns alone fully partitions the list.
    let explicit: Vec<Vec<AnnotationType>> = returns.into_iter()
        .filter(|(_, is_bare)| !*is_bare)
        .map(|(exprs, _)| exprs)
        .collect();

    // Match build_ir: need ≥2 signatures total (counting implicit_nil as one).
    if explicit.len() + if implicit_nil { 1 } else { 0 } < 2 { return Vec::new(); }

    // Arity must match across all explicit returns, and be ≥ 2.
    let mut arity: Option<usize> = None;
    for tuple in &explicit {
        match arity {
            None => arity = Some(tuple.len()),
            Some(a) if a == tuple.len() => {}
            _ => return Vec::new(),
        }
    }
    let arity = arity.unwrap_or(0);
    if arity < 2 { return Vec::new(); }

    let mut tuples: Vec<Vec<AnnotationType>> = explicit;
    if implicit_nil {
        tuples.push(vec![AnnotationType::Simple("nil".to_string()); arity]);
    }

    // Dedupe by tuple; require ≥ 2 distinct signatures.
    let mut emitted: Vec<Vec<AnnotationType>> = Vec::new();
    for returns in tuples {
        if emitted.iter().any(|e| e == &returns) { continue; }
        emitted.push(returns);
    }
    if emitted.len() < 2 { return Vec::new(); }

    emitted.into_iter().map(|returns| OverloadSig {
        params: Vec::new(),
        returns,
        is_vararg: false,
        is_return_only: true,
    }).collect()
}

pub fn scan_file_globals(root: SyntaxNode<'_>, source_path: Option<&Path>) -> Vec<ExternalGlobal> {
    scan_file_globals_with_synth(root, source_path, true)
}

/// Variant of [`scan_file_globals`] that lets the caller disable workspace-level
/// synthesis of correlated return-only overloads for a specific file. The LSP /
/// CLI paths consult `inference.correlated_return_overloads` per-file; stub
/// generation leaves it on.
pub fn scan_file_globals_with_synth(
    root: SyntaxNode<'_>,
    source_path: Option<&Path>,
    correlated_return_overloads: bool,
) -> Vec<ExternalGlobal> {
    let owned_path = source_path.map(|p| p.to_path_buf());
    let Some(block) = Block::cast(root) else { return Vec::new(); };

    let mut addon_ns_var: Option<String> = None;
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                if names.len() >= 2 && exprs.len() == 1 && matches!(exprs[0], Expression::VarArgs(_)) {
                    addon_ns_var = Some(names[1].clone());
                    break;
                }
                // local ns = select(2, ...)
                if !names.is_empty() && exprs.len() == 1
                    && let Some(n) = is_select_varargs(&exprs[0])
                        && n == 2 {
                            addon_ns_var = Some(names[0].clone());
                            break;
                        }
            }
    }

    // Track local aliases to known tables (e.g. `local str = string`, `local tab = table`)
    let mut local_aliases: HashMap<String, String> = HashMap::new();
    // Track local variables assigned table constructors (e.g. `local Locale = {}`)
    let mut local_tables: HashSet<String> = HashSet::new();
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
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
                    if let NodeOrToken::Token(t) = token {
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
            if let Some(class_name) = class_name
                && let Some(name_list) = assign.name_list() {
                    let names = name_list.names();
                    if names.len() == 1 {
                        class_vars.insert(names[0].clone(), class_name);
                    }
                }
        }
    }

    // Also populate class_vars from defclass-style calls:
    // `local X = Y:Init("ClassName")` or chained `local X = Y:From("Z"):Include("ClassName")`
    // Walk the call chain to find the innermost call with a string literal first argument.
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                if names.len() == 1 && exprs.len() == 1 && !class_vars.contains_key(&names[0])
                    && let Expression::FunctionCall(call) = &exprs[0]
                        && let Some(class_name) = extract_string_arg_from_call_chain(call) {
                            class_vars.insert(names[0].clone(), class_name);
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
                // Parser2 emits simple function names as bare Name tokens (no Identifier node).
                // Fall back to func.name() when identifier() returns None.
                let (names, is_colon_opt) = if let Some(ident) = func.identifier() {
                    (ident.names(), Some(ident.is_call_to_self()))
                } else if let Some(name) = func.name() {
                    (vec![name], Some(false))
                } else {
                    continue;
                };
                let is_colon = is_colon_opt.unwrap_or(false);
                {
                    let annotations = extract_annotations(func.syntax());
                    let mut overloads: Vec<OverloadSig> = annotations.overloads.iter()
                        .filter_map(|s| parse_overload(s)).collect();
                    // Synthesize correlated return-only overloads from body when
                    // no `@return` annotations exist and no existing overload is
                    // already return-only. Matches the per-file IR synthesis so
                    // cross-file call sites (method calls resolving through a
                    // workspace-scanned class) also see the synthesized overloads.
                    if correlated_return_overloads
                        && annotations.returns.is_empty()
                        && !overloads.iter().any(|o| o.is_return_only)
                        && let Some(body) = func.block() {
                            overloads.extend(synthesize_return_only_overloads_for_body(&body));
                        }
                    let range = func.syntax().text_range();
                    let def_start = u32::from(range.start());
                    let def_end = u32::from(range.end());
                    // Merge @param annotations with actual parameter names.
                    // When some params have annotations and others don't, the
                    // actual param list is the source of truth for param count;
                    // annotations just add type info.
                    let params = if let Some(param_list) = func.params() {
                        let actual_params: Vec<String> = param_list.parameters().into_iter()
                            .filter(|n| !is_colon || n != "self")
                            .collect();
                        let mut ps: Vec<ParamInfo> = actual_params.iter()
                            .map(|n| {
                                // Use annotation if available for this param name
                                if let Some(ann) = annotations.params.iter().find(|p| &p.name == n) {
                                    ann.clone()
                                } else {
                                    ParamInfo { name: n.clone(), typ: AnnotationType::Simple(String::new()), optional: false, description: None }
                                }
                            })
                            .collect();
                        if param_list.ellipsis() {
                            if let Some(ann) = annotations.params.iter().find(|p| p.name == "...") {
                                ps.push(ann.clone());
                            } else {
                                ps.push(ParamInfo { name: "...".to_string(), typ: AnnotationType::Simple(String::new()), optional: false, description: None });
                            }
                        }
                        ps
                    } else if !annotations.params.is_empty() {
                        annotations.params
                    } else { Vec::new() };
                    let see = annotations.see.clone();
                    if names.len() == 1 {
                        globals.push(ExternalGlobal {
                            name: names[0].clone(), kind: ExternalGlobalKind::Function,
                            params, returns: annotations.returns, overloads,
                            doc: annotations.doc, deprecated: annotations.deprecated,
                            nodiscard: annotations.nodiscard, constructor: annotations.constructor,
                            visibility: annotations.visibility,
                            generics: annotations.generics, defclass: annotations.defclass, defclass_parent: annotations.defclass_parent,
                            source_path: owned_path.clone(),
                            def_start, def_end,
                            builds_field: annotations.builds_field.clone(),
                            built_name: annotations.built_name,
                            built_extends: annotations.built_extends,
                            type_narrows: annotations.type_narrows,
                            type_narrows_class: annotations.type_narrows_class.clone(),
                            string_value: None, number_value: None,
                            is_override: false,
                            see: see.clone(),
                            flavors: 0,
                            flavor_guard: annotations.flavor_guard,
                        });
                    } else if names.len() >= 2 {
                        let root_name = &names[0];
                        let method_name = &names[names.len() - 1];
                        let intermediates: Vec<String> = names[1..names.len()-1].to_vec();
                        // Buffer methods defined on local tables (any depth) for later
                        // flushing onto the addon namespace. At flush time the local name
                        // is rewritten to the addon field alias and prepended to the
                        // buffered intermediates, so `function Db.A:Foo()` + `ns.Db = Db`
                        // resolves as `ns.Db.A:Foo()`.
                        if local_tables.contains(root_name) && !class_vars.contains_key(root_name) && addon_ns_var.as_deref() != Some(root_name.as_str()) {
                            local_table_methods.entry(root_name.clone()).or_default().push(ExternalGlobal {
                                name: String::new(), // placeholder, set when flushed
                                kind: ExternalGlobalKind::Method(intermediates.clone(), method_name.clone(), is_colon),
                                params, returns: annotations.returns, overloads,
                                doc: annotations.doc, deprecated: annotations.deprecated,
                                nodiscard: annotations.nodiscard, constructor: annotations.constructor,
                                visibility: annotations.visibility,
                                generics: annotations.generics, defclass: annotations.defclass, defclass_parent: annotations.defclass_parent,
                                source_path: owned_path.clone(),
                                def_start, def_end,
                                builds_field: annotations.builds_field.clone(),
                                built_name: annotations.built_name,
                                built_extends: annotations.built_extends,
                                type_narrows: annotations.type_narrows,
                                type_narrows_class: annotations.type_narrows_class.clone(),
                                string_value: None, number_value: None,
                                is_override: false,
                                see: see.clone(),
                                flavors: 0,
                                flavor_guard: annotations.flavor_guard,
                            });
                        } else {
                            let canonical_name = if addon_ns_var.as_deref() == Some(root_name.as_str()) {
                                ADDON_NS_NAME.to_string()
                            } else if let Some(class_name) = class_vars.get(root_name) {
                                class_name.clone()
                            } else { root_name.clone() };
                            globals.push(ExternalGlobal {
                                name: canonical_name,
                                kind: ExternalGlobalKind::Method(intermediates, method_name.clone(), is_colon),
                                params, returns: annotations.returns, overloads,
                                doc: annotations.doc, deprecated: annotations.deprecated,
                                nodiscard: annotations.nodiscard, constructor: annotations.constructor,
                                visibility: annotations.visibility,
                                generics: annotations.generics, defclass: annotations.defclass, defclass_parent: annotations.defclass_parent,
                                source_path: owned_path.clone(),
                                def_start, def_end,
                                builds_field: annotations.builds_field.clone(),
                                built_name: annotations.built_name,
                                built_extends: annotations.built_extends,
                                type_narrows: annotations.type_narrows,
                                type_narrows_class: annotations.type_narrows_class.clone(),
                                string_value: None, number_value: None,
                                is_override: false,
                                see: see.clone(),
                                flavors: 0,
                                flavor_guard: annotations.flavor_guard,
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
                            // Extract @type annotation for the variable (e.g. `---@type Button\nFoo = nil`)
                            let annotations = extract_annotations(assign.syntax());
                            let returns = annotations.var_type.into_iter().collect();
                            globals.push(ExternalGlobal {
                                name: names[0].clone(), kind,
                                params: Vec::new(), returns, overloads: Vec::new(),
                                doc: None, deprecated: false, nodiscard: false, constructor: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                                string_value, number_value,
                                is_override: false,
                                see: Vec::new(),
                                flavors: 0, flavor_guard: 0,
                            });
                        } else if names.len() >= 2 {
                            let root_name = &names[0];
                            let is_addon_root = addon_ns_var.as_deref() == Some(root_name.as_str());
                            // Only emit chains of 3+ parts when rooted at the addon namespace.
                            // Non-addon deep writes (e.g. `FrameClass.Inner.x = 1`) are dropped
                            // to avoid fabricating sub-tables on unrelated external classes.
                            if names.len() >= 3 && !is_addon_root { continue; }
                            let intermediates: Vec<String> = names[1..names.len()-1].to_vec();
                            let field_name = names[names.len()-1].clone();
                            let canonical_name = if is_addon_root {
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
                                kind: ExternalGlobalKind::TableField(intermediates, field_name.clone(), value_kind),
                                params: Vec::new(), returns, overloads: Vec::new(),
                                doc: annotations.doc, deprecated: false, nodiscard: false, constructor: false,
                                visibility: default_visibility_for_name(&field_name), generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                                string_value: None, number_value: None,
                                is_override: false,
                                see: Vec::new(),
                                flavors: 0, flavor_guard: 0,
                            });
                            // For depth-2 assignments on the addon ns, track the assigned field
                            // name so methods on buffered local tables can be flushed post-loop.
                            if is_addon_root && names.len() == 2 {
                                addon_assigned_fields.insert(field_name.clone());
                                if let Expression::Identifier(rhs_ident) = &exprs[0] {
                                    let rhs_names = rhs_ident.names();
                                    if rhs_names.len() == 1 && local_tables.contains(&rhs_names[0]) {
                                        local_table_to_addon_field.insert(rhs_names[0].clone(), field_name.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Flush buffered local table methods onto their addon namespace sub-tables.
    // The local root name is rewritten to the addon field alias and prepended to
    // the buffered intermediates — e.g. `function Db.A.B:Foo()` buffered with
    // path=["A","B"], once flushed via `ns.Db = Db`, becomes a Method under the
    // addon ns with path=["Db","A","B"], which walk_deep_path then resolves as
    // `ns.Db.A.B:Foo()` (auto-creating any missing intermediate sub-tables).
    for (local_name, addon_field) in &local_table_to_addon_field {
        if let Some(methods) = local_table_methods.remove(local_name) {
            for mut m in methods {
                m.name = ADDON_NS_NAME.to_string();
                if let ExternalGlobalKind::Method(ref path, ref mname, is_colon) = m.kind {
                    let mut new_path = Vec::with_capacity(path.len() + 1);
                    new_path.push(addon_field.clone());
                    new_path.extend_from_slice(path);
                    m.kind = ExternalGlobalKind::Method(new_path, mname.clone(), is_colon);
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
fn extract_string_arg_from_call_chain(call: &FunctionCall<'_>) -> Option<String> {
    // Check if this call uses colon syntax (method call)
    let ident = call.identifier()?;
    let is_colon = ident.is_call_to_self();
    if is_colon
        && let Some(arg_list) = call.arguments() {
            let args = arg_list.expressions();
            if let Some(Expression::Literal(lit)) = args.first()
                && let Some(s) = lit.get_string() {
                    let name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
                    if !name.is_empty() {
                        return Some(name);
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
pub fn scan_defclass_calls(root: SyntaxNode<'_>, all_globals: &[ExternalGlobal], all_classes: &[ClassDecl]) -> Vec<ClassDecl> {
    use std::collections::{HashMap, HashSet};
    let Some(block) = Block::cast(root) else { return Vec::new() };

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
        let Some(func_path) = func_path(g) else { continue };
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
        /// Recursive field entries extracted from a table literal argument
        table_literal_fields: Vec<DefclassFieldEntry>,
        /// Index signature type from parent class (for typing absorbed fields)
        index_sig_type: Option<AnnotationType>,
    }

    // Helper: walk a FunctionCall chain to find the innermost defclass call.
    // For `DefineClass("X"):AddDep("y"):AddDep("z")`, walks through the nested
    // FunctionCall nodes in the Identifier to find the one matching a defclass func.
    fn find_defclass_in_chain(
        call: &FunctionCall<'_>,
        defclass_funcs: &HashMap<String, DefclassFuncInfo>,
    ) -> Option<DefclassCallResult> {
        let ident = call.identifier()?;
        let func_names = ident.names();
        if func_names.is_empty() { return None; }
        let func_path = func_names.join(".");

        // Check if this call itself is a defclass function
        let matched = defclass_funcs.iter().find_map(|(dc, info)| {
            if func_path == *dc || func_path.ends_with(&format!(".{}", dc.split('.').next_back().unwrap_or(""))) {
                Some(info)
            } else {
                None
            }
        });
        if let Some(info) = matched {
            let arg_list = call.arguments()?;
            let call_args = arg_list.expressions();
            if let Some(Expression::Literal(lit)) = call_args.first()
                && let Some(s) = lit.get_string() {
                    let name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
                    let mut parents = info.parents.clone();
                    let mut constraint_type_arg_subs = Vec::new();
                    // Extract specific parent from the call argument
                    if let Some(idx) = info.parent_param_idx
                        && let Some(parent_name) = call_args.get(idx).and_then(|arg| {
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
                    // Extract field names from table literal argument (recursively for nested constructors)
                    let table_literal_fields = info.values_param_idx
                        .and_then(|idx| call_args.get(idx))
                        .map(|arg| {
                            if let Expression::TableConstructor(tc) = arg {
                                extract_table_literal_fields(tc)
                            } else {
                                Vec::new()
                            }
                        })
                        .unwrap_or_default();
                    return Some(DefclassCallResult { name, parents, constraint_type_arg_subs, table_literal_fields, index_sig_type: info.index_sig_type.clone() });
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
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(*c) } else { None } } else { None }
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
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(*c) } else { None } } else { None }
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
                if let Some(&parent_result_idx) = var_to_result.get(parent.as_str())
                    && parent_result_idx < results.len() {
                        *parent = results[parent_result_idx].name.clone();
                    }
            }
            for (_, resolved_args) in &mut result.constraint_type_arg_subs {
                for arg in resolved_args {
                    if let Some(&parent_result_idx) = var_to_result.get(arg.as_str())
                        && parent_result_idx < results.len() {
                            *arg = results[parent_result_idx].name.clone();
                        }
                }
            }
            // Convert table literal field entries to ClassDecl fields, using index signature type if available.
            // For nested table constructors, create synthetic sub-classes.
            let default_type = result.index_sig_type.unwrap_or_else(|| AnnotationType::Simple("any".to_string()));
            let mut fields: Vec<(String, AnnotationType, Visibility)> = Vec::new();
            let mut field_ranges: HashMap<String, (u32, u32)> = HashMap::new();
            let mut nested_classes: Vec<ClassDecl> = Vec::new();
            fn collect_nested_classes(
                parent_name: &str,
                entries: Vec<DefclassFieldEntry>,
                default_type: &AnnotationType,
                nested_classes: &mut Vec<ClassDecl>,
                fields: &mut Vec<(String, AnnotationType, Visibility)>,
                field_ranges: &mut HashMap<String, (u32, u32)>,
            ) {
                for entry in entries {
                    // Record field name source range for go-to-definition
                    if entry.name_start != 0 || entry.name_end != 0 {
                        field_ranges.insert(entry.name.clone(), (entry.name_start, entry.name_end));
                    }
                    if !entry.children.is_empty() {
                        // Create a synthetic class for this nested group
                        let synthetic_name = format!("{}_{}", parent_name, entry.name);
                        let mut sub_fields = Vec::new();
                        let mut sub_field_ranges = HashMap::new();
                        // Recurse for deeper nesting
                        collect_nested_classes(&synthetic_name, entry.children, default_type, nested_classes, &mut sub_fields, &mut sub_field_ranges);
                        // Inherit from the index sig value type (e.g. EnumValue)
                        let nested_parents = if let AnnotationType::Simple(type_name) = default_type {
                            if type_name != "any" { vec![type_name.clone()] } else { Vec::new() }
                        } else { Vec::new() };
                        nested_classes.push(ClassDecl {
                            name: synthetic_name.clone(),
                            type_params: Vec::new(),
                            type_param_constraints: Vec::new(),
                            parents: nested_parents,
                            fields: sub_fields,
                            accessors: Vec::new(),
                            overloads: Vec::new(),
                            generics: Vec::new(),
                            constructor_methods: Vec::new(),
                            constraint_type_arg_subs: Vec::new(),
                            field_built_names: HashMap::new(),
                            is_enum: false,
                            correlated_groups: Vec::new(),
                            def_range: None,
                            def_path: None,
                            field_ranges: sub_field_ranges,
                            field_paths: HashMap::new(),
                            see: Vec::new(),
                        });
                        fields.push((entry.name.clone(), AnnotationType::Simple(synthetic_name), default_visibility_for_name(&entry.name)));
                    } else {
                        fields.push((entry.name.clone(), default_type.clone(), default_visibility_for_name(&entry.name)));
                    }
                }
            }
            collect_nested_classes(&result.name, result.table_literal_fields, &default_type, &mut nested_classes, &mut fields, &mut field_ranges);
            // Push synthetic nested classes first so they're registered before the parent
            results.extend(nested_classes);
            let idx = results.len();
            if let Some(var_name) = lhs_var_name {
                var_to_result.insert(var_name, idx);
            }
            // Use the statement's text range as the definition location
            let stmt_range = stmt.syntax().text_range();
            results.push(ClassDecl {
                name: result.name,
                type_params: Vec::new(),
                type_param_constraints: Vec::new(),
                parents: result.parents,
                fields,
                accessors: Vec::new(),
                overloads: Vec::new(),
                generics: Vec::new(),
                constructor_methods: Vec::new(),
                constraint_type_arg_subs: result.constraint_type_arg_subs,
                field_built_names: HashMap::new(),
                is_enum: false,
                correlated_groups: Vec::new(),
                def_range: Some((u32::from(stmt_range.start()), u32::from(stmt_range.end()))),
                def_path: None,
                field_ranges,
                field_paths: HashMap::new(),
                see: Vec::new(),
            });
        }
    }

    // Second pass: scan for constructor method definitions and extract self.X = ... fields
    if !results.is_empty() && !constructor_names.is_empty() {
        // Build lookup: func_path → return types for resolving function call RHS in constructors
        let mut global_returns: HashMap<String, Vec<AnnotationType>> = HashMap::new();
        for g in all_globals {
            let Some(path) = func_path(g) else { continue };
            if !g.returns.is_empty() {
                global_returns.insert(path, g.returns.clone());
            }
        }

        // Build @built-name lookup: func_path → param_index for extracting built table names
        let mut built_name_funcs: HashMap<String, usize> = HashMap::new();
        for g in all_globals.iter().filter(|g| g.built_name.is_some()) {
            let Some(path) = func_path(g) else { continue };
            built_name_funcs.insert(path, g.built_name.unwrap());
        }
        // Propagate @built-name through wrapper functions: if a function returns a class
        // whose method (e.g. __init) has @built-name, treat the wrapper as having @built-name too.
        let mut class_init_built_name: HashMap<String, usize> = HashMap::new();
        for g in all_globals.iter().filter(|g| g.built_name.is_some()) {
            if matches!(&g.kind, ExternalGlobalKind::Method(_, _, _)) {
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
                    let Some(path) = func_path(g) else { continue };
                    built_name_funcs.entry(path).or_insert(param_idx);
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
                    if let Expression::FunctionCall(call) = expr
                        && let Some((built_name, _)) = extract_built_name_from_chain(call, &built_name_funcs) {
                            class_field_built_names.entry(result_idx)
                                .or_default()
                                .insert(field_name.clone(), built_name);
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
                fn find_root_field(call: &FunctionCall<'_>) -> Option<(String, String)> {
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
                if let Some((root_var, field_name)) = find_root_field(call)
                    && let Some(&result_idx) = var_to_result.get(&root_var) {
                        class_field_built_names.entry(result_idx)
                            .or_default()
                            .insert(field_name, built_name);
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
                        default_visibility_for_name(field_name),
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
                let ctor_fields = extract_self_fields(body, &global_returns, &field_types, &field_built_names);
                for entry in ctor_fields {
                    if !existing_fields.contains(&entry.name) {
                        let vis = default_visibility_for_name(&entry.name);
                        if let Some(range) = entry.byte_range {
                            results[result_idx].field_ranges.entry(entry.name.clone()).or_insert(range);
                        }
                        results[result_idx].fields.push((
                            entry.name,
                            entry.annotation_type,
                            vis,
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
fn extract_self_fields(block: Block<'_>, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>) -> Vec<SelfFieldEntry> {
    let mut fields = Vec::new();
    let mut seen = HashSet::new();
    let mut field_types = field_types.clone();
    extract_self_fields_inner(block, &mut fields, &mut seen, global_returns, &mut field_types, field_built_names);
    fields
}

/// Infer an `AnnotationType` from a constructor RHS expression.
fn infer_type_from_expression(expr: &Expression<'_>, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>) -> AnnotationType {
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
    call: &FunctionCall<'_>,
    built_name_funcs: &HashMap<String, usize>,
) -> Option<(String, String)> {
    let ident = call.identifier()?;
    let func_names = ident.names();
    if func_names.is_empty() { return None; }
    let func_path = func_names.join(".");

    let matched = built_name_funcs.iter().find_map(|(path, idx)| {
        if func_path == *path || func_path.ends_with(&format!(".{}", path.split('.').next_back().unwrap_or(""))) {
            Some((*idx, path.clone()))
        } else {
            None
        }
    });
    if let Some((param_idx, matched_path)) = matched {
        let arg_list = call.arguments()?;
        let call_args = arg_list.expressions();
        if let Some(Expression::Literal(lit)) = call_args.get(param_idx - 1)
            && let Some(s) = lit.get_string() {
                return Some((s.trim_matches(|c| c == '"' || c == '\'').to_string(), matched_path));
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
///
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
    call: &FunctionCall<'_>,
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
fn extract_type_annotation_for_assign(node: SyntaxNode<'_>) -> Option<AnnotationType> {
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

/// Try to extract a `---@type X` annotation from an inline trailing comment on the same line
/// as an assignment statement. Finds the last "content" token (non-trivia) in the statement,
/// then walks forward looking for a `---@type` comment before any newline.
fn extract_inline_type_annotation(node: SyntaxNode<'_>) -> Option<AnnotationType> {
    // Find the last non-trivia token in the statement (e.g. `nil`, `true`, `"str"`)
    let mut last_content = None;
    for item in node.descendants_with_tokens() {
        if let NodeOrToken::Token(ref t) = item {
            match t.kind() {
                SyntaxKind::Comment | SyntaxKind::Whitespace | SyntaxKind::Newline => {}
                _ => last_content = Some(*t),
            }
        }
    }
    let last_content = last_content?;
    // Walk forward from the last content token looking for a ---@type comment on the same line
    let mut tok = last_content.next_token();
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Whitespace => { tok = t.next_token(); }
            SyntaxKind::Newline => return None,
            SyntaxKind::Comment => {
                let text = t.text().to_string();
                if let Some(rest) = text.strip_prefix("---@type ").or_else(|| text.strip_prefix("---@type\t")) {
                    let trimmed = rest.trim();
                    if !trimmed.is_empty() {
                        return Some(parse_type(trimmed));
                    }
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}

fn extract_self_fields_inner(block: Block<'_>, fields: &mut Vec<SelfFieldEntry>, seen: &mut HashSet<String>, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &mut HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>) {
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
                                // Try @type annotation (preceding line, then inline), then infer from expression
                                let ann_type = extract_type_annotation_for_assign(assign.syntax())
                                    .or_else(|| extract_inline_type_annotation(assign.syntax()))
                                    .unwrap_or_else(|| {
                                        exprs.get(i)
                                            .map(|e| infer_type_from_expression(e, global_returns, field_types, field_built_names))
                                            .unwrap_or_else(|| AnnotationType::Simple("any".to_string()))
                                    });
                                // Track non-any types so later fields can reference them
                                if !matches!(&ann_type, AnnotationType::Simple(s) if s == "any") {
                                    field_types.insert(field_name.clone(), ann_type.clone());
                                }
                                // Extract byte range of the field name token
                                let field_range = ident.syntax().children_with_tokens()
                                    .filter_map(|c| c.into_token()).find(|t| t.kind() == SyntaxKind::Name && t.text() != "self")
                                    .map(|t| {
                                        let r = t.text_range();
                                        (u32::from(r.start()), u32::from(r.end()))
                                    });
                                fields.push(SelfFieldEntry {
                                    name: field_name.clone(), annotation_type: ann_type,
                                    byte_range: field_range,
                                });
                            }
                        }
                    }
                }
            }
            // Recurse into nested blocks
            Statement::If(if_chain) => {
                for child in if_chain.syntax().children() {
                    if let Some(b) = Block::cast(child) {
                        extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names);
                }
            }
            _ => {}
        }
    }
}

/// Scan a file for calls to functions with `@built-name`, extracting the class name
/// from the specified string literal argument. Returns empty `ClassDecl` entries so the
/// name is registered in `PreResolvedGlobals` for cross-file annotation resolution.
pub fn scan_built_name_calls(root: SyntaxNode<'_>, all_globals: &[ExternalGlobal]) -> Vec<ClassDecl> {
    use std::collections::HashMap;
    let Some(block) = Block::cast(root) else { return Vec::new() };

    // Build map of function paths → param index for @built-name
    let mut built_name_funcs: HashMap<String, usize> = HashMap::new();
    // Also track which schema class each func_path belongs to
    let mut func_path_to_schema: HashMap<String, String> = HashMap::new();
    for g in all_globals.iter().filter(|g| g.built_name.is_some()) {
        let Some(path) = func_path(g) else { continue };
        func_path_to_schema.insert(path.clone(), g.name.clone());
        built_name_funcs.insert(path, g.built_name.unwrap());
    }

    // Propagate @built-name through wrapper functions: if a function returns a class
    // whose method (e.g. __init) has @built-name, treat the wrapper as having @built-name too.
    let mut class_init_built_name: HashMap<String, usize> = HashMap::new();
    for g in all_globals.iter().filter(|g| g.built_name.is_some()) {
        if matches!(&g.kind, ExternalGlobalKind::Method(_, _, _)) {
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
                let Some(path) = func_path(g) else { continue };
                func_path_to_schema.entry(path.clone()).or_insert(schema_class);
                built_name_funcs.entry(path).or_insert(param_idx);
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
        let Some(method_path) = func_path(g) else { continue };
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
            ExternalGlobalKind::Method(_, _, _) => &g.name,
            _ => continue,
        };
        for rt in &g.returns {
            if let AnnotationType::Simple(s) = rt
                && let Some(parent) = s.strip_prefix("built:") {
                    schema_built_parent.entry(class_name.clone()).or_insert_with(|| parent.to_string());
                }
        }
    }

    // Helper: walk a FunctionCall chain to find a @built-name call
    // Returns (class_name, matched_func_path_key)
    fn find_built_name_in_chain(
        call: &FunctionCall<'_>,
        built_name_funcs: &HashMap<String, usize>,
    ) -> Option<(String, String)> {
        let ident = call.identifier()?;
        let func_names = ident.names();
        if func_names.is_empty() { return None; }
        let func_path = func_names.join(".");

        let matched = built_name_funcs.iter().find_map(|(path, idx)| {
            if func_path == *path || func_path.ends_with(&format!(".{}", path.split('.').next_back().unwrap_or(""))) {
                Some((*idx, path.clone()))
            } else {
                None
            }
        });
        if let Some((param_idx, matched_path)) = matched {
            let arg_list = call.arguments()?;
            let call_args = arg_list.expressions();
            if let Some(Expression::Literal(lit)) = call_args.get(param_idx - 1)
                && let Some(s) = lit.get_string() {
                    return Some((s.trim_matches(|c| c == '"' || c == '\'').to_string(), matched_path));
                }
            return None;
        }

        // Not a built-name call — check nested chain
        let nested = ident.syntax().children().find_map(FunctionCall::cast)?;
        find_built_name_in_chain(&nested, built_name_funcs)
    }

    // Helper: walk a FunctionCall chain and extract fields from @builds-field methods.
    // Returns Vec<(field_name, field_type, Visibility)> for all builder calls in the chain.
    fn extract_built_fields_from_chain(
        call: &FunctionCall<'_>,
        schema_class: &str,
        builds_field_funcs: &HashMap<String, BuildsFieldInfo>,
    ) -> Vec<(String, AnnotationType, Visibility)> {
        let mut fields = Vec::new();
        collect_built_fields(call, schema_class, builds_field_funcs, &mut fields);
        fields
    }

    fn collect_built_fields(
        call: &FunctionCall<'_>,
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
                    if let Some(Expression::Literal(lit)) = args.get(info.param_idx - 1)
                        && let Some(s) = lit.get_string() {
                            let field_name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
                            // Resolve generic type params from backtick call arguments
                            let field_type = resolve_builds_field_generics(
                                &info.field_type, &info.generics, &info.params, &args,
                            );
                            fields.push((field_name.clone(), field_type, default_visibility_for_name(&field_name)));
                        }
                }
            }
        }

        // Recurse into nested FunctionCall in the identifier (inner chain call)
        if let Some(nested) = ident.syntax().children().find_map(FunctionCall::cast) {
            collect_built_fields(&nested, schema_class, builds_field_funcs, fields);
        }
    }

    /// Extract the generic name from a backtick annotation, searching inside unions.
    fn find_backtick_generic_name(ann: &AnnotationType) -> Option<&str> {
        match ann {
            AnnotationType::Backtick(inner) => {
                if let AnnotationType::Simple(name) = inner.as_ref() {
                    Some(name.as_str())
                } else {
                    None
                }
            }
            AnnotationType::Union(members) => members.iter().find_map(find_backtick_generic_name),
            AnnotationType::NonNil(inner) => find_backtick_generic_name(inner),
            _ => None,
        }
    }

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
            // Find param with Backtick(Simple(gen_name)) type, including inside unions
            for (i, param) in params.iter().enumerate() {
                if let Some(name) = find_backtick_generic_name(&param.typ)
                    && name == gen_name {
                        // Get the string literal at this arg position
                        if let Some(Expression::Literal(lit)) = call_args.get(i)
                            && let Some(s) = lit.get_string() {
                                subs.insert(gen_name.clone(), s.trim_matches(|c| c == '"' || c == '\'').to_string());
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
            AnnotationType::NonNil(inner) => {
                AnnotationType::NonNil(Box::new(substitute_annotation_generics(inner, subs)))
            }
            AnnotationType::Intersection(types) => {
                AnnotationType::Intersection(types.iter().map(|t| substitute_annotation_generics(t, subs)).collect())
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
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(*c) } else { None } } else { None }
                })
            }
            Statement::Assign(a) => {
                a.expression_list().and_then(|el| {
                    let exprs = el.expressions();
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(*c) } else { None } } else { None }
                })
            }
            // Expression statements: ClassName._FIELD:Extend("Name"):...:Commit()
            Statement::FunctionCall(c) => Some(*c),
            _ => None,
        };
        let Some(call) = rhs_call else { continue };

        if let Some((name, matched_path)) = find_built_name_in_chain(&call, &built_name_funcs)
            && seen.insert(name.clone()) {
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
                    type_param_constraints: Vec::new(),
                    parents,
                    fields,
                    accessors: Vec::new(),
                    overloads: Vec::new(),
                    generics: Vec::new(),
                    constructor_methods: Vec::new(),
                    constraint_type_arg_subs: Vec::new(),
                    field_built_names: HashMap::new(),
                    is_enum: false,
                    correlated_groups: Vec::new(),
                    def_range: None,
                    def_path: None,
                    field_ranges: HashMap::new(),
                    field_paths: HashMap::new(),
                    see: Vec::new(),
                });
            }
    }
    results
}

/// Scan a file for typed self-field assignments in method bodies.
/// Finds `self.field = expr ---@type Type` (or preceding-line form) in colon-syntax
/// methods where the receiver name matches a known class name.
pub fn scan_method_typed_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
) -> Vec<TypedSelfField> {
    let mut results = Vec::new();
    for child in root.children() {
        let Some(func) = crate::ast::FunctionDefinition::cast(child) else { continue };
        let Some(ident) = func.identifier() else { continue };
        if !ident.is_call_to_self() { continue; }
        let names = ident.names();
        if names.len() < 2 { continue; }
        let receiver = &names[0];
        if !known_classes.contains(receiver) { continue; }
        let Some(body) = func.block() else { continue };
        // Walk the method body for typed self-field assignments
        let mut seen = HashSet::new();
        let mut field_list = Vec::new();
        scan_typed_self_fields_inner(body, &mut field_list, &mut seen);
        for (field_name, ann_type, range) in field_list {
            let vis = default_visibility_for_name(&field_name);
            results.push(TypedSelfField {
                class_name: receiver.clone(), field_name, annotation_type: ann_type,
                visibility: vis, byte_range: range,
            });
        }
    }
    results
}

/// Walk a block for `self.field = expr` assignments that have explicit `---@type` annotations.
/// Only captures fields with type annotations (not inferred).
fn scan_typed_self_fields_inner(
    block: Block<'_>,
    fields: &mut Vec<(String, AnnotationType, (u32, u32))>,
    seen: &mut HashSet<String>,
) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Assign(assign) => {
                if let Some(vl) = assign.variable_list() {
                    for ident in vl.identifiers() {
                        let names = ident.names();
                        if names.len() == 2 && names[0] == "self" {
                            let field_name = &names[1];
                            if !seen.insert(field_name.clone()) { continue; }
                            // Only capture fields with explicit ---@type annotations
                            let ann_type = extract_type_annotation_for_assign(assign.syntax())
                                .or_else(|| extract_inline_type_annotation(assign.syntax()));
                            if let Some(ann_type) = ann_type {
                                let field_range = ident.syntax().children_with_tokens()
                                    .filter_map(|c| c.into_token()).find(|t| t.kind() == SyntaxKind::Name && t.text() != "self")
                                    .map(|t| {
                                        let r = t.text_range();
                                        (u32::from(r.start()), u32::from(r.end()))
                                    });
                                if let Some(range) = field_range {
                                    fields.push((field_name.clone(), ann_type, range));
                                }
                            }
                        }
                    }
                }
            }
            Statement::If(if_chain) => {
                for child in if_chain.syntax().children() {
                    if let Some(b) = Block::cast(child) {
                        scan_typed_self_fields_inner(b, fields, seen);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            _ => {}
        }
    }
}

// ── Type conversion ──────────────────────────────────────────────────────────

/// Walk `at` through `NonNil` and `Union(T, nil)` wrappers and `alias_fun_types`
/// chains to find the underlying `AnnotationType::Fun(..)`. Returns the terminal
/// Fun annotation together with a flag indicating whether the outer wrap
/// contributed a nil member (e.g. `FunAlias?` or `FunAlias | nil`).
///
/// Returns `None` when `at` doesn't reduce to a function type — unions with
/// multiple non-nil members, aliases pointing at non-function types, or chains
/// that cycle are all rejected. A `HashSet` tracks visited alias names to bound
/// traversal regardless of chain depth.
pub(crate) fn reduce_to_fun_alias<'a>(
    at: &'a AnnotationType,
    local_aliases: &'a std::collections::HashMap<String, AnnotationType>,
    ext_aliases: &'a std::collections::HashMap<String, AnnotationType>,
) -> Option<(&'a AnnotationType, bool)> {
    let (mut current, wraps_nil) = match at {
        AnnotationType::NonNil(inner) => (inner.as_ref(), false),
        AnnotationType::Union(parts) => {
            let has_nil = parts.iter()
                .any(|p| matches!(p, AnnotationType::Simple(s) if s == "nil"));
            let mut non_nil = parts.iter()
                .filter(|p| !matches!(p, AnnotationType::Simple(s) if s == "nil"));
            let first = non_nil.next()?;
            if non_nil.next().is_some() { return None; }
            (first, has_nil)
        }
        _ => (at, false),
    };
    let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
    loop {
        match current {
            AnnotationType::Fun(..) => return Some((current, wraps_nil)),
            AnnotationType::Simple(name) => {
                if !visited.insert(name.as_str()) { return None; }
                current = local_aliases.get(name)
                    .or_else(|| ext_aliases.get(name))?;
            }
            _ => return None,
        }
    }
}

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
                "any" | "unknown" => return Some(ValueType::Any),
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
        AnnotationType::NonNil(inner) => resolve_annotation_type(inner, generics, classes, aliases),
        AnnotationType::Intersection(parts) => {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| resolve_annotation_type(p, generics, classes, aliases)).collect();
            match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => Some(ValueType::Intersection(converted)),
            }
        }
        AnnotationType::TableLiteral(_) => {
            // TableLiteral needs mutable access to create TableInfo entries.
            // The immutable resolve returns Table(None); resolve_annotation_type_mut
            // in prescan.rs handles creating the actual table.
            Some(ValueType::Table(None))
        }
        AnnotationType::VarArgs(inner) => resolve_annotation_type(inner, generics, classes, aliases),
        AnnotationType::Tuple(..) => None,
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
            "any" | "unknown" => Some(ValueType::Any),
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
        AnnotationType::NonNil(inner) => annotation_type_to_value_type(inner),
        AnnotationType::Intersection(parts) => {
            let converted: Vec<ValueType> = parts.iter().filter_map(annotation_type_to_value_type).collect();
            match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => Some(ValueType::Intersection(converted)),
            }
        }
        AnnotationType::TableLiteral(_) => Some(ValueType::Table(None)),
        AnnotationType::VarArgs(inner) => annotation_type_to_value_type(inner),
        AnnotationType::Tuple(..) => None,
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

pub fn scan_diagnostic_directives(root: SyntaxNode<'_>) -> Vec<DiagnosticSuppression> {
    let source = root.tree.source().to_string();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(source.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i + 1))
        .collect();

    let mut suppressions = Vec::new();
    for element in root.descendants_with_tokens() {
        let NodeOrToken::Token(tok) = element else { continue };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_external_global(name: &str, kind: ExternalGlobalKind) -> ExternalGlobal {
        ExternalGlobal {
            name: name.to_string(),
            kind,
            params: Vec::new(),
            returns: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            constructor: false,
            visibility: Visibility::Public,
            generics: Vec::new(),
            defclass: None,
            defclass_parent: None,
            source_path: None,
            def_start: 0,
            def_end: 0,
            builds_field: None,
            built_name: None,
            built_extends: false,
            type_narrows: None,
            type_narrows_class: None,
            string_value: None,
            number_value: None,
            is_override: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
        }
    }

    fn parse_tree(text: &str) -> crate::syntax::tree::SyntaxTree {
        crate::syntax::parser::parse(text)
    }

    #[test]
    fn scan_built_name_detects_chain_method_change() {
        // Regression test: when a builder chain switches from one @builds-field
        // method to another (e.g. AddOptionalField → AddRequiredField), the
        // discovered ClassDecl fields must differ. This is the condition that
        // maybe_rebuild_workspace checks to decide whether to rebuild
        // PreResolvedGlobals.

        // Create external globals for a schema with @built-name and two builder methods
        let mut create_method = make_external_global("Schema", ExternalGlobalKind::Method(Vec::new(), "Create".to_string(), true));
        create_method.built_name = Some(1); // param 1 is the class name
        create_method.params = vec![ParamInfo { name: "name".into(), typ: AnnotationType::Simple("string".into()), optional: false, description: None }];
        create_method.returns = vec![AnnotationType::Simple("Schema".into())];

        let mut add_optional = make_external_global("Schema", ExternalGlobalKind::Method(Vec::new(), "AddOptionalField".to_string(), true));
        add_optional.builds_field = Some((1, AnnotationType::Union(vec![
            AnnotationType::Simple("string".into()),
            AnnotationType::Simple("nil".into()),
        ])));
        add_optional.params = vec![ParamInfo { name: "name".into(), typ: AnnotationType::Simple("string".into()), optional: false, description: None }];
        add_optional.returns = vec![AnnotationType::Simple("Schema".into())];

        let mut add_required = make_external_global("Schema", ExternalGlobalKind::Method(Vec::new(), "AddRequiredField".to_string(), true));
        add_required.builds_field = Some((1, AnnotationType::Simple("string".into())));
        add_required.params = vec![ParamInfo { name: "name".into(), typ: AnnotationType::Simple("string".into()), optional: false, description: None }];
        add_required.returns = vec![AnnotationType::Simple("Schema".into())];

        let globals = vec![create_method, add_optional, add_required];

        // Source A: chain uses AddOptionalField
        let tree_a = parse_tree(r#"local tbl = Schema:Create("MyState"):AddOptionalField("name")"#);
        let root_a = SyntaxNode::new_root(&tree_a);
        let result_a = scan_built_name_calls(root_a, &globals);

        // Source B: chain uses AddRequiredField (same class name, different method)
        let tree_b = parse_tree(r#"local tbl = Schema:Create("MyState"):AddRequiredField("name")"#);
        let root_b = SyntaxNode::new_root(&tree_b);
        let result_b = scan_built_name_calls(root_b, &globals);

        assert_eq!(result_a.len(), 1, "should discover MyState from chain A");
        assert_eq!(result_b.len(), 1, "should discover MyState from chain B");
        assert_eq!(result_a[0].name, "MyState");
        assert_eq!(result_b[0].name, "MyState");

        // The key assertion: the discovered fields must differ because the
        // chain methods have different @builds-field types. This difference
        // is what triggers a PreResolvedGlobals rebuild in maybe_rebuild_workspace.
        assert_ne!(result_a[0].fields, result_b[0].fields,
            "different builder methods must produce different ClassDecl fields");

        // Verify the specific field types
        assert_eq!(result_a[0].fields.len(), 1);
        assert_eq!(result_a[0].fields[0].0, "name");
        assert!(matches!(&result_a[0].fields[0].1, AnnotationType::Union(_)),
            "AddOptionalField should produce a union type (string | nil)");

        assert_eq!(result_b[0].fields.len(), 1);
        assert_eq!(result_b[0].fields[0].0, "name");
        assert!(matches!(&result_b[0].fields[0].1, AnnotationType::Simple(s) if s == "string"),
            "AddRequiredField should produce a simple string type");
    }
}
