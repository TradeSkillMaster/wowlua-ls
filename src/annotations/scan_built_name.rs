use std::collections::HashSet;
use crate::ast::{AstNode, Block, Statement, Expression, FunctionCall};
use crate::syntax::SyntaxNode;
use super::{
    AnnotationType, ClassDecl, ParamInfo, Visibility,
    default_visibility_for_name,
};
use super::annotation_scanning::{
    ExternalGlobal, ExternalGlobalKind, func_path,
    collect_statements_recursive,
};

/// Scan a file for calls to functions with `@built-name`, extracting the class name
/// from the specified string literal argument. Returns empty `ClassDecl` entries so the
/// name is registered in `PreResolvedGlobals` for cross-file annotation resolution.
pub fn scan_built_name_calls(root: SyntaxNode<'_>, all_globals: &[ExternalGlobal], implicit_protected_prefix: bool) -> Vec<ClassDecl> {
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
        implicit_protected_prefix: bool,
    ) -> Vec<(String, AnnotationType, Visibility)> {
        let mut fields = Vec::new();
        collect_built_fields(call, schema_class, builds_field_funcs, &mut fields, implicit_protected_prefix);
        fields
    }

    fn collect_built_fields(
        call: &FunctionCall<'_>,
        schema_class: &str,
        builds_field_funcs: &HashMap<String, BuildsFieldInfo>,
        fields: &mut Vec<(String, AnnotationType, Visibility)>,
        implicit_protected_prefix: bool,
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
                            fields.push((field_name.clone(), field_type, default_visibility_for_name(&field_name, implicit_protected_prefix)));
                        }
                }
            }
        }

        // Recurse into nested FunctionCall in the identifier (inner chain call)
        if let Some(nested) = ident.syntax().children().find_map(FunctionCall::cast) {
            collect_built_fields(&nested, schema_class, builds_field_funcs, fields, implicit_protected_prefix);
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
                        // Also try identifier (variable reference) — the variable name
                        // may match a known class (e.g. `AddField("name", MyClass)`
                        // where MyClass is a local assigned from IncludeClassType)
                        if !subs.contains_key(gen_name)
                            && let Some(Expression::Identifier(ident)) = call_args.get(i) {
                                let names = ident.names();
                                if names.len() == 1 {
                                    subs.insert(gen_name.clone(), names[0].clone());
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
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);
    for stmt in &all_stmts {
        let rhs_call = match stmt {
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
                    .map(|sc| extract_built_fields_from_chain(&call, sc, &builds_field_funcs, implicit_protected_prefix))
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
                    is_key_enum: false,
                    correlated_groups: Vec::new(),
                    def_range: None,
                    def_path: None,
                    field_ranges: HashMap::new(),
                    field_paths: HashMap::new(),
                    see: Vec::new(),
                    declared_field_names: HashSet::new(),
                });
            }
    }
    results
}

#[cfg(test)]
mod tests {
    use crate::annotations::{AnnotationType, ParamInfo, Visibility};
    use crate::annotations::annotation_scanning::{ExternalGlobal, ExternalGlobalKind};
    use crate::syntax::SyntaxNode;
    use super::scan_built_name_calls;

    fn make_external_global(name: &str, kind: ExternalGlobalKind) -> ExternalGlobal {
        ExternalGlobal {
            name: name.to_string(),
            kind,
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
            implicit_nil_return: false,
            narrows_arg: None,
        }
    }

    fn parse_tree(text: &str) -> crate::syntax::tree::SyntaxTree {
        crate::syntax::parser::parse(text)
    }

    #[test]
    fn scan_built_name_detects_chain_method_change() {
        let mut create_method = make_external_global("Schema", ExternalGlobalKind::Method(Vec::new(), "Create".to_string(), true));
        create_method.built_name = Some(1);
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

        let tree_a = parse_tree(r#"local tbl = Schema:Create("MyState"):AddOptionalField("name")"#);
        let root_a = SyntaxNode::new_root(&tree_a);
        let result_a = scan_built_name_calls(root_a, &globals, false);

        let tree_b = parse_tree(r#"local tbl = Schema:Create("MyState"):AddRequiredField("name")"#);
        let root_b = SyntaxNode::new_root(&tree_b);
        let result_b = scan_built_name_calls(root_b, &globals, false);

        assert_eq!(result_a.len(), 1, "should discover MyState from chain A");
        assert_eq!(result_b.len(), 1, "should discover MyState from chain B");
        assert_eq!(result_a[0].name, "MyState");
        assert_eq!(result_b[0].name, "MyState");

        assert_ne!(result_a[0].fields, result_b[0].fields,
            "different builder methods must produce different ClassDecl fields");

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
