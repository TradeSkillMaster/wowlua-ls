use std::collections::HashSet;
use std::path::PathBuf;
use crate::ast::{AstNode, Block, Expression, Statement};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::{TableIndex, ValueType};
use super::{
    AnnotationType, ParamInfo, TypedSelfField, Visibility,
    default_visibility_for_name,
};
use super::annotation_types::{parse_type, OverloadSig};

// ── Shared types and constants ──────────────────────────────────────────────

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

// ── Shared helpers (used by scan_defclass + scan_method_typed_self_fields) ───

/// Try to extract a `---@type X` annotation from the comments preceding an assignment statement.
/// Only considers standalone annotation comments (on their own line), not inline trailing comments.
pub(super) fn extract_type_annotation_for_assign(node: SyntaxNode<'_>) -> Option<AnnotationType> {
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
pub(super) fn extract_inline_type_annotation(node: SyntaxNode<'_>) -> Option<AnnotationType> {
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

// ── Typed self-field scanning ───────────────────────────────────────────────

/// Scan a file for typed self-field assignments in method bodies.
/// Finds `self.field = expr ---@type Type` (or preceding-line form) in colon-syntax
/// methods where the receiver name matches a known class name.
pub fn scan_method_typed_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
    implicit_protected_prefix: bool,
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
            let vis = default_visibility_for_name(&field_name, implicit_protected_prefix);
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

// ── Type conversion ─────────────────────────────────────────────────────────

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
    classes: &std::collections::HashMap<String, TableIndex>,
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

// ── Diagnostic suppression scanning ─────────────────────────────────────────

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
