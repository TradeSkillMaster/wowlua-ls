use std::collections::{HashMap, HashSet};
use crate::ast::{AstNode, Block, Statement, Expression, FunctionCall};
use crate::syntax::SyntaxKind;
use crate::syntax::SyntaxNode;
use super::{
    AnnotationType, ClassDecl, DefclassFieldEntry, SelfFieldEntry, Visibility,
    default_visibility_for_name, extract_table_literal_fields,
};
use super::annotation_scanning::{
    ExternalGlobal, func_path,
    extract_type_annotation_for_assign, extract_inline_type_annotation,
    collect_statements_recursive,
};
use super::scan_built_name::build_built_name_map;

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
    /// All call-argument positions (0-based) where the backtick class-name string may appear.
    /// Includes the position from the primary signature plus positions derived from each
    /// `@overload` (with the implicit `self` param stripped so indices match call-site args).
    backtick_param_positions: Vec<usize>,
}

/// Pre-built lookup tables for defclass scanning, constructed once from all_globals/all_classes
/// and reused across multiple files.
pub struct DefclassContext {
    defclass_funcs: HashMap<String, DefclassFuncInfo>,
    constructor_names: HashSet<String>,
    /// func_path → return types for resolving function call RHS in constructors
    global_returns: HashMap<String, Vec<AnnotationType>>,
    /// func_path → param_index for @built-name extraction
    built_name_funcs: HashMap<String, usize>,
    /// class_name → field_name → field_type, for resolving ClassName._field:Method() patterns
    class_field_types: HashMap<String, HashMap<String, AnnotationType>>,
}

impl DefclassContext {
    pub fn new(all_globals: &[ExternalGlobal], all_classes: &[ClassDecl]) -> Self {
        // Build map of class name → index signature type from @field [string] Type
        let class_index_sigs: HashMap<&str, &AnnotationType> = all_classes.iter()
            .filter_map(|c| {
                c.fields.iter()
                    .find(|(name, _, _)| name == "[string]" || name == "[number]")
                    .map(|(_, typ, _)| (c.name.as_str(), typ))
            })
            .collect();

        let mut defclass_funcs: HashMap<String, DefclassFuncInfo> = HashMap::new();
        for g in all_globals.iter().filter(|g| g.defclass.is_some()) {
            let Some(fp) = func_path(g) else { continue };
            let defclass_name = g.defclass.as_ref().unwrap();
            let parents: Vec<String> = g.generics.iter()
                .filter(|(n, _)| n == defclass_name)
                .filter_map(|(_, c)| c.as_ref().map(|s| s.split('<').next().unwrap_or(s).to_string()))
                .collect();
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
            let values_param_idx = g.params.iter()
                .filter(|p| p.name != "...")
                .position(|p| matches!(&p.typ, AnnotationType::Simple(name) if name == defclass_name));
            let index_sig_type = parents.iter()
                .find_map(|p| class_index_sigs.get(p.as_str()).copied().cloned());
            // Collect all call-arg positions where the backtick class-name string may appear.
            // Primary signature: self is NOT in g.params, so index is direct.
            let mut backtick_seen = std::collections::BTreeSet::new();
            if let Some(pos) = g.params.iter()
                .filter(|p| p.name != "...")
                .position(|p| super::annotation_contains_backtick(&p.typ))
            {
                backtick_seen.insert(pos);
            }
            // Overloads: self IS the first param when present, so subtract 1.
            for ov in &g.overloads {
                let has_self = ov.params.first().map(|p| p.name == "self").unwrap_or(false);
                let skip = usize::from(has_self);
                if let Some(pos) = ov.params.iter()
                    .skip(skip)
                    .filter(|p| p.name != "...")
                    .position(|p| super::annotation_contains_backtick(&p.typ))
                {
                    backtick_seen.insert(pos);
                }
            }
            let backtick_param_positions: Vec<usize> = backtick_seen.into_iter().collect();
            defclass_funcs.insert(fp, DefclassFuncInfo {
                parents, parent_param_idx, values_param_idx, constraint_type_args,
                parent_generic_name, index_sig_type, backtick_param_positions,
            });
        }

        let mut constructor_names: HashSet<String> = HashSet::new();
        for class in all_classes {
            for cname in &class.constructor_methods {
                constructor_names.insert(cname.clone());
            }
        }

        // Only build global_returns when there are constructors to scan
        let global_returns = if constructor_names.is_empty() {
            HashMap::new()
        } else {
            let mut map: HashMap<String, Vec<AnnotationType>> = HashMap::new();
            for g in all_globals {
                let Some(path) = func_path(g) else { continue };
                if !g.returns.is_empty() {
                    map.insert(path, g.returns.clone());
                }
            }
            map
        };

        let built_name_funcs = build_built_name_map(all_globals);

        // Build class field type map from existing class declarations for resolving
        // ClassName._field:Method() patterns (e.g. BaseFrame._STATE_SCHEMA:Extend())
        let mut class_field_types: HashMap<String, HashMap<String, AnnotationType>> = HashMap::new();
        for class in all_classes {
            for (field_name, field_type, _) in &class.fields {
                if !matches!(field_type, AnnotationType::Simple(s) if s == "any") {
                    class_field_types.entry(class.name.clone())
                        .or_default()
                        .insert(field_name.clone(), field_type.clone());
                }
            }
        }

        Self { defclass_funcs, constructor_names, global_returns, built_name_funcs, class_field_types }
    }

