use std::collections::{HashMap, HashSet};
use std::path::Path;
use crate::ast::{AstNode, Block, Statement, Expression, FunctionCall, Operator};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use super::{
    AnnotationType, ParamInfo, Visibility,
    extract_annotations, default_visibility_for_name,
};
use super::annotation_types::{parse_overload, OverloadSig};
use super::annotation_scanning::{
    ADDON_NS_NAME, ExternalGlobal, ExternalGlobalKind, FieldValueKind,
    is_select_varargs, collect_statements_recursive,
};

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

// ── Synthesized return-only overloads (workspace scan) ──────────────────────

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
fn synthesize_return_only_overloads_for_body(body: &Block<'_>) -> Vec<OverloadSig> {
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

// ── Global declaration scanning ─────────────────────────────────────────────

pub fn scan_file_globals(root: SyntaxNode<'_>, source_path: Option<&Path>) -> Vec<ExternalGlobal> {
    scan_file_globals_with_synth(root, source_path, true, false).0
}

/// Variant of [`scan_file_globals`] that lets the caller disable workspace-level
/// synthesis of correlated return-only overloads for a specific file. The LSP /
/// CLI paths consult `inference.correlated_return_overloads` per-file; stub
/// generation leaves it on.
/// Returns `(globals, addon_ns_class_name)`.
/// `addon_ns_class_name` is `Some(class_name)` when the addon namespace variable
/// (the second value from `...`) also has a `@class` annotation, establishing a
/// relationship between the addon namespace table and a named class.
pub(crate) fn scan_file_globals_with_synth(
    root: SyntaxNode<'_>,
    source_path: Option<&Path>,
    correlated_return_overloads: bool,
    implicit_protected_prefix: bool,
) -> (Vec<ExternalGlobal>, Option<String>) {
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
                for name in &names {
                    local_vars.insert(name.clone());
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
                    } else if names.len() >= 2 && addon_ns_var.as_deref() == Some(names.last().unwrap().as_str()) {
                        class_vars.insert(names.last().unwrap().clone(), class_name);
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
                    // Defclass-style calls: `local X = Y:Init("ClassName")` or `local X = DefineClass("ClassName")`
                    if exprs.len() == 1
                        && let Expression::FunctionCall(call) = &exprs[0] {
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
                local_vars.insert(name.clone());
                local_functions.insert(name);
            }
        // Track @class annotations on global assignments (e.g. `---@class Foo\nMyMixin = {}`)
        // so that methods defined on the global are emitted under the class name.
        if let Statement::Assign(assign) = stmt
            && let (Some(var_list), Some(expr_list)) = (assign.variable_list(), assign.expression_list()) {
                let idents = var_list.identifiers();
                let exprs = expr_list.expressions();
                if idents.len() == 1 && exprs.len() == 1 {
                    let names = idents[0].names();
                    if names.len() == 1 && !local_vars.contains(&names[0]) {
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
                            params, returns: annotations.returns, return_names: annotations.return_names, overloads,
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
                        // Skip methods on locals that aren't class-typed or table constructors
                        if local_vars.contains(root_name) && !class_vars.contains_key(root_name) && !local_tables.contains(root_name) && addon_ns_var.as_deref() != Some(root_name.as_str()) {
                            continue;
                        }
                        // Buffer methods defined on local tables (any depth) for later
                        // flushing onto the addon namespace. At flush time the local name
                        // is rewritten to the addon field alias and prepended to the
                        // buffered intermediates, so `function Db.A:Foo()` + `ns.Db = Db`
                        // resolves as `ns.Db.A:Foo()`.
                        if local_tables.contains(root_name) && !class_vars.contains_key(root_name) && addon_ns_var.as_deref() != Some(root_name.as_str()) {
                            local_table_methods.entry(root_name.clone()).or_default().push(ExternalGlobal {
                                name: String::new(), // placeholder, set when flushed
                                kind: ExternalGlobalKind::Method(intermediates.clone(), method_name.clone(), is_colon),
                                params, returns: annotations.returns, return_names: annotations.return_names, overloads,
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
                                params, returns: annotations.returns, return_names: annotations.return_names, overloads,
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
                        let mut names = idents[0].names();
                        // Redirect _G.field to a top-level global (matches build_ir.rs behavior)
                        if names.len() >= 2 && names[0] == "_G" && !local_vars.contains(&names[0]) {
                            names.remove(0);
                        }
                        if names.len() == 1 {
                            let range = assign.syntax().text_range();
                            let effective = unwrap_logical_chain(exprs[0]);
                            let (kind, string_value, number_value) = match &effective {
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
                                Expression::BinaryExpression(bin) => {
                                    let vk = match bin.kind() {
                                        Operator::Concatenate => FieldValueKind::String,
                                        op if op.is_arithmetic() => FieldValueKind::Number,
                                        op if op.is_comparison() => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    };
                                    (ExternalGlobalKind::Variable(vk), None, None)
                                }
                                Expression::UnaryExpression(un) => {
                                    let vk = match un.kind() {
                                        Operator::ArrayLength | Operator::Subtract => FieldValueKind::Number,
                                        Operator::Not => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    };
                                    (ExternalGlobalKind::Variable(vk), None, None)
                                }
                                _ => (ExternalGlobalKind::Variable(FieldValueKind::Unknown), None, None),
                            };
                            // Extract @type or @class annotation for the variable
                            let annotations = extract_annotations(assign.syntax());
                            let returns: Vec<AnnotationType> = if let Some(class_name) = class_vars.get(&names[0]) {
                                vec![AnnotationType::Simple(class_name.clone())]
                            } else {
                                annotations.var_type.into_iter().collect()
                            };
                            globals.push(ExternalGlobal {
                                name: names[0].clone(), kind,
                                params: Vec::new(), returns, return_names: Vec::new(), overloads: Vec::new(),
                                doc: None, deprecated: false, nodiscard: false, constructor: false,
                                visibility: Visibility::Public, generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                                string_value, number_value,
                                is_override: false,
                                see: Vec::new(),
                                flavors: 0, flavor_guard: annotations.flavor_guard,
                            });
                        } else if names.len() >= 2 {
                            let root_name = &names[0];
                            let is_addon_root = addon_ns_var.as_deref() == Some(root_name.as_str());
                            // Skip field assignments on locals that aren't class-typed, table constructors, or @type-annotated
                            if local_vars.contains(root_name) && !class_vars.contains_key(root_name) && !local_tables.contains(root_name) && !local_type_vars.contains_key(root_name) && !is_addon_root {
                                continue;
                            }
                            // Only emit chains of 3+ parts when rooted at the addon namespace
                            // or a local @class variable. Non-addon/non-class deep writes (e.g.
                            // `FrameClass.Inner.x = 1`) are dropped to avoid fabricating
                            // sub-tables on unrelated external classes. Global @class variables
                            // are excluded to prevent deep chain fabrication on them.
                            if names.len() >= 3 && !is_addon_root && !(local_vars.contains(root_name) && class_vars.contains_key(root_name)) { continue; }
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
                            let value_kind = match &effective {
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
                                            } else if let Some(type_name) = local_type_vars.get(&callee_names[0]) {
                                                callee_names[0] = type_name.clone();
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
                                    if rhs_names.len() == 1 && local_functions.contains(&rhs_names[0]) {
                                        FieldValueKind::Function
                                    } else if rhs_names.len() == 1 && local_tables.contains(&rhs_names[0]) {
                                        FieldValueKind::Table
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
                                    } else if rhs_names.len() == 1 {
                                        // Single-name global reference (e.g. `debugstack`).
                                        // Preserve the name so cross-file resolution can
                                        // look it up in scope0 symbols / stubs.
                                        FieldValueKind::FieldRef(rhs_names)
                                    } else {
                                        FieldValueKind::Unknown
                                    }
                                }
                                Expression::BinaryExpression(bin) => {
                                    match bin.kind() {
                                        Operator::Concatenate => FieldValueKind::String,
                                        op if op.is_arithmetic() => FieldValueKind::Number,
                                        op if op.is_comparison() => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    }
                                }
                                Expression::UnaryExpression(un) => {
                                    match un.kind() {
                                        Operator::ArrayLength | Operator::Subtract => FieldValueKind::Number,
                                        Operator::Not => FieldValueKind::Boolean,
                                        _ => FieldValueKind::Unknown,
                                    }
                                }
                                _ => FieldValueKind::Unknown,
                            };
                            let returns = if let Some(ref var_type) = annotations.var_type {
                                vec![var_type.clone()]
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
                            let range = assign.syntax().text_range();
                            globals.push(ExternalGlobal {
                                name: canonical_name,
                                kind: ExternalGlobalKind::TableField(intermediates, field_name.clone(), value_kind),
                                params: Vec::new(), returns, return_names: Vec::new(), overloads: Vec::new(),
                                doc: annotations.doc, deprecated: false, nodiscard: false, constructor: false,
                                visibility: default_visibility_for_name(&field_name, implicit_protected_prefix), generics: Vec::new(),
                                defclass: None, defclass_parent: None, source_path: owned_path.clone(),
                                def_start: u32::from(range.start()), def_end: u32::from(range.end()),
                                builds_field: None, built_name: None, built_extends: false, type_narrows: None, type_narrows_class: None,
                                string_value: None, number_value: None,
                                is_override: false,
                                see: Vec::new(),
                                flavors: 0, flavor_guard: annotations.flavor_guard,
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
