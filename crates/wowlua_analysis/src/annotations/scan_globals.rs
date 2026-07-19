use std::collections::{HashMap, HashSet};
use std::path::Path;
use crate::ast::{AstNode, Block, Statement, Expression, ExpressionList, FunctionCall, Operator,
    LocalAssign, FunctionDefinition, ForCountLoop, ForInLoop, ParameterList};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use super::{
    AnnotationType, ParamInfo, Visibility,
    extract_annotations, default_visibility_for_name,
};
use super::annotation_types::{parse_overload, OverloadSig};
use super::annotation_scanning::{
    ADDON_NS_NAME, ExternalGlobal, ExternalGlobalKind, FieldValueKind, InferredTypeCategory,
    is_select_varargs, collect_statements_recursive, infer_type_category, receiver_name,
    build_var_to_class, funcall_has_chained_receiver,
};

/// First string-literal argument of `call`'s (outer) argument list — used as a
/// class-name hint for defclass resolution (`DefineClass("X")`).
///
/// Returns `None` for a chained receiver (`Inner("X"):Outer(...)`): the outer
/// method transforms the type, so the inner string names the *receiver*, not the
/// field's class (`LibStub("CallbackHandler-1.0"):New()` yields a registry, not
/// the library). Forwarding it would let the class-name fallback in
/// `build_on_stubs` mis-resolve the field to that receiver class. The guard lives
/// here so both sites feeding this hint stay consistent: the direct
/// `ns.field = call()` path and the `local x = call(); ns.field = x` forwarding
/// path (`local_call_origins`). (The sibling `local x = getter():M("Class")` case,
/// where the *local itself* is mis-typed as the class, is guarded separately at the
/// `class_vars` defclass-heuristic site.)
fn first_string_literal_arg(call: &FunctionCall<'_>) -> Option<String> {
    if funcall_has_chained_receiver(call) {
        return None;
    }
    call.arguments().and_then(|al| {
        let args = al.expressions();
        if let Some(Expression::Literal(lit)) = args.first() {
            lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
        } else {
            None
        }
    })
}

/// The canonicalized callee name-chain AND the defclass string-arg hint to record
/// for a `field = call(...)` / `X = call(...)` RHS. Both are empty/`None` when the
/// receiver is itself a call (`LibStub("X"):M()`, `Foo():M()`) or the callee has
/// no name identifier (`(cond and F1 or F2)(...)`).
///
/// For a chained receiver `ident.names()` collapses to just the trailing method
/// name, dropping the receiver — that bare name then mis-resolves in
/// `resolve_funcall_chain` as a *global* function of the same name (e.g. the WoW
/// API global `GetLocale()` shadowing an AceLocale `:GetLocale()` method,
/// yielding a bogus `string`). An anonymous callee has no usable name at all.
/// Either way an empty chain makes `resolve_funcall_chain` bail, so the field
/// falls to the refineable table placeholder and per-file resolution supplies the
/// precise type.
///
/// The string-arg hint (`first_string_literal_arg`, the `DefineClass("X")`
/// class-name heuristic) is paired here so it is returned ONLY alongside a
/// non-empty chain: `first_string_literal_arg`'s own guard suppresses chained
/// receivers but NOT the anonymous-callee case, so returning it unconditionally
/// would let `(cond and F1 or F2)("Class")` mis-type the field to `Class` via the
/// class-name fallback in `build_on_stubs`. A genuine named-defclass call
/// (`DefineClass("X")`) keeps its hint.
///
/// The chain root is canonicalized to the addon-namespace sentinel / declared
/// class / local-type name so cross-file resolution can find it.
fn scan_funcall_callee(
    call: &FunctionCall<'_>,
    addon_ns_var: Option<&str>,
    class_vars: &HashMap<String, String>,
    local_type_vars: &HashMap<String, String>,
) -> (Vec<String>, Option<String>) {
    if funcall_has_chained_receiver(call) {
        return (Vec::new(), None);
    }
    let mut names = call.identifier().map(|ident| ident.names()).unwrap_or_default();
    if names.is_empty() {
        return (Vec::new(), None);
    }
    if addon_ns_var == Some(names[0].as_str()) {
        names[0] = ADDON_NS_NAME.to_string();
    } else if let Some(cn) = class_vars.get(&names[0]) {
        names[0] = cn.clone();
    } else if let Some(tn) = local_type_vars.get(&names[0]) {
        names[0] = tn.clone();
    }
    let first_string_arg = first_string_literal_arg(call);
    (names, first_string_arg)
}

/// Unwrap `and`/`or` chains to the effective operand for type inference.
/// `a and b` evaluates to `b` when `a` is truthy, so the effective type is `b`.
/// `a or b` evaluates to `b` when `a` is falsy (the defensive-init pattern
/// `x = x or fallback`), so `b` is the best hint for the field's type.
fn unwrap_logical_chain<'a>(mut expr: Expression<'a>) -> Expression<'a> {
    loop {
        if let Expression::BinaryExpression(ref bin) = expr
            && matches!(bin.kind(), Operator::And | Operator::Or)
        {
            let terms = bin.get_terms();
            if terms.len() == 2 {
                expr = terms[1];
                continue;
            }
        }
        return expr;
    }
}

/// Collect every name bound as a local *anywhere* in the file: `local` declarations,
/// `local function` names, function parameters (named and anonymous), and for-loop
/// variables — including those inside function bodies. Used to decide whether a bare
/// `X = ...` assignment nested in a function body is a genuine implicit global creation
/// (name absent from this set) or just a reassignment of a function-scoped local.
///
/// This is a deliberate file-wide over-approximation: if a name is *ever* a local in
/// the file, an assignment to it is treated as a local write and not registered as a
/// cross-file global. That conservatively avoids leaking function locals as phantom
/// globals at the cost of occasionally missing a genuine global that shadows a same-named
/// local elsewhere — the safe direction for the coarse cross-file scan.
fn collect_all_local_names(root: &SyntaxNode<'_>) -> HashSet<String> {
    let mut locals: HashSet<String> = HashSet::new();
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::LocalAssignStatement => {
                if let Some(assign) = LocalAssign::cast(node)
                    && let Some(name_list) = assign.name_list() {
                    locals.extend(name_list.names());
                }
            }
            SyntaxKind::FunctionDefinition => {
                if let Some(func) = FunctionDefinition::cast(node)
                    && func.is_local()
                    && let Some(name) = func.name() {
                    locals.insert(name);
                }
            }
            SyntaxKind::ParameterList => {
                if let Some(params) = ParameterList::cast(node) {
                    locals.extend(params.parameters());
                }
            }
            SyntaxKind::ForCountLoop => {
                if let Some(for_loop) = ForCountLoop::cast(node)
                    && let Some(name) = for_loop.name() {
                    locals.insert(name);
                }
            }
            SyntaxKind::ForInLoop => {
                if let Some(for_loop) = ForInLoop::cast(node)
                    && let Some(name_list) = for_loop.name_list() {
                    locals.extend(name_list.names());
                }
            }
            _ => {}
        }
    }
    locals
}

/// Extract named field kinds from a table constructor for `FieldValueKind::Table`.
fn extract_table_field_kinds(tc: &crate::ast::TableConstructor<'_>) -> Vec<(String, FieldValueKind)> {
    let mut fields = Vec::new();
    for field in tc.fields() {
        if let Some(crate::ast::FieldKind::Named { name, value }) = field.kind() {
            let kind = classify_expression_value_kind(&value);
            fields.push((name, kind));
        }
    }
    fields
}

/// Classify a literal expression (including negated number literals) into a `FieldValueKind`,
/// preserving literal values (string text, number text) when available.
fn classify_literal_value_kind(expr: &Expression<'_>) -> Option<FieldValueKind> {
    match expr {
        Expression::Literal(lit) => {
            if let Some(s) = lit.get_string() { Some(FieldValueKind::String(Some(s))) }
            else if lit.get_bool().is_some() { Some(FieldValueKind::Boolean) }
            else if let Some(n) = lit.get_number() { Some(FieldValueKind::Number(Some(n))) }
            else if lit.is_nil() { Some(FieldValueKind::Nil) }
            else { None }
        }
        Expression::UnaryExpression(u) if matches!(u.kind(), crate::ast::Operator::Subtract) => {
            super::annotation_scanning::extract_number_from_expr(expr)
                .map(|n| FieldValueKind::Number(Some(n)))
        }
        _ => None,
    }
}

/// Classify any expression into a `FieldValueKind`, using the shared
/// `infer_type_category` for operator-to-type mappings. Falls back to
/// `classify_literal_value_kind` first to preserve literal values (e.g. string
/// text, number text), then to `infer_type_category` for expression shapes, and
/// finally handles `Table`/`Function` structurally.
fn classify_expression_value_kind(expr: &Expression<'_>) -> FieldValueKind {
    // Try literal classification first to preserve exact values.
    if let Some(kind) = classify_literal_value_kind(expr) {
        return kind;
    }
    // Delegate to the shared helper for operator-based inference. Table and
    // Function are handled structurally below (Table needs recursive sub-field
    // extraction; Function is a simple tag).
    if let Some(cat) = infer_type_category(expr) {
        return match cat {
            InferredTypeCategory::String => FieldValueKind::String(None),
            InferredTypeCategory::Number => FieldValueKind::Number(None),
            InferredTypeCategory::Boolean => FieldValueKind::Boolean,
            InferredTypeCategory::Nil => FieldValueKind::Nil,
            InferredTypeCategory::Function => FieldValueKind::Function,
            // For tables we need recursive field extraction, handled below.
            InferredTypeCategory::Table => {
                if let Expression::TableConstructor(tc) = expr {
                    FieldValueKind::Table(extract_table_field_kinds(tc))
                } else {
                    FieldValueKind::Unknown
                }
            }
        };
    }
    FieldValueKind::Unknown
}