    pub fn is_empty(&self) -> bool {
        self.defclass_funcs.is_empty()
    }
}

/// Scan for `local X = Y.func("ClassName")` calls where `Y.func` has `@defclass`.
/// Returns ClassDecl entries for discovered classes, with parent info from generic constraints.
/// `all_globals` should contain globals from ALL scanned files (not just this file).
pub fn scan_defclass_calls(root: SyntaxNode<'_>, all_globals: &[ExternalGlobal], all_classes: &[ClassDecl], implicit_protected_prefix: bool) -> Vec<ClassDecl> {
    let ctx = DefclassContext::new(all_globals, all_classes);
    scan_defclass_calls_with_context(root, &ctx, implicit_protected_prefix)
}

/// Like `scan_defclass_calls`, but uses a pre-built `DefclassContext` to avoid
/// rebuilding lookup tables from all_globals on every call. Use this when scanning
/// multiple files against the same set of globals.
pub fn scan_defclass_calls_with_context(root: SyntaxNode<'_>, ctx: &DefclassContext, implicit_protected_prefix: bool) -> Vec<ClassDecl> {
    let Some(block) = Block::cast(root) else { return Vec::new() };
    if ctx.defclass_funcs.is_empty() { return Vec::new(); }

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
            // Find the class-name string at any of the known backtick positions (primary or
            // overloads).  Positions are tried in ascending call-argument index order.
            let class_name_str = info.backtick_param_positions.iter().find_map(|&pos| {
                if let Some(Expression::Literal(lit)) = call_args.get(pos) {
                    lit.get_string()
                        .map(|s| s.trim_matches(|c: char| c == '"' || c == '\'').to_string())
                } else {
                    None
                }
            });
            if let Some(name) = class_name_str {
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

    let mut results: Vec<ClassDecl> = Vec::new();
    // Map local variable name → index in results (for matching constructor definitions)
    let mut var_to_result: HashMap<String, usize> = HashMap::new();
    let mut stmts = Vec::new();
    collect_statements_recursive(&block, &mut stmts);

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
                    if idents.len() == 1 {
                        let names = idents[0].names();
                        if names.len() == 1 { Some(names.into_iter().next().unwrap()) } else { None }
                    } else { None }
                });
                let call = a.expression_list().and_then(|el| {
                    let exprs = el.expressions();
                    if exprs.len() == 1 { if let Expression::FunctionCall(c) = &exprs[0] { Some(*c) } else { None } } else { None }
                });
                (call, var_name)
            }
            // Bare function-call statements: `addon:NewAddon("Name")` — no LHS variable.
            // @defclass still fires; the class is registered but not bound to a local.
            Statement::FunctionCall(call) => (Some(*call), None),
            _ => (None, None),
        };
        let Some(call) = rhs_call else { continue };

        if let Some(mut result) = find_defclass_in_chain(&call, &ctx.defclass_funcs) {
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
                implicit_protected_prefix: bool,
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
                        collect_nested_classes(&synthetic_name, entry.children, default_type, nested_classes, &mut sub_fields, &mut sub_field_ranges, implicit_protected_prefix);
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
                            is_key_enum: false,
                            correlated_groups: Vec::new(),
                            def_range: None,
                            def_path: None,
                            field_ranges: sub_field_ranges,
                            field_paths: HashMap::new(),
                            see: Vec::new(),
                            declared_field_names: HashSet::new(),
                            field_literals: HashMap::new(),
                            field_descriptions: HashMap::new(),
                        });
                        fields.push((entry.name.clone(), AnnotationType::Simple(synthetic_name), default_visibility_for_name(&entry.name, implicit_protected_prefix)));
                    } else {
                        fields.push((entry.name.clone(), default_type.clone(), default_visibility_for_name(&entry.name, implicit_protected_prefix)));
                    }
                }
            }
            collect_nested_classes(&result.name, result.table_literal_fields, &default_type, &mut nested_classes, &mut fields, &mut field_ranges, implicit_protected_prefix);
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
                is_key_enum: false,
                correlated_groups: Vec::new(),
                def_range: Some((u32::from(stmt_range.start()), u32::from(stmt_range.end()))),
                def_path: None,
                field_ranges,
                field_paths: HashMap::new(),
                see: Vec::new(),
                declared_field_names: HashSet::new(),
                field_literals: HashMap::new(),
                field_descriptions: HashMap::new(),
            });
        }
    }

    // Second pass: scan for constructor method definitions and extract self.X = ... fields
    if !results.is_empty() && !ctx.constructor_names.is_empty() {
        let global_returns = &ctx.global_returns;
        let built_name_funcs = &ctx.built_name_funcs;

        // Scan class-level field assignments (ClassName.field = expr) to build per-class field type maps.
        // This allows constructor scanning to resolve self._X:Method() by knowing _X's type.
        // Also tracks @built-name for fields whose RHS chain contains a @built-name call.
        // `class_field_types` tracks non-any typed fields for constructor method resolution.
        // `class_field_all` tracks ALL discovered fields (including any-typed) for cross-file visibility.
        let mut class_field_types: HashMap<usize, HashMap<String, AnnotationType>> = HashMap::new();
        let mut class_field_all: HashMap<usize, HashMap<String, AnnotationType>> = HashMap::new();
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
                        .unwrap_or_else(|| infer_type_from_expression(expr, global_returns, &HashMap::new(), &HashMap::new(), &ctx.class_field_types));
                    // Skip storing `any`-typed fields when the RHS is a self-referential
                    // method call (X.field = X.field:Method(...)). In this case the parent
                    // class likely has a better type that would be masked by `any`.
                    let is_self_ref_any = matches!(&field_type, AnnotationType::Simple(s) if s == "any")
                        && is_self_referential_call(expr, root_var, field_name);
                    if !is_self_ref_any {
                        // Always record for cross-file visibility
                        class_field_all.entry(result_idx)
                            .or_default()
                            .insert(field_name.clone(), field_type.clone());
                    }
                    // Only record non-any types for constructor method resolution
                    if !matches!(&field_type, AnnotationType::Simple(s) if s == "any") {
                        class_field_types.entry(result_idx)
                            .or_default()
                            .insert(field_name.clone(), field_type);
                    }
                    // Extract @built-name from the call chain if the RHS is a function call
                    if let Expression::FunctionCall(call) = expr
                        && let Some((built_name, _)) = extract_built_name_from_chain(call, built_name_funcs) {
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
            if let Some((built_name, _)) = extract_built_name_from_chain(call, built_name_funcs) {
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

        // Add class-level static fields to ClassDecl so they're visible cross-file.
        // Uses class_field_all (includes any-typed fields) so that fields whose RHS
        // is a local variable (unresolvable at scan time) still appear on the class.
        // Without this, `ClassName.FIELD = localVar` would be invisible cross-file,
        // causing false-positive `undefined-field` diagnostics.
        for (&result_idx, fields) in &class_field_all {
            let existing: HashSet<String> = results[result_idx].fields.iter()
                .map(|(name, _, _)| name.clone()).collect();
            // Sort by field name for deterministic output: `class_field_all` is a
            // HashMap whose iteration order varies run-to-run. Without this, the
            // pushed field order is non-deterministic, which makes `class_semantic_eq`
            // (an order-sensitive Vec comparison) report phantom changes on re-scan,
            // triggering unnecessary Full workspace rebuilds.
            let mut sorted: Vec<_> = fields.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            for (field_name, field_type) in sorted {
                if !existing.contains(field_name) {
                    results[result_idx].fields.push((
                        field_name.clone(),
                        field_type.clone(),
                        default_visibility_for_name(field_name, implicit_protected_prefix),
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
            if !ctx.constructor_names.contains(method_name) { continue; }
            let Some(&result_idx) = var_to_result.get(root_var) else { continue; };

            // Walk the constructor body for self.X = ... assignments
            if let Some(body) = func.block() {
                let existing_fields: HashSet<String> = results[result_idx].fields.iter()
                    .map(|(name, _, _)| name.clone()).collect();
                let field_types = class_field_types.get(&result_idx).cloned().unwrap_or_default();
                let field_built_names = class_field_built_names.get(&result_idx).cloned().unwrap_or_default();
                let ctor_fields = extract_self_fields(body, global_returns, &field_types, &field_built_names, &ctx.class_field_types);
                for entry in ctor_fields {
                    if !existing_fields.contains(&entry.name) {
                        let vis = default_visibility_for_name(&entry.name, implicit_protected_prefix);
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
fn extract_self_fields(block: Block<'_>, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>, class_field_types: &HashMap<String, HashMap<String, AnnotationType>>) -> Vec<SelfFieldEntry> {
    let mut fields = Vec::new();
    let mut seen = HashSet::new();
    let mut field_types = field_types.clone();
    extract_self_fields_inner(block, &mut fields, &mut seen, global_returns, &mut field_types, field_built_names, class_field_types);
    fields
}

/// Infer an `AnnotationType` from a constructor RHS expression.
fn infer_type_from_expression(expr: &Expression<'_>, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>, class_field_types: &HashMap<String, HashMap<String, AnnotationType>>) -> AnnotationType {
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
        Expression::UnaryExpression(u) if matches!(u.kind(), crate::ast::Operator::Subtract) => {
            if super::annotation_scanning::extract_number_from_expr(expr).is_some() {
                AnnotationType::Simple("number".to_string())
            } else {
                AnnotationType::Simple("any".to_string())
            }
        }
        Expression::TableConstructor(_) => AnnotationType::Simple("table".to_string()),
        Expression::Function(_) => AnnotationType::Simple("function".to_string()),
        Expression::FunctionCall(call) => {
            match resolve_funcall_return_type(call, global_returns, field_types, field_built_names, class_field_types) {
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

/// Check if an expression is a self-referential method call, i.e. `X.field:Method(...)`
/// where `X` matches `root_var` and `field` matches `field_name`. This handles both
/// direct calls (`X.field:Method()`) and chained calls (`X.field:Method():Other()`).
fn is_self_referential_call(expr: &Expression<'_>, root_var: &str, field_name: &str) -> bool {
    let Expression::FunctionCall(call) = expr else { return false };
    // Walk down chained calls iteratively to find the innermost identifier
    let mut current_ident = match call.identifier() {
        Some(id) => id,
        None => return false,
    };
    while let Some(nested) = current_ident.syntax().children().find_map(FunctionCall::cast) {
        match nested.identifier() {
            Some(id) => current_ident = id,
            None => return false,
        }
    }
    let names = current_ident.names();
    // Check if the root identifier matches X.field (e.g. BaseFrame._STATE_SCHEMA)
    names.len() >= 2 && names[0] == root_var && names[1] == field_name
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
/// `class_field_types` maps class_name → field_name → type for resolving
/// ClassName._field:Method() patterns across classes.
fn resolve_funcall_return_type(
    call: &FunctionCall<'_>,
    global_returns: &HashMap<String, Vec<AnnotationType>>,
    field_types: &HashMap<String, AnnotationType>,
    field_built_names: &HashMap<String, String>,
    class_field_types: &HashMap<String, HashMap<String, AnnotationType>>,
) -> Option<ResolvedReturn> {
    let ident = call.identifier()?;

    // Check for chained calls: the identifier contains a nested FunctionCall
    if let Some(nested_call) = ident.syntax().children().find_map(FunctionCall::cast) {
        // Resolve the inner call to get the receiver type
        let inner = resolve_funcall_return_type(&nested_call, global_returns, field_types, field_built_names, class_field_types)?;

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

    // Class-field method call: ClassName._field:Method() → names = ["ClassName", "_field", "Method"]
    // Look up the field's type on the class, then resolve the method on that type.
    // Only handles Simple and Parameterized("Name", _) field types; compound types
    // (unions, intersections, fun() shapes) are not resolved through this path.
    if names.len() == 3 {
        let class_name = &names[0];
        let field_name = &names[1];
        let method_name = &names[2];
        let field_class_name = match class_field_types.get(class_name.as_str())
            .and_then(|fields| fields.get(field_name.as_str()))
        {
            Some(AnnotationType::Simple(s)) => Some(s.as_str()),
            Some(AnnotationType::Parameterized(s, _)) => Some(s.as_str()),
            _ => None,
        };
        if let Some(field_class) = field_class_name {
            let method_path = format!("{}.{}", field_class, method_name);
            if let Some(returns) = global_returns.get(&method_path) {
                let chain_type = pick_effective_return(returns, Some(field_class))?;
                return Some(ResolvedReturn { chain_type, built_name: None });
            }
        }
    }

    let func_path = names.join(".");
    if let Some(returns) = global_returns.get(&func_path) {
        // For method calls (2+ names), the receiver is the name before the
        // method — names[names.len()-2], not names[0] — so deep chains like
        // Parent.Sub:Method() resolve `self` to "Sub", not "Parent".
        let receiver = if names.len() >= 2 { Some(names[names.len() - 2].as_str()) } else { None };
        let chain_type = pick_effective_return(returns, receiver)?;
        return Some(ResolvedReturn { chain_type, built_name: None });
    }

    None
}

fn extract_self_fields_inner(block: Block<'_>, fields: &mut Vec<SelfFieldEntry>, seen: &mut HashSet<String>, global_returns: &HashMap<String, Vec<AnnotationType>>, field_types: &mut HashMap<String, AnnotationType>, field_built_names: &HashMap<String, String>, class_field_types: &HashMap<String, HashMap<String, AnnotationType>>) {
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
                                            .map(|e| infer_type_from_expression(e, global_returns, field_types, field_built_names, class_field_types))
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
                        extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names, class_field_types);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names, class_field_types);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names, class_field_types);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names, class_field_types);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    extract_self_fields_inner(b, fields, seen, global_returns, field_types, field_built_names, class_field_types);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scan_defclass_calls;
    use crate::annotations::{AnnotationType, ClassDecl, ParamInfo};
    use crate::annotations::annotation_scanning::{ExternalGlobal, ExternalGlobalKind};
    use crate::syntax::SyntaxNode;

    fn make_defclass_global() -> ExternalGlobal {
        let mut g = ExternalGlobal::for_test("DefineClass", ExternalGlobalKind::Function);
        g.params = vec![ParamInfo {
            name: "name".into(),
            typ: AnnotationType::Backtick(Box::new(AnnotationType::Simple("T".into()))),
            optional: false,
            description: None,
        }];
        g.returns = vec![AnnotationType::Simple("T".into())];
        g.defclass = Some("T".to_string());
        g
    }

    // Regression: class-level static fields (ClassName.FIELD = expr) discovered via
    // a @defclass scan must come out in a deterministic (sorted) order. They are
    // collected from a HashMap whose iteration order varies between scans; without
    // sorting, `class_semantic_eq` (an order-sensitive Vec comparison) reports
    // phantom changes on re-scan, triggering needless Full workspace rebuilds.
    #[test]
    fn defclass_static_fields_are_sorted() {
        let globals = vec![make_defclass_global()];
        // A class with a constructor method so the second pass (which collects
        // class-level static fields) runs.
        let mut dummy = ClassDecl::for_test("Dummy");
        dummy.constructor_methods = vec!["__init".to_string()];
        let classes = vec![dummy];

        let src = "local Obj = DefineClass(\"MyClass\")\n\
                   Obj.ZEBRA = 1\n\
                   Obj.APPLE = 2\n\
                   Obj.MANGO = 3\n";
        let tree = crate::syntax::parser::parse(src);
        let root = SyntaxNode::new_root(&tree);
        let result = scan_defclass_calls(root, &globals, &classes, false);

        let my = result.iter().find(|c| c.name == "MyClass")
            .expect("should discover MyClass from defclass call");
        let names: Vec<&str> = my.fields.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(names, vec!["APPLE", "MANGO", "ZEBRA"],
            "class-level static fields must be in deterministic sorted order");
    }
}
