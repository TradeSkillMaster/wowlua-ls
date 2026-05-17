use std::collections::{HashMap, HashSet};
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

// ── Shared helpers ─────────────────────────────────────────────────────────

/// Flatten control-flow blocks: recurse into do/if/while/repeat/for bodies so that
/// assignments (and their annotations) inside conditionals are visible to cross-file scanning.
pub(crate) fn collect_statements_recursive<'a>(block: &Block<'a>, out: &mut Vec<Statement<'a>>) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Do(group) => {
                // Do-blocks are purely structural wrappers — surface inner
                // statements without pushing the Do itself.
                if let Some(inner_block) = group.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::If(chain) => {
                out.push(stmt);
                for branch in chain.if_branches() {
                    if let Some(inner_block) = branch.block() {
                        collect_statements_recursive(&inner_block, out);
                    }
                }
                if let Some(else_branch) = chain.else_branch()
                    && let Some(inner_block) = else_branch.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::While(w) => {
                out.push(stmt);
                if let Some(inner_block) = w.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::Repeat(r) => {
                out.push(stmt);
                if let Some(inner_block) = r.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::ForCountLoop(f) => {
                out.push(stmt);
                if let Some(inner_block) = f.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::ForInLoop(f) => {
                out.push(stmt);
                if let Some(inner_block) = f.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            _ => {
                out.push(stmt);
            }
        }
    }
}

// ── Shared types and constants ──────────────────────────────────────────────

pub(crate) const ADDON_NS_NAME: &str = "__addon_ns__";

/// Build a dotted path string for a method/function global.
/// Returns the fully qualified dotted name: `root.int1.int2.method` for methods
/// or just `root` for top-level functions. Returns None for non-method/function
/// variants (TableField, Variable, etc.).
pub(crate) fn func_path(g: &ExternalGlobal) -> Option<String> {
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
pub enum FieldValueKind { String, Number, Boolean, Nil, Table(Vec<(std::string::String, FieldValueKind)>), Function, FunctionCall(Vec<std::string::String>, Option<std::string::String>), FieldRef(Vec<std::string::String>), Unknown }

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
    #[serde(default)]
    pub return_names: Vec<Option<String>>,
    #[serde(default)]
    pub return_descriptions: Vec<Option<String>>,
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
    /// True when the function body has no non-bare `return` statements (only
    /// bare `return` or fall-through). Propagated to `Function::implicit_nil_return`
    /// so cross-file callers correctly infer nil instead of `?`.
    #[serde(default)]
    pub implicit_nil_return: bool,
    /// `@narrows-arg N` — calling this function narrows the Nth argument's type.
    #[serde(default)]
    pub narrows_arg: Option<usize>,
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

/// Extract an inline `---@class ClassName` from a trailing comment on the same line as
/// an assignment node. Only finds comments before the first newline in the node's
/// descendants. Preceding-line `---@class` annotations are handled by `extract_annotations`;
/// this function is the fallback for the `MyVar = {} ---@class Foo` pattern.
pub(crate) fn extract_inline_class(node: SyntaxNode<'_>) -> Option<String> {
    extract_inline_class_with_offset(node).map(|(name, _)| name)
}

/// Like `extract_inline_class`, but also returns the byte offset of the `@class` comment token
/// for positional disambiguation when multiple `@class` declarations share the same name.
pub(crate) fn extract_inline_class_with_offset(node: SyntaxNode<'_>) -> Option<(String, u32)> {
    let mut past_newline = false;
    for item in node.descendants_with_tokens() {
        if let NodeOrToken::Token(t) = item {
            if t.kind() == SyntaxKind::Newline {
                past_newline = true;
            } else if past_newline {
                break;
            } else if t.kind() == SyntaxKind::Comment {
                let text = t.text();
                let content = text.trim_start_matches('-').trim();
                if let Some(rest) = content.strip_prefix("@class") {
                    let rest = rest.trim();
                    let offset = u32::from(t.text_range().start());
                    return rest.split_whitespace().next()
                        .map(|s| (s.trim_end_matches(':').to_string(), offset));
                }
            }
        }
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

/// Build a per-file mapping from local variable names to `@class` names.
/// Handles `--- @class Foo\nlocal Bar = ...` and inline `local Bar = ... ---@class Foo`.
fn build_var_to_class(all_stmts: &[Statement<'_>]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for stmt in all_stmts {
        if let Statement::LocalAssign(assign) = stmt {
            let annotations = super::extract_annotations(assign.syntax());
            let class_name = annotations.class
                .or_else(|| extract_inline_class(assign.syntax()));
            if let Some(class_name) = class_name
                && let Some(name_list) = assign.name_list() {
                    let names = name_list.names();
                    if names.len() == 1 {
                        map.insert(names[0].clone(), class_name);
                    }
                }
        }
    }
    map
}

/// Scan a file for typed self-field assignments in method bodies.
/// Finds `self.field = expr ---@type Type` (or preceding-line form) in colon-syntax
/// methods where the receiver name matches a known class name.
pub(crate) fn scan_method_typed_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
    implicit_protected_prefix: bool,
) -> Vec<TypedSelfField> {
    let mut results = Vec::new();
    let Some(block) = Block::cast(root) else { return results };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);
    let var_to_class = build_var_to_class(&all_stmts);
    for stmt in &all_stmts {
        let Statement::FunctionDefinition(func) = stmt else { continue };
        let Some(ident) = func.identifier() else { continue };
        if !ident.is_call_to_self() { continue; }
        let names = ident.names();
        if names.len() < 2 { continue; }
        let receiver = &names[0];
        let class_name = if known_classes.contains(receiver) {
            receiver.clone()
        } else if let Some(cn) = var_to_class.get(receiver).filter(|cn| known_classes.contains(*cn)) {
            cn.clone()
        } else {
            continue;
        };
        let Some(body) = func.block() else { continue };
        // Walk the method body for typed self-field assignments
        let mut seen = HashSet::new();
        let mut field_list = Vec::new();
        scan_typed_self_fields_inner(body, &mut field_list, &mut seen);
        for (field_name, ann_type, range) in field_list {
            let vis = default_visibility_for_name(&field_name, implicit_protected_prefix);
            results.push(TypedSelfField {
                class_name: class_name.clone(), field_name, annotation_type: ann_type,
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
            Statement::Repeat(r) => {
                if let Some(b) = r.syntax().children().find_map(Block::cast) {
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

/// Scan method bodies for `self.field = funcall()` without explicit `---@type`.
/// Returns ExternalGlobal entries with FieldValueKind::FunctionCall so that
/// build_on_stubs can resolve the return type through the normal funcall chain.
pub(crate) fn scan_method_funcall_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
    implicit_protected_prefix: bool,
    typed_self_field_names: &HashSet<(String, String)>,
    source_path: Option<PathBuf>,
) -> Vec<ExternalGlobal> {
    let mut results = Vec::new();
    let Some(block) = Block::cast(root) else { return results };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);
    let var_to_class = build_var_to_class(&all_stmts);
    for stmt in &all_stmts {
        let Statement::FunctionDefinition(func) = stmt else { continue };
        let Some(ident) = func.identifier() else { continue };
        if !ident.is_call_to_self() { continue; }
        let names = ident.names();
        if names.len() < 2 { continue; }
        let receiver = &names[0];
        let class_name = if known_classes.contains(receiver) {
            receiver.clone()
        } else if let Some(cn) = var_to_class.get(receiver).filter(|cn| known_classes.contains(*cn)) {
            cn.clone()
        } else {
            continue;
        };
        let Some(body) = func.block() else { continue };
        let mut seen = HashSet::new();
        let mut field_list = Vec::new();
        scan_funcall_self_fields_inner(body, &class_name, &mut field_list, &mut seen);
        for (field_name, callee_names, first_string_arg, range) in field_list {
            // Skip fields already captured with explicit @type
            if typed_self_field_names.contains(&(class_name.clone(), field_name.clone())) {
                continue;
            }
            let vis = default_visibility_for_name(&field_name, implicit_protected_prefix);
            results.push(ExternalGlobal {
                name: class_name.clone(),
                kind: ExternalGlobalKind::TableField(
                    Vec::new(),
                    field_name,
                    FieldValueKind::FunctionCall(callee_names, first_string_arg),
                ),
                params: Vec::new(), returns: Vec::new(), return_names: Vec::new(), return_descriptions: Vec::new(),
                overloads: Vec::new(), doc: None, deprecated: false, nodiscard: false,
                constructor: false, visibility: vis,
                generics: Vec::new(), defclass: None, defclass_parent: None,
                source_path: source_path.clone(),
                def_start: range.0, def_end: range.1,
                builds_field: None, built_name: None, built_extends: false,
                type_narrows: None, type_narrows_class: None,
                string_value: None, number_value: None,
                is_override: false, see: Vec::new(), flavors: 0, flavor_guard: 0,
                implicit_nil_return: false,
                narrows_arg: None,
            });
        }
    }
    results
}

/// (field_name, callee_names, first_string_arg, byte_range)
type FuncallSelfField = (String, Vec<String>, Option<String>, (u32, u32));

/// Walk a block for `self.field = call()` assignments without explicit `---@type`.
fn scan_funcall_self_fields_inner(
    block: Block<'_>,
    class_name: &str,
    fields: &mut Vec<FuncallSelfField>,
    seen: &mut HashSet<String>,
) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Assign(assign) => {
                if let Some(vl) = assign.variable_list() {
                    let exprs = assign.expression_list().map(|el| el.expressions()).unwrap_or_default();
                    for (i, ident) in vl.identifiers().iter().enumerate() {
                        let names = ident.names();
                        if names.len() == 2 && names[0] == "self" {
                            let field_name = &names[1];
                            if !seen.insert(field_name.clone()) { continue; }
                            // Skip if there's an explicit @type annotation
                            if extract_type_annotation_for_assign(assign.syntax()).is_some()
                                || extract_inline_type_annotation(assign.syntax()).is_some()
                            {
                                continue;
                            }
                            // Only handle function call expressions
                            let Some(Expression::FunctionCall(call)) = exprs.get(i) else { continue };
                            let Some(call_ident) = call.identifier() else { continue };
                            let mut callee_names = call_ident.names();
                            if callee_names.is_empty() { continue; }
                            // Canonicalize `self` → class name in callee chain
                            if callee_names[0] == "self" {
                                callee_names[0] = class_name.to_string();
                            }
                            let first_string_arg = call.arguments().and_then(|al| {
                                let args = al.expressions();
                                if let Some(Expression::Literal(lit)) = args.first() {
                                    lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                } else {
                                    None
                                }
                            });
                            let field_range = ident.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name && t.text() != "self")
                                .map(|t| {
                                    let r = t.text_range();
                                    (u32::from(r.start()), u32::from(r.end()))
                                });
                            if let Some(range) = field_range {
                                fields.push((field_name.clone(), callee_names, first_string_arg, range));
                            }
                        }
                    }
                }
            }
            Statement::If(if_chain) => {
                for child in if_chain.syntax().children() {
                    if let Some(b) = Block::cast(child) {
                        scan_funcall_self_fields_inner(b, class_name, fields, seen);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            Statement::Repeat(r) => {
                if let Some(b) = r.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            _ => {}
        }
    }
}

// ── Bare self-field scanning ─────────────────────────────────────────────────

/// Scan method bodies for `self.field = expr` assignments that have neither
/// explicit `---@type` annotations nor function-call RHS. Infers types from:
/// - `@param` annotations on the enclosing function (when RHS is a parameter name)
/// - Literal expressions (number, string, boolean)
/// - Table constructors → `table`
/// - Falls back to `any` for other expressions
///
/// Results have lowest priority: only adds fields not already captured by
/// typed or funcall self-field scans.
pub(crate) fn scan_method_bare_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
    implicit_protected_prefix: bool,
    already_captured: &HashSet<(String, String)>,
) -> Vec<TypedSelfField> {
    let mut results = Vec::new();
    let Some(block) = Block::cast(root) else { return results };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);
    let var_to_class = build_var_to_class(&all_stmts);
    for stmt in &all_stmts {
        let Statement::FunctionDefinition(func) = stmt else { continue };
        let Some(ident) = func.identifier() else { continue };
        if !ident.is_call_to_self() { continue; }
        let names = ident.names();
        if names.len() < 2 { continue; }
        let receiver = &names[0];
        let class_name = if known_classes.contains(receiver) {
            receiver.clone()
        } else if let Some(cn) = var_to_class.get(receiver).filter(|cn| known_classes.contains(*cn)) {
            cn.clone()
        } else {
            continue;
        };
        let Some(body) = func.block() else { continue };
        // Build param name → type map from @param annotations
        let annotations = super::extract_annotations(func.syntax());
        let param_types: HashMap<&str, &AnnotationType> = annotations.params.iter()
            .map(|p| (p.name.as_str(), &p.typ))
            .collect();
        let mut seen = HashSet::new();
        let mut field_list = Vec::new();
        scan_bare_self_fields_inner(body, &param_types, &mut field_list, &mut seen);
        for (field_name, ann_type, range) in field_list {
            if already_captured.contains(&(class_name.clone(), field_name.clone())) {
                continue;
            }
            let vis = default_visibility_for_name(&field_name, implicit_protected_prefix);
            results.push(TypedSelfField {
                class_name: class_name.clone(), field_name, annotation_type: ann_type,
                visibility: vis, byte_range: range,
            });
        }
    }
    results
}

/// Walk a block for bare `self.field = expr` assignments (no @type, not funcall).
fn scan_bare_self_fields_inner(
    block: Block<'_>,
    param_types: &HashMap<&str, &AnnotationType>,
    fields: &mut Vec<(String, AnnotationType, (u32, u32))>,
    seen: &mut HashSet<String>,
) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Assign(assign) => {
                if let Some(vl) = assign.variable_list() {
                    let exprs = assign.expression_list().map(|el| el.expressions()).unwrap_or_default();
                    for (i, ident) in vl.identifiers().iter().enumerate() {
                        let names = ident.names();
                        if names.len() == 2 && names[0] == "self" {
                            let field_name = &names[1];
                            if !seen.insert(field_name.clone()) { continue; }
                            // Skip if there's an explicit @type annotation (handled by typed scan)
                            if extract_type_annotation_for_assign(assign.syntax()).is_some()
                                || extract_inline_type_annotation(assign.syntax()).is_some()
                            {
                                continue;
                            }
                            // Infer type from RHS expression
                            let ann_type = match exprs.get(i) {
                                // Skip funcall RHS (handled by funcall scan)
                                Some(Expression::FunctionCall(_)) => continue,
                                // Skip function literals (not useful cross-file)
                                Some(Expression::Function(_)) => continue,
                                Some(Expression::Literal(lit)) => {
                                    if lit.is_nil() {
                                        // Skip nil-only assignments
                                        continue;
                                    } else if lit.get_number().is_some() {
                                        AnnotationType::Simple("number".into())
                                    } else if lit.get_string().is_some() {
                                        AnnotationType::Simple("string".into())
                                    } else if lit.get_bool().is_some() {
                                        AnnotationType::Simple("boolean".into())
                                    } else {
                                        AnnotationType::Simple("any".into())
                                    }
                                }
                                Some(Expression::TableConstructor(_)) => {
                                    AnnotationType::Simple("table".into())
                                }
                                Some(Expression::Identifier(rhs_ident)) => {
                                    let rhs_names = rhs_ident.names();
                                    if rhs_names.len() == 1 {
                                        if let Some(param_type) = param_types.get(rhs_names[0].as_str()) {
                                            (*param_type).clone()
                                        } else {
                                            AnnotationType::Simple("any".into())
                                        }
                                    } else {
                                        AnnotationType::Simple("any".into())
                                    }
                                }
                                // No RHS or complex expression
                                _ => AnnotationType::Simple("any".into()),
                            };
                            let field_range = ident.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name && t.text() != "self")
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
            Statement::If(if_chain) => {
                for child in if_chain.syntax().children() {
                    if let Some(b) = Block::cast(child) {
                        scan_bare_self_fields_inner(b, param_types, fields, seen);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            Statement::Repeat(r) => {
                if let Some(b) = r.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
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
                _ => Some(ValueType::make_union(converted)),
            }
        }
        AnnotationType::Array(_inner) => Some(ValueType::Table(None)),
        AnnotationType::Parameterized(base, _args) => {
            // expression<C, R> is a built-in type for inline Lua expressions;
            // at the ValueType level it's just a string (the annotation metadata
            // is preserved on param_annotations for call-site analysis).
            if base == "expression" {
                return Some(ValueType::String(None));
            }
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
        AnnotationType::VarArgs(inner) => {
            // Check for variadic generic: `...M` where `@generic ...M` was declared.
            // Non-Simple inner types (e.g. `...SomeComplex<T>`) fall through to
            // normal resolution, which is correct — only bare names can be variadics.
            if let AnnotationType::Simple(name) = inner.as_ref() {
                let dotted = format!("...{}", name);
                if generics.iter().any(|(g, _)| g == &dotted) {
                    return Some(ValueType::TypeVariable(dotted));
                }
            }
            resolve_annotation_type(inner, generics, classes, aliases)
        }
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

/// Extract a `---@type X` annotation from a syntax node (table field, expression, etc.).
///
/// Checks three locations in order:
/// 1. Trailing comment within the node (after the last `Name` token)
/// 2. Sibling comment after the node (same line)
/// 3. Preceding standalone comment on the line above the node
///
/// Used by both per-file analysis (build_ir) and cross-file scanning (scan_globals).
pub(crate) fn extract_inline_type_from_node(field_node: SyntaxNode<'_>) -> Option<AnnotationType> {
    // Check within the node itself: find the last Name token and walk forward
    // on the same line. This handles Identifier nodes that capture trailing comments.
    let mut last_name_tok = None;
    for item in field_node.children_with_tokens() {
        if let NodeOrToken::Token(t) = &item
            && t.kind() == SyntaxKind::Name {
                last_name_tok = Some(*t);
            }
    }
    if let Some(name_tok) = last_name_tok {
        let node_end = u32::from(field_node.text_range().end());
        let mut tok = name_tok.next_token();
        while let Some(t) = tok {
            if u32::from(t.text_range().start()) >= node_end { break; }
            match t.kind() {
                SyntaxKind::Whitespace | SyntaxKind::Comma | SyntaxKind::Semicolon => {
                    tok = t.next_token();
                }
                SyntaxKind::Comment => {
                    let text = t.text();
                    let content = text.trim_start_matches('-').trim();
                    if let Some(rest) = content.strip_prefix("@type") {
                        let rest = rest.trim();
                        if !rest.is_empty() {
                            return Some(parse_type(rest));
                        }
                    }
                    break;
                }
                _ => break,
            }
        }
    }
    // Check for trailing sibling comments on the same line as the field
    let last_token = field_node.last_token()?;
    let mut tok = last_token.next_token();
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Comma | SyntaxKind::Whitespace | SyntaxKind::Semicolon => {
                tok = t.next_token();
            }
            SyntaxKind::Comment => {
                let text = t.text();
                let content = text.trim_start_matches('-').trim();
                if let Some(rest) = content.strip_prefix("@type") {
                    let rest = rest.trim();
                    if !rest.is_empty() {
                        return Some(parse_type(rest));
                    }
                }
                break;
            }
            _ => break,
        }
    }
    // Fall back to preceding comments on lines above the field, matching
    // the `@field`-style pattern that many WoW addon codebases use:
    //     ---@type Pool<number>
    //     pool = Pool.New(),
    // A preceding `@type` comment is only valid when it sits ALONE on
    // its own line — i.e. only whitespace or a newline precedes it. A
    // comment like `prev = v, ---@type X` on the previous line is a
    // TRAILING comment on `prev` and must not be captured for this field.
    let first_token = field_node.first_token()?;
    let mut tok = first_token.prev_token();
    let mut crossed_newline = false;
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Whitespace => {
                tok = t.prev_token();
            }
            SyntaxKind::Newline => {
                crossed_newline = true;
                tok = t.prev_token();
            }
            SyntaxKind::Comment if crossed_newline => {
                // Verify the comment is standalone: only whitespace/newline
                // between it and the preceding newline (i.e. it's on a
                // line by itself, not trailing another statement).
                let mut back = t.prev_token();
                let mut standalone = true;
                while let Some(b) = back {
                    match b.kind() {
                        SyntaxKind::Whitespace => back = b.prev_token(),
                        SyntaxKind::Newline => break,
                        _ => { standalone = false; break; }
                    }
                }
                if !standalone { return None; }
                let text = t.text();
                let content = text.trim_start_matches('-').trim();
                if let Some(rest) = content.strip_prefix("@type") {
                    let rest = rest.trim();
                    if !rest.is_empty() {
                        return Some(parse_type(rest));
                    }
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}