/// Extract parent class names from a `CreateFromMixins(Base1, Base2, …)` call.
/// Returns an empty vec for any other callee. Each identifier argument whose
/// name resolves to a known class variable contributes a parent name.
fn extract_mixin_parents(
    call: &FunctionCall<'_>,
    callee_names: &[String],
    class_vars: &HashMap<String, String>,
) -> Vec<String> {
    if callee_names.len() != 1 || callee_names[0] != "CreateFromMixins" {
        return Vec::new();
    }
    let Some(arg_list) = call.arguments() else { return Vec::new() };
    let mut parents = Vec::new();
    for arg in arg_list.expressions() {
        if let Expression::Identifier(ident) = &arg {
            let arg_names = ident.names();
            if arg_names.len() == 1 {
                let resolved = class_vars.get(&arg_names[0])
                    .cloned()
                    .unwrap_or_else(|| arg_names[0].clone());
                parents.push(resolved);
            }
        }
    }
    parents
}

/// Emit an `ExternalGlobal` as a Method on the namespace/class, resolving
/// visibility from the function's own annotation or the field name convention.
/// When `addon_assigned_fields` is Some, also tracks the field for local-table-
/// method flush (caller passes Some only when `is_addon_root && names.len() == 2`).
fn push_method_global(
    globals: &mut Vec<ExternalGlobal>,
    canonical_name: String,
    mut base: ExternalGlobal,
    field_name: &str,
    intermediates: Vec<String>,
    implicit_protected_prefix: bool,
    addon_assigned_fields: Option<&mut HashSet<String>>,
) {
    let visibility = if base.visibility != Visibility::Public {
        base.visibility
    } else {
        default_visibility_for_name(field_name, implicit_protected_prefix)
    };
    base.name = canonical_name;
    base.kind = ExternalGlobalKind::Method(intermediates, field_name.to_string(), false);
    base.visibility = visibility;
    globals.push(base);
    if let Some(fields) = addon_assigned_fields {
        fields.insert(field_name.to_string());
    }
}

// ── Body-return bareness (workspace scan) ───────────────────────────────────

/// Walk a function body and record one `is_bare` flag per return statement
/// (`true` when the return has no expressions / empty expression list). Descends
/// into control-flow blocks (if/else/do/while/for/repeat) but NOT into nested
/// function definitions (their returns belong to a different function). This is
/// the *only* body-return information the workspace scanner records — the precise
/// return types and correlated overloads are inferred lazily by the per-file
/// harvest (`resolve_deferred_sig`).
fn collect_return_bareness(block: &Block<'_>, out: &mut Vec<bool>) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Return(ret) => {
                let is_bare = ret.expression_list()
                    .map(|el| el.expressions().is_empty())
                    .unwrap_or(true);
                out.push(is_bare);
            }
            Statement::If(chain) => {
                for branch in chain.if_branches() {
                    if let Some(inner) = branch.block() { collect_return_bareness(&inner, out); }
                }
                if let Some(eb) = chain.else_branch()
                    && let Some(inner) = eb.block() { collect_return_bareness(&inner, out); }
            }
            Statement::Do(g) => {
                if let Some(inner) = g.block() { collect_return_bareness(&inner, out); }
            }
            Statement::While(w) => {
                if let Some(inner) = w.block() { collect_return_bareness(&inner, out); }
            }
            Statement::Repeat(r) => {
                if let Some(inner) = r.block() { collect_return_bareness(&inner, out); }
            }
            Statement::ForCountLoop(f) => {
                if let Some(inner) = f.block() { collect_return_bareness(&inner, out); }
            }
            Statement::ForInLoop(f) => {
                if let Some(inner) = f.block() { collect_return_bareness(&inner, out); }
            }
            _ => {}
        }
    }
}

/// Build a complete [`ExternalGlobal`] capturing a function definition's
/// signature: params (merged with `@param` annotations), explicit `@return`
/// types and `@overload`s, `@deprecated`/`@nodiscard`/visibility/generics/etc.,
/// and the function's byte range. When the function has no `@return`, the body
/// is classified by return-statement bareness only (driving `is_body_derived`
/// for deferred membership and `implicit_nil_return`); its precise body-derived
/// return types and correlated overloads are resolved lazily by the per-file
/// harvest, not here.
///
/// The returned global has placeholder `name` (empty) and `kind`
/// ([`ExternalGlobalKind::Function`]); callers override them via struct-update
/// syntax (`ExternalGlobal { name, kind, ..base }`) to emit the function as a
/// top-level global, a method on a table/namespace, or a namespace field
/// assigned a local function (`ns.f = f`).
fn build_func_external(
    func: &crate::ast::FunctionDefinition<'_>,
    anno_node: SyntaxNode<'_>,
    is_colon: bool,
    owned_path: Option<&std::path::Path>,
) -> ExternalGlobal {
    // Annotations may live on an enclosing statement rather than the function
    // node itself (e.g. `---@param ...\nlocal f = function() end`), so the
    // caller specifies which node to scan for `---@` comments.
    let mut annotations = extract_annotations(anno_node);
    let overloads: Vec<OverloadSig> = annotations.overloads.iter()
        .filter_map(|s| parse_overload(s)).collect();
    // When the function has no `@return` annotation, classify its body returns
    // by *bareness only*.  We deliberately do NOT infer coarse return types or
    // synthesize return-only overloads here: the lazy, memoized per-file harvest
    // (`resolve_deferred_sig`) is the single source of truth for body-derived
    // return types and overloads.  This pass only records enough to drive the
    // deferred membership set and `implicit_nil_return`:
    //   - `is_body_derived` (any value-returning statement) → deferred function
    //   - `implicit_nil_return` (no return statements, or all bare returns)
    let body_return_bareness = if annotations.returns.is_empty() {
        func.block().map(|body| {
            let mut out = Vec::new();
            collect_return_bareness(&body, &mut out);
            out
        })
    } else {
        None
    };
    // Implicit nil return: every return in the body is bare (no expressions),
    // or the body has no return statements at all (`all` over an empty iter).
    let implicit_nil_return = body_return_bareness.as_ref()
        .is_some_and(|b| b.iter().all(|&is_bare| is_bare));
    // Body-derived (deferred): at least one return statement yields a value.
    // Its precise return types/overloads are resolved lazily, not here.
    let is_body_derived = body_return_bareness.as_ref()
        .is_some_and(|b| b.iter().any(|&is_bare| !is_bare));
    let range = func.syntax().text_range();
    let def_start = u32::from(range.start());
    let def_end = u32::from(range.end());
    // Find the first Name token for precise diagnostic positioning.
    let (name_start, name_end) = func.syntax().children_with_tokens()
        .find_map(|c| match c {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => {
                let r = t.text_range();
                Some((u32::from(r.start()), u32::from(r.end())))
            }
            _ => None,
        })
        .unwrap_or((def_start, def_end));
    // Merge @param annotations with actual parameter names.
    let params = if let Some(param_list) = func.params() {
        let actual_params: Vec<String> = param_list.parameters().into_iter()
            .filter(|n| !is_colon || n != "self")
            .collect();
        let mut ps: Vec<ParamInfo> = actual_params.iter()
            .map(|n| {
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
        std::mem::take(&mut annotations.params)
    } else { Vec::new() };
    ExternalGlobal {
        name: String::new(),
        kind: ExternalGlobalKind::Function,
        params,
        returns: annotations.returns,
        return_names: annotations.return_names,
        return_descriptions: annotations.return_descriptions,
        overloads,
        doc: annotations.doc,
        deprecated: annotations.deprecated,
        nodiscard: annotations.nodiscard,
        constructor: annotations.constructor,
        visibility: annotations.visibility,
        generics: annotations.generics,
        defclass: annotations.defclass,
        defclass_parent: annotations.defclass_parent,
        source_path: owned_path.map(|p| p.to_path_buf()),
        def_start,
        def_end,
        builds_field: annotations.builds_field,
        built_name: annotations.built_name,
        built_extends: annotations.built_extends,
        type_narrows: annotations.type_narrows,
        type_narrows_class: annotations.type_narrows_class,
        string_value: None,
        number_value: None,
        is_override: false,
        is_meta: false,
        see: annotations.see,
        flavors: 0,
        flavor_guard: annotations.flavor_guard,
        implicit_nil_return,
        narrows_arg: annotations.narrows_arg,
        creates_global: annotations.creates_global,
        generates_events: annotations.generates_events,
        callback_event_arg: annotations.callback_event_arg,
        requires: annotations.requires,
        body_derived_returns: is_body_derived,
        deferred_call_type: false,
        name_start,
        name_end,
        mixin_parents: Vec::new(),
        returns_class_name: annotations.returns_class_name,
    }
}

// ── Dynamic global prefix scanning ──────────────────────────────────────────

/// Minimum length for a string literal in `_G["PREFIX"..k]` to be considered
/// a valid dynamic global prefix. Short prefixes risk masking real
/// `undefined-global` diagnostics.
const MIN_DYNAMIC_PREFIX_LEN: usize = 3;

/// Check if a bracket-access node has `_G` as its base expression.
fn is_g_bracket_access(node: SyntaxNode<'_>) -> bool {
    for child in node.children_with_tokens() {
        match &child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::LeftSquareBracket => break,
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name && t.text() == "_G" => {
                return true;
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::NameRef => {
                for c in n.children_with_tokens() {
                    if let NodeOrToken::Token(t) = c
                        && t.kind() == SyntaxKind::Name
                        && t.text() == "_G"
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Scan a file for `_G["PREFIX" .. k] = value` patterns and return glob patterns
/// like `"PREFIX*"` for each detected prefix. These patterns are added as
/// workspace-wide allowed globals so that reads of `PREFIX<anything>` in other
/// files don't false-positive as `undefined-global`.
///
/// Detection is deliberately conservative:
///
/// - Only `_G[<concat>]` on the LHS of an assignment is recognized.
/// - The concatenation must have a string literal operand of at least 3 chars.
/// - Fully dynamic writes (`_G[k] = v` with no literal part) are ignored.
pub fn scan_dynamic_global_prefixes(root: SyntaxNode<'_>) -> Vec<String> {
    let Some(block) = Block::cast(root) else { return Vec::new(); };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);

    let mut seen = std::collections::HashSet::new();
    let mut prefixes = Vec::new();
    for stmt in &all_stmts {
        if let Statement::Assign(assign) = stmt
            && let Some(var_list) = assign.variable_list()
        {
            for ident in var_list.identifiers() {
                if ident.syntax().kind() != SyntaxKind::BracketAccess {
                    continue;
                }
                if !is_g_bracket_access(ident.syntax()) {
                    continue;
                }
                if let Some((literal, is_prefix)) = crate::ast::extract_bracket_concat_string_literal(ident.syntax())
                    && literal.len() >= MIN_DYNAMIC_PREFIX_LEN
                {
                    let pattern = if is_prefix {
                        format!("{}*", literal)
                    } else {
                        format!("*{}", literal)
                    };
                    if seen.insert(pattern.clone()) {
                        prefixes.push(pattern);
                    }
                }
            }
        }
    }
    prefixes
}

// ── Global declaration scanning ─────────────────────────────────────────────

/// Whether a file scan synthesizes correlated return-only overloads for
/// unannotated multi-return functions (driven by
/// `inference.correlated_return_overloads`). Travels with [`ProtectedPrefix`]
/// as a pair from the LSP scan wrappers down to [`scan_file_globals_with_synth`]
/// in place of a bare `bool`, so the two flags can't be silently swapped.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CorrelatedReturns {
    /// Synthesize correlated return-only overloads.
    Synthesize,
    /// Leave them unsynthesized.
    Skip,
}

impl CorrelatedReturns {
    /// Build from a config-derived `inference.correlated_return_overloads` flag.
    pub fn from_enabled(enabled: bool) -> Self {
        if enabled { Self::Synthesize } else { Self::Skip }
    }
}

/// Whether an `_`-prefixed field/name defaults to `@protected` visibility
/// (driven by `inference.implicit_protected_prefix`). Travels with
/// [`CorrelatedReturns`] as a pair; [`scan_file_globals_with_synth`] is the
/// deepest signature carrying the pair — below it the flag is threaded as a
/// lone `bool` (`implicit_protected_prefix`) through the per-element scanners,
/// so the enum boundary stops there.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProtectedPrefix {
    /// `_name` is implicitly `@protected`.
    Implicit,
    /// Visibility comes only from explicit annotations.
    Explicit,
}

impl ProtectedPrefix {
    /// Build from a config-derived `inference.implicit_protected_prefix` flag.
    pub fn from_enabled(enabled: bool) -> Self {
        if enabled { Self::Implicit } else { Self::Explicit }
    }
    pub fn is_implicit(self) -> bool {
        matches!(self, Self::Implicit)
    }
}

pub fn scan_file_globals(root: SyntaxNode<'_>, source_path: Option<&Path>) -> Vec<ExternalGlobal> {
    scan_file_globals_with_synth(root, source_path, CorrelatedReturns::Synthesize, ProtectedPrefix::Explicit, &CreatesGlobalMap::new()).0
}

/// Variant of [`scan_file_globals`] retaining the per-file
/// [`CorrelatedReturns`] flag in its signature for caller compatibility.
/// The flag no longer affects workspace scanning: correlated return-only
/// overloads (and all body-derived return types) are now synthesized lazily by
/// the per-file harvest (`resolve_deferred_sig`), which reads
/// `inference.correlated_return_overloads` directly from the file's config. The
/// parameter is therefore unused here.
/// Returns `(globals, addon_ns_class_name)`.
/// `addon_ns_class_name` is `Some(class_name)` when the addon namespace variable
/// (the second value from `...`) also has a `@class` annotation, establishing a
/// relationship between the addon namespace table and a named class.
pub fn scan_file_globals_with_synth(
    root: SyntaxNode<'_>,
    source_path: Option<&Path>,
    _correlated_returns: CorrelatedReturns,
    protected_prefix: ProtectedPrefix,
    creates_global_specs: &CreatesGlobalMap,
) -> (Vec<ExternalGlobal>, Option<String>) {
    // `protected_prefix` travels as part of the swap-prone pair down to here;
    // below this point it's threaded as a plain `bool` through the per-element
    // scanners, so unwrap it once.
    let implicit_protected_prefix = protected_prefix.is_implicit();
    let owned_path = source_path.map(|p| p.to_path_buf());
    let Some(block) = Block::cast(root) else { return (Vec::new(), None); };

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

    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);

    // Track local aliases to known tables (e.g. `local str = string`, `local tab = table`)
    let mut local_aliases: HashMap<String, String> = HashMap::new();
    // Track local variables assigned table constructors (e.g. `local Locale = {}`)
    let mut local_tables: HashSet<String> = HashSet::new();
    // Track local functions (e.g. `local function Foo()` or `local Foo = function()`)
    let mut local_functions: HashSet<String> = HashSet::new();
    // Track ALL local variable names so we can skip field/method assignments on
    // non-class, non-table locals (e.g. `local frame = CreateFrame(...); frame.x = 1`
    // should not create a phantom global class "frame").
    let mut local_vars: HashSet<String> = HashSet::new();
    // Byte offset of the *first* `local X` declaration for each name. A plain
    // `X = ...` assignment is only a reassignment of that local (not an implicit
    // global) if it appears at or after the local comes into scope. Tracking the
    // earliest declaration offset lets a genuine global assignment that *precedes*
    // a later same-named `local X` (e.g. `X = 100` then `local X = X`) still be
    // recognized as a global.
    let mut first_local_offset: HashMap<String, u32> = HashMap::new();
    // Track local variables annotated with @class (e.g. local LibTSMCore = {} ---@class LibTSMCore)
    let mut class_vars: HashMap<String, String> = HashMap::new();
    // Track locals with @type annotations so field assignments on them are emitted
    // under the annotated class name (cross-file overlay tracking).
    let mut local_type_vars: HashMap<String, String> = HashMap::new();
    for stmt in &all_stmts {
        if let Statement::LocalAssign(assign) = stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                let decl_offset: u32 = assign.syntax().text_range().start().into();
                for name in &names {
                    local_vars.insert(name.clone());
                    first_local_offset.entry(name.clone()).or_insert(decl_offset);
                }
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
                    if matches!(&exprs[0], Expression::Function(_)) {
                        local_functions.insert(names[0].clone());
                    }
                }

                // @class annotation (preceding or inline trailing comment)
                let annotations = extract_annotations(assign.syntax());
                let class_name = annotations.class
                    .or_else(|| super::extract_inline_class(assign.syntax()));
                if let Some(class_name) = class_name {
                    if names.len() == 1 {
                        class_vars.insert(names[0].clone(), class_name);
                    } else if names.len() >= 2 {
                        // @class on multi-assignment annotates the first variable
                        // (e.g. `---@class Lib \n local Lib, oldMinor = LibStub:NewLibrary(...)`)
                        // Also handle addon namespace in last position.
                        if addon_ns_var.as_deref() == Some(names.last().unwrap().as_str()) {
                            class_vars.insert(names.last().unwrap().clone(), class_name.clone());
                        }
                        class_vars.insert(names[0].clone(), class_name);
                    }
                } else if names.len() == 1 && !class_vars.contains_key(&names[0]) {
                    // @type annotation → track as local_type_vars for overlay field emission.
                    // Also populate class_vars when the variable name matches the type name
                    // (e.g. `---@type Glider \n local Glider = ns.GliderUI`), so methods
                    // defined on the local are associated with the class cross-file.
                    // Only Simple types (and the Simple member of Intersection) are matched.
                    let type_name = match &annotations.var_type {
                        Some(AnnotationType::Simple(s)) => Some(s.clone()),
                        Some(AnnotationType::Intersection(members)) => {
                            members.iter().find_map(|m| {
                                if let AnnotationType::Simple(s) = m { Some(s.clone()) } else { None }
                            })
                        }
                        _ => None,
                    };
                    if let Some(type_name) = type_name {
                        if names[0] == type_name {
                            class_vars.insert(names[0].clone(), type_name.clone());
                        }
                        local_type_vars.insert(names[0].clone(), type_name);
                    }
                    // Defclass-style calls: `local X = Y:Init("ClassName")` or `local X = DefineClass("ClassName")`.
                    // A chained receiver is allowed only for the string-keyed navigation idiom
                    // (`Base:From("Lib"):IncludeClassType("ClassName")` — every hop is `:Method("name")`,
                    // so the local genuinely becomes the named class). An instance transform
                    // (`getter():asType("ClassName")`, `h:getReg():asType("ClassName")`) has a hop that
                    // yields a runtime instance rather than a named class, so its outer string is just a
                    // parameter, not the class the local becomes — it is skipped.
                    if exprs.len() == 1
                        && let Expression::FunctionCall(call) = &exprs[0]
                        && (!funcall_has_chained_receiver(call) || chain_is_string_keyed_navigation(call)) {
                            if let Some(cn) = extract_string_arg_from_call_chain(call) {
                                class_vars.insert(names[0].clone(), cn);
                            } else if let Some(cn) = extract_first_string_arg(call) {
                                class_vars.insert(names[0].clone(), cn);
                            }
                        }
                }
            }
        if let Statement::FunctionDefinition(func) = stmt
            && func.is_local()
            && let Some(name) = func.name() {
                let decl_offset: u32 = func.syntax().text_range().start().into();
                first_local_offset.entry(name.clone()).or_insert(decl_offset);
                local_vars.insert(name.clone());
                local_functions.insert(name);
            }
        // Track @class annotations on global assignments (e.g. `---@class Foo\nMyMixin = {}`)
        // so that methods defined on the global are emitted under the class name.
        // Also handles `local name, AddOn = ... \n ---@class Foo \n AddOn = LibStub(...)`:
        // the variable is in local_vars but still needs class_vars tracking for field routing.
        if let Statement::Assign(assign) = stmt
            && let (Some(var_list), Some(expr_list)) = (assign.variable_list(), assign.expression_list()) {
                let idents = var_list.identifiers();
                let exprs = expr_list.expressions();
                if idents.len() == 1 && exprs.len() == 1 {
                    let names = idents[0].names();
                    if names.len() == 1 {
                        let annotations = extract_annotations(assign.syntax());
                        let class_name = annotations.class
                            .or_else(|| super::extract_inline_class(assign.syntax()));
                        if let Some(class_name) = class_name {
                            class_vars.insert(names[0].clone(), class_name);
                        }
                    }
                }
            }
    }

    // Track return types of same-file function definitions (e.g. `---@return Foo \n function X.bar()`)
    // so that `local x = X.bar(); Class.field = x` propagates `Foo` as the field type cross-file.
    let mut func_return_types: HashMap<String, AnnotationType> = HashMap::new();
    for stmt in &all_stmts {
        if let Statement::FunctionDefinition(func) = stmt {
            if func.is_local() { continue; }
            let Some(ident) = func.identifier() else { continue };
            let func_names = ident.names();
            if func_names.is_empty() { continue; }
            let annotations = extract_annotations(func.syntax());
            if let Some(ret) = annotations.returns.into_iter().next()
                && !matches!(&ret, AnnotationType::Simple(s) if s.is_empty()) {
                    func_return_types.insert(func_names.join("."), ret);
            }
        }
    }

    // Track local variable return types from annotated function calls
    let mut local_return_types: HashMap<String, AnnotationType> = HashMap::new();
    // Track local variables assigned from function calls whose return type isn't known
    // locally (e.g. stub/external methods).  Stores (canonicalized callee chain, first
    // string arg) so that `ns.Field = localVar` can emit FieldValueKind::FunctionCall
    // instead of FieldRef, letting build_on_stubs resolve the return type.
    let mut local_call_origins: HashMap<String, (Vec<String>, Option<String>)> = HashMap::new();
    for stmt in &all_stmts {
        if let Statement::LocalAssign(assign) = stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                let names = name_list.names();
                let exprs = expr_list.expressions();
                if names.len() == 1 && exprs.len() == 1 && !class_vars.contains_key(&names[0])
                    && let Expression::FunctionCall(call) = &exprs[0]
                    && let Some(call_ident) = call.identifier() {
                        let call_names = call_ident.names();
                        if !call_names.is_empty() {
                            let func_key = call_names.join(".");
                            if let Some(ret_type) = func_return_types.get(&func_key) {
                                local_return_types.insert(names[0].clone(), ret_type.clone());
                            } else {
                                // Return type not known from same-file definitions; store the
                                // call origin so build_on_stubs can resolve it. `scan_funcall_callee`
                                // empties the chain (and drops the string-arg hint) for a chained
                                // receiver (`local x = LibStub("X"):M()`) so the forwarded origin
                                // can't mis-resolve as a same-named global.
                                let (callee_chain, first_string_arg) = scan_funcall_callee(
                                    call, addon_ns_var.as_deref(), &class_vars, &local_type_vars,
                                );
                                local_call_origins.insert(names[0].clone(), (callee_chain, first_string_arg));
                            }
                        }
                    }
            }
    }

    // Capture full signatures of local functions (both `local function f()` and
    // `local f = function()`) keyed by name. Assigning a local function to a
    // namespace/class field (`ns.f = f`) then re-uses the captured signature so
    // the params/returns survive cross-file, instead of degrading to a bare
    // `function` type. Body-derived return types are NOT captured here — they
    // are resolved lazily by the per-file harvest at call time.
    let mut local_function_sigs: HashMap<String, ExternalGlobal> = HashMap::new();
    for stmt in &all_stmts {
        match stmt {
            Statement::FunctionDefinition(func) if func.is_local() => {
                if let Some(name) = func.name() {
                    local_function_sigs.insert(name, build_func_external(
                        func, func.syntax(), false, owned_path.as_deref(),
                    ));
                }
            }
            Statement::LocalAssign(assign) => {
                if let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list()) {
                    let names = name_list.names();
                    let exprs = expr_list.expressions();
                    if names.len() == 1 && exprs.len() == 1
                        && let Expression::Function(fd) = &exprs[0] {
                            local_function_sigs.insert(names[0].clone(), build_func_external(
                                fd, assign.syntax(), false, owned_path.as_deref(),
                            ));
                        }
                }
            }
            _ => {}
        }
    }

    // Named globals created as a side effect of calling functions annotated with
    // `@creates-global` (e.g. WoW's `CreateFrame(type, "Name")` creates `_G.Name`).
    // Annotation-driven so no specific function names are hard-coded here; the
    // spec map is sourced from the stub globals' `creates_global` field. Collected
    // first so they win initial registration in build_on_stubs over a same-named
    // assignment whose RHS type is less precise. See `scan_created_globals`.
    let mut globals = scan_created_globals(root, creates_global_specs, source_path);

    // Track field names assigned on the addon table in this file (e.g. ns.LibTSMApp = ...)
    // Used to gate 3-part chains so we don't inject fields onto unrelated external classes
    let mut addon_assigned_fields: HashSet<String> = HashSet::new();
    // Buffer methods defined on local tables (e.g. function Locale.GetTable())
    // so they can be emitted when the local table is assigned to the addon ns
    let mut local_table_methods: HashMap<String, Vec<ExternalGlobal>> = HashMap::new();
    // Map local table var name → addon field name (e.g. "Locale" → "Locale" from ns.Locale = Locale)
    let mut local_table_to_addon_field: HashMap<String, String> = HashMap::new();

    for stmt in &all_stmts {
        match stmt {
            Statement::FunctionDefinition(func) => {
                // Parser2 emits simple function names as bare Name tokens (no Identifier node).
                // Fall back to func.name() when identifier() returns None.
                let (mut names, is_colon_opt) = if let Some(ident) = func.identifier() {
                    (ident.names(), Some(ident.is_call_to_self()))
                } else if let Some(name) = func.name() {
                    (vec![name], Some(false))
                } else {
                    continue;
                };
                // Redirect _G.func to a top-level global (matches build_ir.rs behavior)
                if names.len() >= 2 && names[0] == "_G" && !local_vars.contains(&names[0]) {
                    names.remove(0);
                }
                let is_colon = is_colon_opt.unwrap_or(false);
                {
                    let base = build_func_external(
                        func, func.syntax(), is_colon, owned_path.as_deref(),
                    );
                    // Local functions are file-scoped, not cross-file globals
                    // (multi-name branch needs no check — Lua syntax forbids `local function a.b()`)
                    if names.len() == 1 && !func.is_local() {
                        globals.push(ExternalGlobal { name: names[0].clone(), ..base });
                    } else if names.len() >= 2 {
                        let root_name = &names[0];
                        let method_name = &names[names.len() - 1];
                        let intermediates: Vec<String> = names[1..names.len()-1].to_vec();
                        // Skip methods on locals that aren't class-typed or table constructors.
                        // Use offset-aware check: only skip if the function definition
                        // comes after the local declaration (same logic as single-name assignments).
                        let func_offset: u32 = func.syntax().text_range().start().into();
                        if local_vars.contains(root_name) && !class_vars.contains_key(root_name) && !local_tables.contains(root_name) && addon_ns_var.as_deref() != Some(root_name.as_str())
                            && first_local_offset.get(root_name).is_some_and(|&lo| func_offset >= lo)
                        {
                            continue;
                        }
                        // Buffer methods defined on local tables (any depth) for later
                        // flushing onto the addon namespace. At flush time the local name
                        // is rewritten to the addon field alias and prepended to the
                        // buffered intermediates, so `function Db.A:Foo()` + `ns.Db = Db`
                        // resolves as `ns.Db.A:Foo()`.
                        if local_tables.contains(root_name) && !class_vars.contains_key(root_name) && addon_ns_var.as_deref() != Some(root_name.as_str()) {
                            local_table_methods.entry(root_name.clone()).or_default().push(ExternalGlobal {
                                // name left empty; set when flushed onto the addon ns
                                kind: ExternalGlobalKind::Method(intermediates.clone(), method_name.clone(), is_colon),
                                ..base.clone()
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
                                ..base
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
                        // A field/index write through a parenthesized/prefix-expression
                        // base (e.g. `(A or B).field = v`) collapses `names()` to the
                        // trailing field/key name; it mutates a member of the evaluated
                        // prefix, not a bare global, so never register it as one
                        // (matches build_ir.rs's skip of the same shape).
                        if idents[0].has_prefix_expr_base() { continue; }
                        let mut names = idents[0].names();
                        // Redirect _G.field to a top-level global (matches build_ir.rs behavior)
                        let mut was_g_redirect = false;
                        if names.len() >= 2 && names[0] == "_G" && !local_vars.contains(&names[0]) {
                            names.remove(0);
                            was_g_redirect = true;
                        }
                        // A plain `value = ...` assignment to a name that is declared as a
                        // local *earlier in the file* is a reassignment of that local, not an
                        // implicit global creation — skip it so locals don't leak as phantom
                        // globals. A global assignment that *precedes* a later same-named
                        // `local X` (e.g. `X = 100` then `local X = X`, common in FrameXML
                        // money constants) is still a genuine global, so compare offsets.
                        // (An explicit `_G.value = ...` write still creates a global.)
                        if names.len() == 1 && !was_g_redirect {
                            let assign_offset: u32 = assign.syntax().text_range().start().into();
                            if first_local_offset.get(&names[0])
                                .is_some_and(|&local_off| assign_offset >= local_off)
                            {
                                continue;
                            }
                        }
                        if names.len() == 1 {
                            // Skip bracket-element writes on a bare name (`asdf[2] = 2`):
                            // these write to an element OF `asdf`, not to `asdf` itself, so
                            // they do NOT create the global `asdf` (indexing it reads it,
                            // erroring at runtime if nil). Registering it here would mask the
                            // `undefined-global` on the base read. Mirrors the `>= 2` branch's
                            // identical skip for `ns.field[123] = true`.
                            if idents[0].has_non_string_bracket_tail() { continue; }
                            let range = assign.syntax().text_range();
                            let effective = unwrap_logical_chain(exprs[0]);
                            // A bare global assigned a function literal
                            // (`X = function() ... end`) is a global function.
                            // Emit it as a real Function (with params/returns
                            // harvested from the literal) so it routes through
                            // the function-registration pass, which runs before
                            // the variable pass and therefore wins over a
                            // same-named `X = nil` — the FrameXML idiom
                            // `X = nil; do X = function() end end` that otherwise
                            // leaves the global typed `nil` (false `cannot-call`).
                            if let Expression::Function(fd) = &effective {
                                let base = build_func_external(fd, assign.syntax(), false, owned_path.as_deref());
                                globals.push(ExternalGlobal { name: names[0].clone(), ..base });
                                continue;
                            }
                            let (kind, string_value, number_value, mixin_parents) = if let Some(vk) = classify_literal_value_kind(&effective) {
                                let sv = if let Expression::Literal(lit) = &effective {
                                    lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                } else { None };
                                let nv = super::annotation_scanning::extract_number_from_expr(&effective);
                                (ExternalGlobalKind::Variable(vk), sv, nv, Vec::new())
                            } else { match &effective {
                                Expression::TableConstructor(_) => (ExternalGlobalKind::Table, None, None, Vec::new()),
                                // (`Expression::Function` is handled by the early branch above.)
                                Expression::Identifier(ident) => {
                                    let mut rhs_names = ident.names();
                                    if rhs_names.len() == 2 {
                                        let table_name = local_aliases.get(&rhs_names[0])
                                            .cloned().unwrap_or_else(|| rhs_names[0].clone());
                                        (ExternalGlobalKind::FieldRef(table_name, rhs_names[1].clone()), None, None, Vec::new())
                                    } else if rhs_names.len() >= 2 {
                                        // Multi-part reference (e.g. Enum.BagIndex.Backpack)
                                        if addon_ns_var.as_deref() == Some(rhs_names[0].as_str()) {
                                            rhs_names[0] = ADDON_NS_NAME.to_string();
                                        } else if let Some(cn) = class_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = cn.clone();
                                        } else if let Some(type_name) = local_type_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = type_name.clone();
                                        }
                                        (ExternalGlobalKind::Variable(FieldValueKind::FieldRef(rhs_names)), None, None, Vec::new())
                                    } else if rhs_names.len() == 1 {
                                        (ExternalGlobalKind::Variable(FieldValueKind::FieldRef(rhs_names)), None, None, Vec::new())
                                    } else {
                                        (ExternalGlobalKind::Variable(FieldValueKind::Unknown), None, None, Vec::new())
                                    }
                                }
                                Expression::FunctionCall(call) => {
                                    // Empty chain / no string-arg hint for a chained receiver
                                    // (`LibStub("X"):M()`) or anonymous callee, so neither can
                                    // mis-resolve as a same-named global (see scan_funcall_callee).
                                    let (callee_names, first_string_arg) = scan_funcall_callee(
                                        call, addon_ns_var.as_deref(), &class_vars, &local_type_vars,
                                    );
                                    let mp = extract_mixin_parents(call, &callee_names, &class_vars);
                                    (ExternalGlobalKind::Variable(FieldValueKind::FunctionCall(callee_names, first_string_arg)), None, None, mp)
                                }
                                Expression::BinaryExpression(bin) => {
                                    let vk = match bin.kind() {
                                        Operator::Concatenate => FieldValueKind::String(None),
                                        op if op.is_arithmetic() => FieldValueKind::Number(None),
                                        op if op.is_comparison() => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    };
                                    (ExternalGlobalKind::Variable(vk), None, None, Vec::new())
                                }
                                Expression::UnaryExpression(un) => {
                                    let vk = match un.kind() {
                                        Operator::ArrayLength | Operator::Subtract => FieldValueKind::Number(None),
                                        Operator::Not => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    };
                                    (ExternalGlobalKind::Variable(vk), None, None, Vec::new())
                                }
                                _ => (ExternalGlobalKind::Variable(FieldValueKind::Unknown), None, None, Vec::new()),
                            }};
                            // Extract @type or @class annotation for the variable
                            let annotations = extract_annotations(assign.syntax());
                            let returns: Vec<AnnotationType> = if let Some(class_name) = class_vars.get(&names[0]) {
                                vec![AnnotationType::Simple(class_name.clone())]
                            } else {
                                annotations.var_type.into_iter().collect()
                            };
                            let (ns, ne) = idents[0].syntax().children_with_tokens()
                                .find_map(|c| match c {
                                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => {
                                        let r = t.text_range();
                                        Some((u32::from(r.start()), u32::from(r.end())))
                                    }
                                    _ => None,
                                })
                                .unwrap_or((u32::from(range.start()), u32::from(range.end())));
                            globals.push(ExternalGlobal {
                                name: names[0].clone(), kind,
                                params: Vec::new(), returns, return_names: Vec::new(), return_descriptions: Vec::new(), overloads: Vec::new(),
                                doc: None, deprecated: false, nodiscard: false, constructor: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                                string_value, number_value,
                                is_override: false,
                                is_meta: false,
                                see: Vec::new(),
                                flavors: 0, flavor_guard: annotations.flavor_guard,
                                implicit_nil_return: false,
                                narrows_arg: None,
                                creates_global: None,
                                generates_events: None,
                                callback_event_arg: None,
                                requires: Vec::new(),
                                body_derived_returns: false,
                                deferred_call_type: false,
                                name_start: ns, name_end: ne,
                                mixin_parents,
                                returns_class_name: false,
                            });
                        } else if names.len() >= 2 {
                            // Skip bracket-element writes (e.g. `ns.field[123] = true`):
                            // these write to an element OF the table, not to the field itself.
                            if idents[0].has_non_string_bracket_tail() { continue; }
                            let root_name = &names[0];
                            let is_addon_root = addon_ns_var.as_deref() == Some(root_name.as_str());
                            // Skip field assignments on locals that aren't class-typed, table constructors, or @type-annotated.
                            // Use offset-aware check: only skip if the assignment comes after the local declaration.
                            let assign_offset: u32 = assign.syntax().text_range().start().into();
                            if local_vars.contains(root_name) && !class_vars.contains_key(root_name) && !local_tables.contains(root_name) && !local_type_vars.contains_key(root_name) && !is_addon_root
                                && first_local_offset.get(root_name).is_some_and(|&lo| assign_offset >= lo)
                            {
                                continue;
                            }
                            // Drop 3+ part chains for local non-class variables to avoid
                            // fabricating sub-tables on e.g. local Frame instances.
                            // Allow: addon-ns roots, local @class vars, and non-local
                            // (global) roots — build_on_stubs/mod.rs decides whether to
                            // create sub-tables based on class vs non-class status via
                            // `is_deep_class_global()`. This trades some extra entries
                            // (global class roots emit deep chains that are later skipped)
                            // for simpler filter logic here — the downstream guard is the
                            // single source of truth for class-global deep-path suppression.
                            if names.len() >= 3 && !is_addon_root && local_vars.contains(root_name) && !class_vars.contains_key(root_name)
                                && first_local_offset.get(root_name).is_some_and(|&lo| assign_offset >= lo)
                            { continue; }
                            let intermediates: Vec<String> = names[1..names.len()-1].to_vec();
                            let field_name = names[names.len()-1].clone();
                            let canonical_name = if is_addon_root {
                                ADDON_NS_NAME.to_string()
                            } else if let Some(class_name) = class_vars.get(root_name) {
                                class_name.clone()
                            } else if let Some(type_name) = local_type_vars.get(root_name) {
                                type_name.clone()
                            } else { root_name.clone() };
                            let annotations = extract_annotations(assign.syntax());
                            // Unwrap `and`/`or` chains so `ns.B = X and X.Y`
                            // infers from the effective operand (Y), not the whole chain.
                            let effective = unwrap_logical_chain(exprs[0]);
                            let value_kind = if let Some(vk) = classify_literal_value_kind(&effective) {
                                vk
                            } else { match &effective {
                                Expression::TableConstructor(tc) => FieldValueKind::Table(extract_table_field_kinds(tc)),
                                Expression::Function(_) => FieldValueKind::Function,
                                Expression::FunctionCall(call) => {
                                    // `scan_funcall_callee` drops the chain (and the defclass
                                    // string-arg hint) for a chained receiver
                                    // (`LibStub("X"):GetLocale(...)`) or an anonymous callee
                                    // (`(cond and F1 or F2)("Class")`), either of which would
                                    // otherwise mis-resolve as a same-named global / mis-type the
                                    // field to the string-named class. A genuine named-defclass
                                    // call (`DefineClass("X")`) keeps its hint; builder chains
                                    // rooted at a named object (`Schema:AddField("x"):...`) still
                                    // resolve via the callee chain through the fixpoint.
                                    let (callee_names, first_string_arg) = scan_funcall_callee(
                                        call, addon_ns_var.as_deref(), &class_vars, &local_type_vars,
                                    );
                                    FieldValueKind::FunctionCall(callee_names, first_string_arg)
                                }
                                Expression::Identifier(ident) => {
                                    let mut rhs_names = ident.names();
                                    if rhs_names.len() == 1 && local_functions.contains(&rhs_names[0]) {
                                        FieldValueKind::Function
                                    } else if rhs_names.len() == 1 && local_tables.contains(&rhs_names[0]) {
                                        FieldValueKind::Table(vec![])
                                    } else if rhs_names.len() == 1 {
                                        if let Some((callee_chain, first_string_arg)) = local_call_origins.get(&rhs_names[0]) {
                                            // Local was assigned from a function call whose return type
                                            // isn't known locally — forward the call origin so
                                            // build_on_stubs can resolve the return type cross-file.
                                            FieldValueKind::FunctionCall(callee_chain.clone(), first_string_arg.clone())
                                        } else {
                                            // Single-name reference (e.g. a local or global);
                                            // preserve so cross-file resolution can look it up
                                            // in scope0 symbols / stubs.
                                            FieldValueKind::FieldRef(rhs_names)
                                        }
                                    } else if rhs_names.len() >= 2 {
                                        // Canonicalize root for field references (e.g. Util.FRAME → Banking.Util.FRAME)
                                        if addon_ns_var.as_deref() == Some(rhs_names[0].as_str()) {
                                            rhs_names[0] = ADDON_NS_NAME.to_string();
                                        } else if let Some(cn) = class_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = cn.clone();
                                        } else if let Some(type_name) = local_type_vars.get(&rhs_names[0]) {
                                            rhs_names[0] = type_name.clone();
                                        }
                                        FieldValueKind::FieldRef(rhs_names)
                                    } else {
                                        FieldValueKind::Unknown
                                    }
                                }
                                Expression::BinaryExpression(bin) => {
                                    match bin.kind() {
                                        Operator::Concatenate => FieldValueKind::String(None),
                                        op if op.is_arithmetic() => FieldValueKind::Number(None),
                                        op if op.is_comparison() => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    }
                                }
                                Expression::UnaryExpression(un) => {
                                    match un.kind() {
                                        Operator::ArrayLength | Operator::Subtract => FieldValueKind::Number(None),
                                        Operator::Not => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    }
                                }
                                _ => FieldValueKind::Unknown,
                            }};
                            let returns = if let Some(ref var_type) = annotations.var_type {
                                vec![var_type.clone()]
                            } else if let Some(ref class_name) = annotations.class {
                                // @enum Name or @class Name above a field assignment
                                vec![AnnotationType::Simple(class_name.clone())]
                            } else if let Expression::TableConstructor(tc) = &effective {
                                extract_table_literal_annotation(tc)
                                    .map_or_else(Vec::new, |tl| vec![tl])
                            } else if let Expression::Identifier(ident) = &effective {
                                let rhs_names = ident.names();
                                if rhs_names.len() == 1 {
                                    if let Some(class_name) = class_vars.get(&rhs_names[0]) {
                                        vec![AnnotationType::Simple(class_name.clone())]
                                    } else if let Some(type_name) = local_type_vars.get(&rhs_names[0]) {
                                        vec![AnnotationType::Simple(type_name.clone())]
                                    } else if let Some(ret_type) = local_return_types.get(&rhs_names[0]) {
                                        vec![ret_type.clone()]
                                    } else { Vec::new() }
                                } else { Vec::new() }
                            } else { Vec::new() };
                            // When the RHS is a single local function with a captured
                            // signature, emit a method on the namespace/class so the full
                            // signature (params/returns/etc.) is preserved cross-file,
                            // matching `function ns.field(...)`. Otherwise the field would
                            // degrade to a bare `function` type.
                            if let Expression::Identifier(rhs_ident) = &effective {
                                let rhs_names = rhs_ident.names();
                                if rhs_names.len() == 1
                                    && let Some(base) = local_function_sigs.get(&rhs_names[0]) {
                                        // Merge assignment-level annotations (doc comment,
                                        // @flavor guard) that override the function-level
                                        // ones, so `--- @flavor retail\nns.f = impl` works.
                                        let mut merged = base.clone();
                                        if annotations.doc.is_some() { merged.doc = annotations.doc; }
                                        if annotations.flavor_guard != 0 { merged.flavor_guard = annotations.flavor_guard; }
                                        if annotations.deprecated { merged.deprecated = true; }
                                        let track = if is_addon_root && names.len() == 2 {
                                            Some(&mut addon_assigned_fields)
                                        } else { None };
                                        push_method_global(
                                            &mut globals, canonical_name, merged,
                                            &field_name, intermediates, implicit_protected_prefix, track,
                                        );
                                        continue;
                                    }
                            }
                            // When the RHS is a function literal, build its full signature
                            // directly so params/returns survive cross-file (same as when a
                            // local function is assigned by name above).
                            if let Expression::Function(func_lit) = &effective {
                                let base = build_func_external(
                                    func_lit, assign.syntax(), false, owned_path.as_deref(),
                                );
                                let track = if is_addon_root && names.len() == 2 {
                                    Some(&mut addon_assigned_fields)
                                } else { None };
                                push_method_global(
                                    &mut globals, canonical_name, base,
                                    &field_name, intermediates, implicit_protected_prefix, track,
                                );
                                continue;
                            }
                            let range = assign.syntax().text_range();
                            globals.push(ExternalGlobal {
                                name: canonical_name,
                                kind: ExternalGlobalKind::TableField(intermediates, field_name.clone(), value_kind),
                                params: Vec::new(), returns, return_names: Vec::new(), return_descriptions: Vec::new(), overloads: Vec::new(),
                                doc: annotations.doc, deprecated: false, nodiscard: false, constructor: false,
                                visibility: default_visibility_for_name(&field_name, implicit_protected_prefix), generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                                string_value: None, number_value: None,
                                is_override: false,
                                is_meta: false,
                                see: Vec::new(),
                                flavors: 0, flavor_guard: annotations.flavor_guard,
                                implicit_nil_return: false,
                                narrows_arg: None,
                                creates_global: None,
                                generates_events: None,
                                callback_event_arg: None,
                                requires: Vec::new(),
                                body_derived_returns: false,
                                deferred_call_type: false,
                                name_start: u32::from(range.start()),
                                name_end: u32::from(range.end()),
                                mixin_parents: Vec::new(),
                                returns_class_name: false,
                            });
                            // For depth-2 assignments on the addon ns, track the assigned field
                            // name so methods on buffered local tables can be flushed post-loop.
                            if is_addon_root && names.len() == 2 {
                                addon_assigned_fields.insert(field_name.clone());
                                if let Expression::Identifier(rhs_ident) = &effective {
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

    // Register the *existence* of namespace/class fields assigned only from
    // inside function bodies. The scan above walks only top-level + control-flow
    // statements (via `collect_statements_recursive`), because the coarse
    // cross-file scan cannot reliably *type* values produced inside functions
    // (locals, `Mixin`'d frames, chained builder results, etc.) — committing a
    // wrong concrete type there causes worse false positives than it fixes. But
    // a field assigned to the addon namespace / a `@class` table from within a
    // function (e.g. `ns.CurrentFont = GetFont()` in an event handler) is still
    // a real field; never registering it makes every *read* of it elsewhere
    // false-positive as `undefined-field`.
    //
    // So we emit an existence-only entry (`FieldValueKind::Unknown`, no type)
    // for each such field. The build's Unknown-field pass registers an
    // otherwise-absent field as `any` (the honest "unknown") with no annotation —
    // enough to suppress `undefined-field` on reads without fabricating a shape
    // (so no `field-type-mismatch` on the write, no spurious `undefined-field` on
    // its own sub-fields, and — unlike a bare `table` — no false `type-mismatch`
    // when the field's value is passed to a typed parameter, nor `cannot-call`
    // when it is invoked) — and *skips* fields that already carry a precise type
    // or methods, so this never clobbers a better definition. Multi-target
    // assignments (`a.x, a.y = f()`) are handled uniformly by iterating every
    // LHS target.
    // Names bound as locals (incl. params and for-vars) anywhere in the file —
    // used to tell a genuine implicit-global write nested in a function body from
    // a reassignment of a function-scoped local. Computed lazily on the first
    // candidate so files without any in-function/multi-target writes pay nothing.
    let mut all_local_names: Option<HashSet<String>> = None;
    // Per-file variable/last-segment → `@class` map, mirroring what the
    // typed/bare self-field scanners use to gate a colon method's receiver.
    // Built lazily only when a deeply-nested `self.field = ...` needs it.
    let mut self_field_var_to_class: Option<HashMap<String, String>> = None;
    for node in root.descendants() {
        let Some(Statement::Assign(assign)) = Statement::cast(node) else { continue };
        let Some(var_list) = assign.variable_list() else { continue };
        let idents = var_list.identifiers();
        // The main loop already precisely handles single-target, top-level (and
        // control-flow) writes. We only need to fill the gaps it leaves: writes
        // nested inside a function body, and multi-target assignments
        // (`a.x, a.y = ...`), which it does not scan at all.
        let in_function = node.ancestors().any(|a| a.kind() == SyntaxKind::FunctionDefinition);
        if !in_function && idents.len() < 2 { continue; }
        // Per-target RHS, so a field assigned a function literal can be registered
        // callable (`FieldValueKind::Function`) rather than existence-only `any` —
        // preserving the precise callable type for hover/completion.
        let rhs_exprs = assign.expression_list().map(|el| el.expressions()).unwrap_or_default();
        for (target_idx, ident) in idents.iter().enumerate() {
            // Skip bracket-element writes (`ns.field[123] = ...`): those write to
            // an element of the field, not to the field itself.
            if ident.has_non_string_bracket_tail() { continue; }
            // Skip field/index writes through a parenthesized/prefix-expression base
            // (`(A or B).field = ...`): they target a member of the evaluated prefix,
            // not a bare global, even though `names()` collapses to the trailing name.
            if ident.has_prefix_expr_base() { continue; }
            let mut names = ident.names();
            // Redirect `_G.field` to a top-level global (matches build_ir.rs).
            let mut was_g_redirect = false;
            if names.len() >= 2 && names[0] == "_G" && !local_vars.contains(&names[0]) {
                names.remove(0);
                was_g_redirect = true;
            }
            // Single-name write: a bare `X = ...` (or `_G.X = ...`). The main loop
            // only catches these at top level / control flow, so a global created
            // inside a function body (e.g. a saved-variable global initialized in an
            // event handler) is otherwise never registered, making every cross-file
            // *read* of it false-positive as `undefined-global`. Register an
            // existence-only entry (no type) so reads resolve, mirroring the
            // Unknown-field treatment below. A bare name that is a local *anywhere*
            // in the file is a local reassignment, not a global — skip it (an
            // explicit `_G.X = ...` write is always a global, so it bypasses the
            // local check).
            // Extract the last Name token range from the identifier for precise
            // go-to-definition navigation (highlights just the name, not the
            // whole assignment statement).
            let last_name_range = ident.syntax().descendants_with_tokens()
                .filter_map(|c| match c {
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text_range()),
                    _ => None,
                })
                .last();
            if names.len() == 1 {
                let name = &names[0];
                if !was_g_redirect {
                    let locals = all_local_names
                        .get_or_insert_with(|| collect_all_local_names(&root));
                    if locals.contains(name) { continue; }
                }
                let range = assign.syntax().text_range();
                let (ns, ne) = last_name_range
                    .map(|r| (u32::from(r.start()), u32::from(r.end())))
                    .unwrap_or((u32::from(range.start()), u32::from(range.end())));
                globals.push(ExternalGlobal {
                    name: name.clone(),
                    kind: ExternalGlobalKind::Variable(FieldValueKind::Unknown),
                    params: Vec::new(), returns: Vec::new(), return_names: Vec::new(), return_descriptions: Vec::new(), overloads: Vec::new(),
                    doc: None, deprecated: false, nodiscard: false, constructor: false,
                    visibility: Visibility::Public, generics: Vec::new(),
                    defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                    def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                    builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                    string_value: None, number_value: None,
                    is_override: false,
                    is_meta: false,
                    see: Vec::new(),
                    flavors: 0, flavor_guard: 0,
                    implicit_nil_return: false,
                    narrows_arg: None,
                    creates_global: None,
                    generates_events: None,
                    callback_event_arg: None,
                    requires: Vec::new(),
                    body_derived_returns: false,
                    deferred_call_type: false,
                    name_start: ns,
                    name_end: ne,
                    mixin_parents: Vec::new(),
                    returns_class_name: false,
                });
                continue;
            }
            if names.len() < 2 { continue; }
            let root_name = &names[0];
            let canonical_name = if addon_ns_var.as_deref() == Some(root_name.as_str()) {
                ADDON_NS_NAME.to_string()
            } else if let Some(class_name) = class_vars.get(root_name) {
                class_name.clone()
            } else if let Some(type_name) = local_type_vars.get(root_name) {
                type_name.clone()
            } else if root_name == "self" {
                // `self.field = ...` inside a colon method. Resolve `self` to the
                // enclosing colon method's receiver so the field is registered
                // (existence-only) on that table. This fills the gap left by the
                // typed/funcall/bare self-field scanners (annotation_scanning.rs),
                // which only fire when the receiver is a recognized local @class —
                // they miss mixin tables (`Foo = {}` promoted to a class only via an
                // XML `mixin=` / `Mixin()` reference), whose self-field writes would
                // otherwise read as `undefined-field` cross-file. Walk outward to the
                // nearest *colon method*, skipping any anonymous or named non-colon
                // wrapper functions in between — `self` closes over the enclosing
                // colon method's `self` through both (e.g.
                // `self:On("x", function() self.y = 1 end)` and
                // `function Helpers.run() self.y = 1 end` nested in a method).
                let Some(func_ident) = node.ancestors()
                    .filter_map(FunctionDefinition::cast)
                    .filter_map(|fd| fd.identifier())
                    .find(|id| id.is_call_to_self())
                else { continue };
                let func_names = func_ident.names();
                if func_names.len() < 2 { continue; }
                let receiver = receiver_name(&func_names).to_string();
                if addon_ns_var.as_deref() == Some(receiver.as_str()) {
                    // `self` is the addon namespace (typed via its `@class`).
                    // Register on the namespace table; `merge_addon_ns_into_classes`
                    // forwards it to the class table where reads resolve. The
                    // typed/funcall/bare self-field scanners miss this because
                    // `build_var_to_class` only maps single-name `@class` assigns,
                    // not the `local _, ns = ... ---@class Foo` namespace pattern.
                    ADDON_NS_NAME.to_string()
                } else if class_vars.contains_key(&receiver)
                    || local_type_vars.contains_key(&receiver)
                {
                    // Covered by the self-field scanners via ClassDecl.fields;
                    // emitting here would be a filtered no-op, so skip.
                    continue;
                } else {
                    // Mixin path. `self` is keyed by the receiver's single (leaf)
                    // name, so the field lands on whichever class/table later
                    // resolves to that name. That is only safe when the leaf
                    // unambiguously identifies the table `self` refers to:
                    //   • a *single-name* receiver (`function Mixin:M()`) is the
                    //     method's own root, so the leaf *is* that table — the
                    //     plain-global mixin pattern (`DataProviderMixin = {}`
                    //     promoted to a class only via an XML `mixin=` / `Mixin()`
                    //     reference), which is invisible to the typed/bare scanners
                    //     and the whole reason this path exists; or
                    //   • a *deeply-nested* receiver (`function A.B.C:M()`) whose
                    //     leaf names a recognized `@class` — matching the
                    //     typed/bare self-field scanners, which key cross-file
                    //     classes by their single name via `var_to_class`.
                    // A deeply-nested receiver whose leaf is *not* a known class
                    // would key by a bare name that can collide with an unrelated
                    // same-named global (the field would land on the wrong table),
                    // so skip it rather than misattribute. A function-scoped local
                    // can't be a cross-file mixin table either.
                    let locals = all_local_names.get_or_insert_with(|| collect_all_local_names(&root));
                    if locals.contains(&receiver) { continue; }
                    if func_names.len() == 2 {
                        receiver
                    } else if let Some(class_name) = self_field_var_to_class
                        .get_or_insert_with(|| build_var_to_class(&all_stmts))
                        .get(&receiver)
                        .cloned()
                    {
                        class_name
                    } else {
                        continue;
                    }
                }
            } else {
                // Not a known namespace/class root (e.g. a function-local table):
                // the coarse scan can't attribute the field, so skip.
                continue;
            };
            let intermediates: Vec<String> = names[1..names.len()-1].to_vec();
            let field_name = names[names.len()-1].clone();
            // A function-literal RHS registers callable so calls on the field
            // (`self.cb()`) don't false-positive as `cannot-call`. A *forwarded*
            // value — another field or a parameter (`ns.Foo = current.func`,
            // `ns.Cb = callback`), i.e. an `Identifier` RHS — may also hold a
            // callable, so on a *namespace/`@class` field* it registers
            // callable-or-unknown (`MaybeCallable`). Everything else (a function
            // *call* like `CreateFrame(...)`, a table constructor, a literal) stays
            // existence-only as `any`.
            //
            // Scoped to non-`self` roots: that is exactly the reported case
            // (namespace/`@class` fields, which commonly hold callbacks). A `self.`
            // field is excluded — those usually hold data, not callbacks, and they
            // also collect a per-file overlay / self-field-scanner type; forcing
            // them callable would union `function & table` with that real type and
            // turn clean reads into `type-mismatch`. Self-field function *literals*
            // are still made callable by the `Function` arm above.
            let root_is_self = root_name.as_str() == "self";
            let field_value_kind = match rhs_exprs.get(target_idx) {
                Some(Expression::Function(_)) => FieldValueKind::Function,
                Some(Expression::Identifier(_)) if !root_is_self => FieldValueKind::MaybeCallable,
                _ => FieldValueKind::Unknown,
            };
            let range = assign.syntax().text_range();
            let (ns, ne) = last_name_range
                .map(|r| (u32::from(r.start()), u32::from(r.end())))
                .unwrap_or((u32::from(range.start()), u32::from(range.end())));
            globals.push(ExternalGlobal {
                name: canonical_name,
                kind: ExternalGlobalKind::TableField(intermediates, field_name, field_value_kind),
                params: Vec::new(), returns: Vec::new(), return_names: Vec::new(), return_descriptions: Vec::new(), overloads: Vec::new(),
                doc: None, deprecated: false, nodiscard: false, constructor: false,
                visibility: Visibility::Public, generics: Vec::new(),
                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                string_value: None, number_value: None,
                is_override: false,
                is_meta: false,
                see: Vec::new(),
                flavors: 0, flavor_guard: 0,
                implicit_nil_return: false,
                narrows_arg: None,
                creates_global: None,
                generates_events: None,
                callback_event_arg: None,
                requires: Vec::new(),
                body_derived_returns: false,
                deferred_call_type: false,
                name_start: ns,
                name_end: ne,
                mixin_parents: Vec::new(),
                returns_class_name: false,
            });
        }
    }

    let addon_ns_class = addon_ns_var.as_ref()
        .and_then(|var| class_vars.get(var))
        .cloned();
    (globals, addon_ns_class)
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

/// The nested *receiver* call of `call`, if any — the call whose result `call` is
/// invoked on. Descends through grouping nodes (`(expr)`) so a parenthesized
/// receiver like `(getReg()):asType(...)` still reaches `getReg()`, and skips the
/// argument list so a call passed as an *argument* is never mistaken for the
/// receiver. Returns `None` when the receiver is a bare name / dotted path (the
/// chain's root) — or when there is no receiver (a plain `f(...)` call).
fn nested_receiver_call<'a>(call: &FunctionCall<'a>) -> Option<FunctionCall<'a>> {
    let ident = call.identifier()?;
    ident.syntax().children().find_map(|child| {
        if ExpressionList::cast(child).is_some() {
            return None; // argument list, not the receiver
        }
        FunctionCall::cast(child).or_else(|| child.descendants().find_map(FunctionCall::cast))
    })
}

/// Whether a (possibly chained) receiver is the "navigate to a class by name"
/// idiom: every call in the chain is a colon method-call taking a string-literal
/// first argument, bottoming out at a bare name
/// (`Base:From("Lib"):IncludeClassType("Class")`).
///
/// This is the signal that the defclass-local heuristic below may bind the local
/// to the class named by the outer string. It deliberately rejects instance
/// transforms — `getReg():asType("Class")` and `h:getReg():asType("Class")` —
/// where an intermediate call yields a runtime instance (no string key) and the
/// outer string is merely a parameter, not a class the local becomes. Classifying
/// by call *form* alone (`is_call_to_self`) can't separate those from navigation:
/// both are `name:m():m()`. Requiring a string-literal key on *every* hop is what
/// distinguishes name-navigation from instance access. The coarse scanner has no
/// callee `@return`/`@generic` type info, so this syntactic key check stands in for
/// "the method returns the class it was passed the name of".
fn chain_is_string_keyed_navigation(call: &FunctionCall<'_>) -> bool {
    let Some(ident) = call.identifier() else { return false };
    // This hop must be `:Method("name-literal")`.
    if !ident.is_call_to_self() || extract_first_string_arg(call).is_none() {
        return false;
    }
    match nested_receiver_call(call) {
        Some(nested) => chain_is_string_keyed_navigation(&nested),
        None => true, // reached the bare-name root
    }
}

/// Extract the first string literal argument from a plain (non-colon) function call.
/// For `DefineClass("MyComp")` returns `Some("MyComp")`.
/// Used alongside `extract_string_arg_from_call_chain` to populate `class_vars`
/// for locals assigned from factory-style function calls.
fn extract_first_string_arg(call: &FunctionCall<'_>) -> Option<String> {
    let arg_list = call.arguments()?;
    let args = arg_list.expressions();
    if let Some(Expression::Literal(lit)) = args.first()
        && let Some(s) = lit.get_string() {
            let name = s.trim_matches(|c| c == '"' || c == '\'').to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    None
}

/// Extract a non-empty string literal value from an expression, stripping quotes.
fn string_literal_value(expr: &Expression<'_>) -> Option<String> {
    if let Expression::Literal(lit) = expr {
        let s = lit.get_string()?.trim_matches(|c| c == '"' || c == '\'').to_string();
        if !s.is_empty() { return Some(s); }
    }
    None
}

/// Map of function name → [`CreatesGlobalSpec`]. Built from stub globals'
/// `creates_global` field via [`build_creates_global_map`] and threaded into the
/// global scanner so the set of "creates a named global" functions is
/// annotation-driven, not hard-coded.
pub type CreatesGlobalMap = HashMap<String, super::CreatesGlobalSpec>;

/// Build a [`CreatesGlobalMap`] from a slice of globals (typically the stub
/// globals), collecting those that carry a `@creates-global` annotation.
pub fn build_creates_global_map(globals: &[ExternalGlobal]) -> CreatesGlobalMap {
    globals.iter()
        .filter_map(|g| g.creates_global.as_ref().map(|cg| (g.name.clone(), cg.clone())))
        .collect()
}

/// Scan a file for calls to `@creates-global` functions and return the implicit
/// named globals they create as a side effect (e.g. WoW's
/// `CreateFrame(type, "Name")` creates `_G.Name`). Registering these eliminates
/// false `undefined-global` at read sites in other files (mirrors how
/// `xml_scan.rs` registers `name=` frames as `ExternalGlobal`). Returns an empty
/// vec when `specs` is empty (no `@creates-global` functions known).
pub fn scan_created_globals(
    root: SyntaxNode<'_>,
    specs: &CreatesGlobalMap,
    source_path: Option<&Path>,
) -> Vec<ExternalGlobal> {
    let mut out = Vec::new();
    if specs.is_empty() { return out; }
    let owned_path = source_path.map(|p| p.to_path_buf());
    // Walk every call in the file (not just top-level statements): a named global
    // is created wherever the call runs — inside an init function body, as a
    // nested call argument (`AceEvent:Embed(CreateFrame(...))`), etc.
    for node in root.descendants() {
        let Some(call) = FunctionCall::cast(node) else { continue };
        if let Some(name) = extract_created_global(&call, specs) {
            // The callee is always a plain global name (e.g. CreateFrame), so no
            // addon_ns / class_vars normalization is needed.
            let callee_names = call.identifier().map(|id| id.names()).unwrap_or_default();
            let range = call.syntax().text_range();
            // No explicit `returns`: the global is marked `deferred_call_type` so
            // its type is harvested from the creating call's *resolved* return
            // type (`def_start` locates the call in `source_path`). This yields the
            // full type — including any template/mixin intersection a `CreateFrame`
            // call produces — instead of a coarse annotation-reconstructed type.
            out.push(ExternalGlobal {
                name,
                kind: ExternalGlobalKind::Variable(FieldValueKind::FunctionCall(
                    callee_names,
                    None,
                )),
                params: Vec::new(),
                returns: Vec::new(),
                return_names: Vec::new(),
                return_descriptions: Vec::new(),
                overloads: Vec::new(),
                doc: None,
                deprecated: false,
                nodiscard: false,
                constructor: false,
                visibility: Visibility::Public,
                generics: Vec::new(),
                defclass: None,
                defclass_parent: None,
                source_path: owned_path.clone(),
                def_start: u32::from(range.start()),
                def_end: u32::from(range.end()),
                builds_field: None,
                built_name: None,
                built_extends: false,
                type_narrows: None,
                type_narrows_class: None,
                string_value: None,
                number_value: None,
                is_override: false,
                is_meta: false,
                see: Vec::new(),
                flavors: 0,
                flavor_guard: 0,
                implicit_nil_return: false,
                narrows_arg: None,
                creates_global: None,
                generates_events: None,
                callback_event_arg: None,
                requires: Vec::new(),
                body_derived_returns: false,
                deferred_call_type: true,
                name_start: u32::from(range.start()),
                name_end: u32::from(range.end()),
                mixin_parents: Vec::new(),
                returns_class_name: false,
            });
        }
    }
    out
}

/// If `call` invokes a `@creates-global` function (per `specs`) with a
/// string-literal at the name parameter, return the created global's name (e.g.
/// `CreateFrame("Button", "X")` → `Some("X")`). The global's *type* is not
/// derived here — it is harvested from the call's resolved return type.
fn extract_created_global(call: &FunctionCall<'_>, specs: &CreatesGlobalMap) -> Option<String> {
    let ident = call.identifier()?;
    if ident.is_call_to_self() { return None; }
    let names = ident.names();
    if names.len() != 1 { return None; }
    let spec = specs.get(&names[0])?;
    let args = call.arguments()?.expressions();
    // 1-based index into the argument list.
    string_literal_value(args.get(spec.name_param.checked_sub(1)?)?)
}

/// Extract named fields from a table constructor as a `TableLiteral` annotation.
/// Returns `None` if the table has no named fields.  Used by the global scanner
/// and bare self-field scanner to preserve table shape across files.
/// Positional and bracket-keyed entries are intentionally skipped — only
/// `Name = expr` fields map to `TableLiteral` field entries.
pub fn extract_table_literal_annotation(tc: &crate::ast::TableConstructor<'_>) -> Option<AnnotationType> {
    let mut fields = Vec::new();
    for field in tc.fields() {
        if let Some(crate::ast::FieldKind::Named { name, value }) = field.kind() {
            // Check for per-field ---@type annotation first (preceding-line or trailing)
            let field_type = if let Some(at) = super::annotation_scanning::extract_inline_type_from_node(field.syntax()) {
                at
            } else {
                match &value {
                    Expression::Literal(lit) => {
                        if lit.get_string().is_some() { AnnotationType::Simple("string".into()) }
                        else if lit.get_number().is_some() { AnnotationType::Simple("number".into()) }
                        else if lit.get_bool().is_some() { AnnotationType::Simple("boolean".into()) }
                        else if lit.is_nil() { AnnotationType::Simple("nil".into()) }
                        else { AnnotationType::Simple("any".into()) }
                    }
                    Expression::UnaryExpression(u) if matches!(u.kind(), crate::ast::Operator::Subtract) => {
                        if super::annotation_scanning::extract_number_from_expr(&value).is_some() {
                            AnnotationType::Simple("number".into())
                        } else {
                            AnnotationType::Simple("any".into())
                        }
                    }
                    Expression::TableConstructor(nested) => {
                        extract_table_literal_annotation(nested)
                            .unwrap_or_else(|| AnnotationType::Simple("table".into()))
                    }
                    Expression::Function(_) => AnnotationType::Simple("function".into()),
                    _ => AnnotationType::Simple("any".into()),
                }
            };
            fields.push((name, field_type));
        }
    }
    if fields.is_empty() { None } else { Some(AnnotationType::TableLiteral(fields)) }
}
