//! Documentation data model and generation from `PreResolvedGlobals`.
//!
//! Builds a `Vec<DocNamespace>` describing all workspace-defined classes,
//! their fields, methods, parameters, and return types. The output is
//! consumed by `doc_gen_md` to produce VitePress-compatible markdown.

use std::path::Path;

use crate::annotations::{self, AnnotationType, Visibility};
use crate::pre_globals::PreResolvedGlobals;
use crate::types::*;

// ── Data model ──────────────────────────────────────────────────────────────

/// A top-level namespace representing a single `@class`.
pub(crate) struct DocNamespace {
    pub name: String,
    pub defines: Vec<DocDefine>,
    pub fields: Vec<DocField>,
}

/// A class definition within a namespace.
pub(crate) struct DocDefine {
    pub extends: Vec<DocBaseClass>,
    pub desc: Option<String>,
}

/// A parent class reference in a class definition.
pub(crate) struct DocBaseClass {
    pub view: String,
}

/// A field or method within a namespace.
pub(crate) struct DocField {
    pub name: String,
    pub kind: DocFieldKind,
    pub extends: Option<DocFieldExtends>,
    pub view: Option<String>,
    pub deprecated: bool,
    pub visible: Option<String>,
    pub desc: Option<String>,
}

/// Discriminator for field types.
pub(crate) enum DocFieldKind {
    /// Data field (non-function).
    DataField,
    /// Colon-syntax method with implicit self.
    Method,
    /// Dot-syntax function.
    Function,
}

/// Function signature info attached to a field.
pub(crate) struct DocFieldExtends {
    pub args: Vec<DocParam>,
    pub returns: Vec<DocParam>,
}

/// A single parameter or return value.
pub(crate) struct DocParam {
    pub name: Option<String>,
    pub view: String,
    pub desc: Option<String>,
}

// ── Stub dumping (for diffable output) ───────────────────────────────────────

/// Dump every global name from precomputed stubs with its resolved type.
/// Returns sorted `(name, type_string)` pairs for deterministic diffing.
pub fn dump_stub_globals(pg: &PreResolvedGlobals) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for maps in [&pg.scope0_symbols, &pg.framexml_scope0_symbols] {
        for (sym_id, sym_idx) in maps {
            let SymbolIdentifier::Name(name) = sym_id else { continue };
            let Some(local_idx) = sym_idx.0.checked_sub(EXT_BASE) else { continue };
            let sym = &pg.symbols[local_idx];
            let type_str = match sym.versions.last().and_then(|v| v.resolved_type.as_ref()) {
                Some(vt) => format_value_type(vt, pg),
                None => "?".to_string(),
            };
            entries.push((name.clone(), type_str));
        }
    }
    entries.sort();
    entries
}

// ── Type formatting (standalone, operates on PreResolvedGlobals) ─────────────

/// Format a `ValueType` to a display string using `PreResolvedGlobals` for lookups.
fn format_value_type(vt: &ValueType, pg: &PreResolvedGlobals) -> String {
    match vt {
        ValueType::Any => "any".to_string(),
        ValueType::Nil => "nil".to_string(),
        ValueType::Boolean(Some(true)) => "true".to_string(),
        ValueType::Boolean(Some(false)) => "false".to_string(),
        ValueType::Boolean(None) => "boolean".to_string(),
        ValueType::Number => "number".to_string(),
        ValueType::NumberLiteral(val) => val.clone(),
        ValueType::String(Some(val)) => format!("\"{}\"", val),
        ValueType::String(None) => "string".to_string(),
        ValueType::Userdata => "userdata".to_string(),
        ValueType::Thread => "thread".to_string(),
        ValueType::TypeVariable(name) => name.clone(),
        ValueType::OpaqueAlias(name, _) => name.clone(),
        ValueType::Function(Some(func_idx)) => format_function_type(*func_idx, pg),
        ValueType::Function(None) => "function".to_string(),
        ValueType::Table(Some(table_idx)) => {
            let table = ext_table(pg, *table_idx);
            if let Some(ref name) = table.class_name {
                name.clone()
            } else if let Some(ref val_vt) = table.value_type {
                if table.is_explicit_map {
                    let key_str = table.key_type.as_ref()
                        .map(|k| format_value_type(k, pg))
                        .unwrap_or_else(|| "any".to_string());
                    format!("table<{}, {}>", key_str, format_value_type(val_vt, pg))
                } else {
                    format!("{}[]", format_value_type(val_vt, pg))
                }
            } else {
                "table".to_string()
            }
        }
        ValueType::Table(None) => "table".to_string(),
        ValueType::Union(parts) => {
            // Special-case T|nil → T?
            if parts.len() == 2
                && parts.iter().any(|p| matches!(p, ValueType::Nil))
                && parts.iter().any(|p| !matches!(p, ValueType::Nil))
            {
                let other = parts.iter().find(|p| !matches!(p, ValueType::Nil)).unwrap();
                let formatted = format_value_type(other, pg);
                if matches!(other, ValueType::Function(..)) {
                    format!("({})?", formatted)
                } else {
                    format!("{}?", formatted)
                }
            } else {
                parts.iter()
                    .map(|p| format_value_type(p, pg))
                    .collect::<Vec<_>>()
                    .join(" | ")
            }
        }
        ValueType::Intersection(parts) => {
            parts.iter()
                .map(|p| format_value_type(p, pg))
                .collect::<Vec<_>>()
                .join(" & ")
        }
    }
}

/// Format a function type as `fun(params): returns`.
fn format_function_type(func_idx: FunctionIndex, pg: &PreResolvedGlobals) -> String {
    let func = ext_func(pg, func_idx);
    let args = format_function_params(func, pg);
    let rets = format_function_returns(func, pg);
    if rets.is_empty() {
        format!("fun({})", args.join(", "))
    } else {
        format!("fun({}):{}", args.join(", "), join_returns(&rets))
    }
}

/// Join return values, wrapping any element that contains a top-level comma
/// (i.e. a nested multi-return function type) in parens to disambiguate.
fn join_returns(rets: &[String]) -> String {
    if rets.len() <= 1 {
        return rets.join(", ");
    }
    rets.iter()
        .map(|r| {
            if has_top_level_comma(r) { format!("({r})") } else { r.clone() }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn has_top_level_comma(s: &str) -> bool {
    let mut depth = 0i32;
    for b in s.bytes() {
        match b {
            b'(' | b'<' | b'[' | b'{' => depth += 1,
            b')' | b'>' | b']' | b'}' => { depth = (depth - 1).max(0); }
            b',' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

/// Format function parameter list as strings.
fn format_function_params(func: &Function, pg: &PreResolvedGlobals) -> Vec<String> {
    let mut result: Vec<String> = func.args.iter().enumerate().map(|(i, &sym_idx)| {
        let name = match &ext_sym(pg, sym_idx).id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => "?".to_string(),
        };
        let optional = func.param_optional.get(i).copied().unwrap_or(false);
        let ann_has_nil = func.param_annotations.get(i)
            .is_some_and(annotations::annotation_type_is_nullable);
        let suffix = if optional && !ann_has_nil { "?" } else { "" };
        let type_str = param_annotation_text(func, i)
            .or_else(|| {
                ext_sym(pg, sym_idx).versions.first()
                    .and_then(|v| v.resolved_type.as_ref())
                    .map(|rt| {
                        let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                        format_value_type(&display_type, pg)
                    })
            });
        match type_str {
            Some(t) => format!("{}{}: {}", name, suffix, t),
            None => format!("{}{}", name, suffix),
        }
    }).collect();
    if func.is_vararg {
        let vararg_str = match &func.vararg_annotation {
            Some(ann) => format!("...: {}", annotations::format_annotation_type(ann)),
            None => "...".to_string(),
        };
        result.push(vararg_str);
    }
    result
}

/// Format function return types as strings.
fn format_function_returns(func: &Function, pg: &PreResolvedGlobals) -> Vec<String> {
    if func.returns_self {
        return vec!["self".to_string()];
    }
    if !func.return_annotations.is_empty() {
        return func.return_annotations.iter().enumerate().map(|(i, vt)| {
            let formatted = format_value_type(vt, pg);
            if func.has_vararg_return && i + 1 == func.return_annotations.len() {
                format!("...{}", formatted)
            } else {
                formatted
            }
        }).collect();
    }
    Vec::new()
}

/// Get the annotation text for a function parameter (preserving alias names).
fn param_annotation_text(func: &Function, i: usize) -> Option<String> {
    func.param_annotations.get(i).and_then(|ann| {
        if matches!(ann, AnnotationType::Simple(s) if s == "any") {
            return None;
        }
        Some(annotations::format_annotation_type(ann))
    })
}

/// Format an annotation type for doc params, wrapping backtick generics in `<>`.
fn format_annotation_for_doc_param(ann: &AnnotationType) -> String {
    if let AnnotationType::Backtick(inner) = ann {
        let inner_str = annotations::format_annotation_type(inner);
        format!("<{}>", inner_str)
    } else {
        annotations::format_annotation_type(ann)
    }
}

/// Format a ValueType for docs, wrapping type variables in `<>` with constraints.
fn format_value_type_for_doc(vt: &ValueType, pg: &PreResolvedGlobals, generics: &[(String, Option<ValueType>)]) -> String {
    if let ValueType::TypeVariable(name) = vt {
        let constraint = generics.iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, c)| c.as_ref());
        match constraint {
            Some(c) => format!("<{}:{}>", name, format_value_type(c, pg)),
            None => format!("<{}>", name),
        }
    } else {
        format_value_type(vt, pg)
    }
}

/// Look up a symbol in PreResolvedGlobals (adjusting for EXT_BASE).
fn ext_sym(pg: &PreResolvedGlobals, idx: SymbolIndex) -> &Symbol {
    &pg.symbols[idx.0 - EXT_BASE]
}

/// Look up a function in PreResolvedGlobals (adjusting for EXT_BASE).
fn ext_func(pg: &PreResolvedGlobals, idx: FunctionIndex) -> &Function {
    &pg.functions[idx.0 - EXT_BASE]
}

/// Look up a table in PreResolvedGlobals (adjusting for EXT_BASE).
fn ext_table(pg: &PreResolvedGlobals, idx: TableIndex) -> &TableInfo {
    &pg.tables[idx.0 - EXT_BASE]
}

/// Resolve the function index for a field: checks annotation first, then the
/// field's expression (which is `Expr::FunctionDef(idx)` for method fields).
fn resolve_field_func_idx(field: &FieldInfo, pg: &PreResolvedGlobals) -> Option<FunctionIndex> {
    if let Some(ValueType::Function(Some(idx))) = &field.annotation {
        return Some(*idx);
    }
    if field.expr.0 >= EXT_BASE
        && let Expr::FunctionDef(idx) = &pg.exprs[field.expr.0 - EXT_BASE] {
            return Some(*idx);
        }
    None
}

// ── Visibility formatting ───────────────────────────────────────────────────

fn visibility_str(vis: Visibility) -> Option<String> {
    match vis {
        Visibility::Public => Some("public".to_string()),
        Visibility::Private => Some("private".to_string()),
        Visibility::Protected => Some("protected".to_string()),
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Generate markdown API documentation for all workspace-defined classes.
///
/// Scans `PreResolvedGlobals` for classes within `project_root`, then writes
/// VitePress-compatible `.md` files to `out_dir`.
pub fn generate_markdown_docs(
    pg: &PreResolvedGlobals,
    project_root: &Path,
    out_dir: &Path,
    class_filter: Option<&[String]>,
) -> std::io::Result<()> {
    let namespaces = generate_docs(pg, project_root, class_filter);
    crate::doc_gen_md::generate_markdown_docs(&namespaces, out_dir)
}

// ── Core doc generation ─────────────────────────────────────────────────────

/// Generate documentation data for all workspace-defined classes.
///
/// Filters to classes whose source file is within `project_root`, excluding
/// WoW API stubs and other external definitions.
pub(crate) fn generate_docs(pg: &PreResolvedGlobals, project_root: &Path, class_filter: Option<&[String]>) -> Vec<DocNamespace> {
    let mut namespaces = Vec::new();

    // When --class is given, include exactly those classes (even stubs).
    // Otherwise, include workspace-defined classes within project_root.
    let mut class_entries: Vec<(&String, &TableIndex)> = pg.classes.iter()
        .filter(|(name, _)| {
            if let Some(filter) = class_filter {
                return filter.iter().any(|f| f == *name);
            }
            pg.class_locations.get(*name)
                .is_some_and(|loc| loc.path.starts_with(project_root))
        })
        .collect();
    class_entries.sort_by_key(|(name, _)| name.as_str());

    for &(class_name, table_idx) in &class_entries {
        let table = ext_table(pg, *table_idx);
        let ns = build_class_namespace(class_name, table, *table_idx, pg);
        namespaces.push(ns);
    }

    namespaces
}

/// Build a DocNamespace for a single class.
fn build_class_namespace(
    class_name: &str,
    table: &TableInfo,
    table_idx: TableIndex,
    pg: &PreResolvedGlobals,
) -> DocNamespace {
    let parent_views: Vec<DocBaseClass> = table.parent_classes.iter()
        .filter_map(|&parent_idx| {
            let parent_table = ext_table(pg, parent_idx);
            parent_table.class_name.as_ref().map(|name| DocBaseClass { view: name.clone() })
        })
        .collect();

    // Gather description from @see entries on the class
    let desc = if table.see.is_empty() {
        None
    } else {
        Some(table.see.iter().map(|s| format!("@see {}", s)).collect::<Vec<_>>().join("\n"))
    };

    let define = DocDefine {
        extends: parent_views,
        desc,
    };

    // Build fields
    let field_locs = pg.field_locations.get(&table_idx);
    let mut fields: Vec<DocField> = Vec::new();
    let mut field_names: Vec<&String> = table.fields.keys().collect();
    field_names.sort();

    for field_name in field_names {
        let field_info = &table.fields[field_name];
        // Skip private fields from doc output
        if field_info.visibility == Visibility::Private {
            continue;
        }
        // Skip undocumented data fields (runtime-discovered without @field annotation).
        // Functions/methods are always included since they're explicitly defined.
        // Check declared_class_fields to distinguish explicit @field annotations from
        // inferred constructor self-fields (both set annotation/annotation_type_raw).
        if resolve_field_func_idx(field_info, pg).is_none() {
            let is_declared = pg.declared_class_fields
                .get(class_name)
                .is_some_and(|s| s.contains(field_name.as_str()));
            if !is_declared {
                continue;
            }
        }
        let _field_loc = field_locs.and_then(|m| m.get(field_name));
        let doc_field = build_doc_field(field_name, class_name, field_info, pg);
        fields.push(doc_field);
    }

    DocNamespace {
        name: class_name.to_string(),
        defines: vec![define],
        fields,
    }
}

/// Build a DocField for a single class field.
fn build_doc_field(
    name: &str,
    class_name: &str,
    field: &FieldInfo,
    pg: &PreResolvedGlobals,
) -> DocField {
    let visible = visibility_str(field.visibility);

    // Determine if this is a function field
    if let Some(func_idx) = resolve_field_func_idx(field, pg) {
        let func = ext_func(pg, func_idx);
        let is_method = func.args.first().is_some_and(|&sym_idx| {
            matches!(&ext_sym(pg, sym_idx).id, SymbolIdentifier::Name(n) if n == "self")
        });
        let kind = if is_method { DocFieldKind::Method } else { DocFieldKind::Function };

        // Build params
        let args: Vec<DocParam> = func.args.iter().enumerate().map(|(i, &sym_idx)| {
            let param_name = match &ext_sym(pg, sym_idx).id {
                SymbolIdentifier::Name(n) => Some(n.clone()),
                _ => None,
            };
            let is_self = param_name.as_deref() == Some("self");
            let optional = func.param_optional.get(i).copied().unwrap_or(false);
            let ann_has_nil = func.param_annotations.get(i)
                .is_some_and(annotations::annotation_type_is_nullable);
            let type_str = if is_self {
                class_name.to_string()
            } else {
                func.param_annotations.get(i)
                    .and_then(|ann| {
                        if matches!(ann, AnnotationType::Simple(s) if s == "any") {
                            return None;
                        }
                        Some(format_annotation_for_doc_param(ann))
                    })
                    .or_else(|| {
                        ext_sym(pg, sym_idx).versions.first()
                            .and_then(|v| v.resolved_type.as_ref())
                            .map(|rt| {
                                let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                                format_value_type_for_doc(&display_type, pg, &func.generics)
                            })
                    })
                    .unwrap_or_else(|| "any".to_string())
            };
            let desc = func.param_descriptions.get(i).cloned().flatten();
            DocParam {
                name: param_name,
                view: type_str,
                desc,
            }
        }).collect();

        // Add vararg param if present
        let mut all_args = args;
        if func.is_vararg {
            let view = match &func.vararg_annotation {
                Some(ann) => annotations::format_annotation_type(ann),
                None => "any".to_string(),
            };
            all_args.push(DocParam {
                name: Some("...".to_string()),
                view,
                desc: func.vararg_description.clone(),
            });
        }

        // Build returns
        let returns: Vec<DocParam> = if func.returns_self {
            vec![DocParam {
                name: None,
                view: "self".to_string(),
                desc: None,
            }]
        } else {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                let formatted = format_value_type_for_doc(vt, pg, &func.generics);
                let view = if func.has_vararg_return && i + 1 == func.return_annotations.len() {
                    format!("...{}", formatted)
                } else {
                    formatted
                };
                let label = func.return_labels.get(i).cloned().flatten();
                let desc = func.return_descriptions.get(i).cloned().flatten();
                DocParam {
                    name: label,
                    view,
                    desc,
                }
            }).collect()
        };

        let desc = func.doc.clone();

        return DocField {
            name: name.to_string(),
            kind,
            extends: Some(DocFieldExtends {
                args: all_args,
                returns,
            }),
            view: None,
            deprecated: func.deprecated,
            visible,
            desc,
        };
    }

    // Non-function field
    let view = field.annotation_text.clone()
        .or_else(|| field.annotation.as_ref().map(|vt| format_value_type(vt, pg)))
        .unwrap_or_else(|| "any".to_string());

    DocField {
        name: name.to_string(),
        kind: DocFieldKind::DataField,
        extends: None,
        view: Some(view),
        deprecated: false,
        visible,
        desc: field.description.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::types::{ExternalLocation, FieldInfo, TableInfo, Expr, Function, Symbol, SymbolVersion, DefNode, SymbolIdentifier, ScopeIndex};

    /// Helper: create a minimal FieldInfo (data field, no annotation).
    fn make_field(annotation: Option<ValueType>, annotation_type_raw: Option<String>) -> FieldInfo {
        FieldInfo {
            expr: ExprId(0),
            extra_exprs: Vec::new(),
            visibility: Visibility::Public,
            annotation,
            annotation_text: annotation_type_raw.clone(),
            annotation_type_raw: annotation_type_raw.map(|s| crate::annotations::parse_type(&s)),
            lateinit: false,
            def_range: None,
            flavor_guard: 0,
            description: None,
            from_scan: false,
        }
    }

    /// Helper: create a function/method field. Pushes the function and expression
    /// into `pg` and returns the FieldInfo referencing them.
    fn make_func_field(pg: &mut PreResolvedGlobals, is_method: bool) -> FieldInfo {
        let self_sym_idx = if is_method {
            let sym = Symbol {
                id: SymbolIdentifier::Name("self".to_string()),
                scope_idx: ScopeIndex(0),
                versions: vec![SymbolVersion {
                    def_node: DefNode::DUMMY,
                    type_source: None,
                    resolved_type: None,
                    type_args: Vec::new(),
                    created_in_scope: ScopeIndex(0),
                    creation_order: 0,
                    original_type_source: None,
                }],
                flavor_guard: 0,
                flavors: 0,
            };
            pg.symbols.push(sym);
            Some(SymbolIndex(EXT_BASE + pg.symbols.len() - 1))
        } else {
            None
        };

        let args = self_sym_idx.into_iter().collect();
        let func = Function {
            def_node: DefNode::DUMMY,
            scope: ScopeIndex(0),
            args,
            rets: Vec::new(),
            return_annotations: Vec::new(),
            return_annotations_raw: Vec::new(),
            return_labels: Vec::new(),
            return_descriptions: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            generics: Vec::new(),
            generic_constraints_raw: Vec::new(),
            param_annotations: Vec::new(),
            param_descriptions: Vec::new(),
            defclass: None,
            defclass_parent: None,
            is_vararg: false,
            vararg_annotation: None,
            vararg_description: None,
            param_optional: Vec::new(),
            returns_self: false,
            explicit_void_return: false,
            implicit_nil_return: false,
            constructor: false,
            builds_field: None,
            built_name: None,
            built_extends: false,
            returns_built: false,
            returns_built_parent: None,
            type_narrows: None,
            type_narrows_class: None,
            has_vararg_return: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            return_projections: Default::default(),
            vararg_projection: None,
            event_params: None,
            narrows_arg: None,
            requires_constraints: Vec::new(),
            returns_self_type_args: None,
        };
        pg.functions.push(func);
        let func_idx = FunctionIndex(EXT_BASE + pg.functions.len() - 1);

        pg.exprs.push(Expr::FunctionDef(func_idx));
        let expr_id = ExprId(EXT_BASE + pg.exprs.len() - 1);

        FieldInfo {
            expr: expr_id,
            extra_exprs: Vec::new(),
            visibility: Visibility::Public,
            annotation: None,
            annotation_text: None,
            annotation_type_raw: None,
            lateinit: false,
            def_range: None,
            flavor_guard: 0,
            description: None,
            from_scan: false,
        }
    }

    /// Helper: register a class on `pg` with the given fields.
    /// `declared` lists field names from explicit `@field` annotations.
    fn register_class(pg: &mut PreResolvedGlobals, class_name: &str, fields: Vec<(&str, FieldInfo)>, root: &Path, declared: &[&str]) {
        let mut table = TableInfo::default();
        table.class_name = Some(class_name.to_string());
        for (name, field) in fields {
            table.fields.insert(name.to_string(), field);
        }
        pg.tables.push(table);
        let table_idx = TableIndex(EXT_BASE + pg.tables.len() - 1);
        pg.classes.insert(class_name.to_string(), table_idx);
        if !declared.is_empty() {
            pg.declared_class_fields.insert(
                class_name.to_string(),
                declared.iter().map(|s| s.to_string()).collect(),
            );
        }
        pg.class_locations.insert(class_name.to_string(), ExternalLocation {
            path: root.join("test.lua"),
            start: 0,
            end: 100,
            ..Default::default()
        });
    }

    #[test]
    fn undocumented_data_fields_excluded() {
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        register_class(&mut pg, "MyClass", vec![
            ("undocumented", make_field(None, None)),
        ], &root, &[]);
        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 1);
        assert!(docs[0].fields.is_empty(), "unannotated data field should be excluded");
    }

    #[test]
    fn inferred_type_fields_excluded() {
        // Fields with annotation (inferred type) but not in declared_class_fields
        // should be excluded — this is the defclass self-field case.
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        let inferred_field = FieldInfo {
            expr: ExprId(0),
            extra_exprs: Vec::new(),
            visibility: Visibility::Public,
            annotation: Some(ValueType::Table(None)),
            annotation_text: None,
            annotation_type_raw: Some(crate::annotations::parse_type("table")),
            lateinit: false,
            def_range: None,
            flavor_guard: 0,
            description: None,
            from_scan: false,
        };
        register_class(&mut pg, "MyClass", vec![
            ("classInfo", inferred_field),
        ], &root, &[]); // not declared
        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 1);
        assert!(docs[0].fields.is_empty(), "inferred-only field should be excluded");
    }

    #[test]
    fn annotated_data_fields_included() {
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        register_class(&mut pg, "MyClass", vec![
            ("count", make_field(Some(ValueType::Number), Some("number".to_string()))),
        ], &root, &["count"]);
        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields.len(), 1);
        assert_eq!(docs[0].fields[0].name, "count");
        assert!(matches!(docs[0].fields[0].kind, DocFieldKind::DataField));
    }

    #[test]
    fn function_fields_always_included() {
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        let method_field = make_func_field(&mut pg, true);
        register_class(&mut pg, "MyClass", vec![
            ("doStuff", method_field),
        ], &root, &[]);

        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields.len(), 1);
        assert_eq!(docs[0].fields[0].name, "doStuff");
        assert!(matches!(docs[0].fields[0].kind, DocFieldKind::Method));
    }

    #[test]
    fn mixed_fields_only_documented_included() {
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        let method = make_func_field(&mut pg, true);
        register_class(&mut pg, "MyClass", vec![
            ("doStuff", method),
            ("count", make_field(Some(ValueType::Number), Some("number".to_string()))),
            ("_internal", make_field(None, None)),
            ("_cache", make_field(None, None)),
        ], &root, &["count"]);

        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 1);
        // Fields are sorted alphabetically. _cache and _internal excluded (not declared).
        let names: Vec<&str> = docs[0].fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["count", "doStuff"]);
    }

    #[test]
    fn class_filter_limits_output() {
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        register_class(&mut pg, "Alpha", vec![
            ("x", make_field(Some(ValueType::Number), Some("number".to_string()))),
        ], &root, &["x"]);
        register_class(&mut pg, "Beta", vec![
            ("y", make_field(Some(ValueType::Number), Some("number".to_string()))),
        ], &root, &["y"]);
        register_class(&mut pg, "Gamma", vec![
            ("z", make_field(Some(ValueType::Number), Some("number".to_string()))),
        ], &root, &["z"]);

        // No filter: all three
        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 3);

        // Filter to one class
        let filter = vec!["Beta".to_string()];
        let docs = generate_docs(&pg, &root, Some(&filter));
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name, "Beta");

        // Filter to two classes
        let filter = vec!["Alpha".to_string(), "Gamma".to_string()];
        let docs = generate_docs(&pg, &root, Some(&filter));
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].name, "Alpha");
        assert_eq!(docs[1].name, "Gamma");
    }

    #[test]
    fn private_fields_excluded_from_docs() {
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        let mut private_field = make_field(Some(ValueType::Table(None)), None);
        private_field.visibility = Visibility::Private;
        register_class(&mut pg, "MyClass", vec![
            ("publicField", make_field(Some(ValueType::Number), Some("number".to_string()))),
            ("secretField", private_field),
        ], &root, &["publicField", "secretField"]);

        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 1);
        let names: Vec<&str> = docs[0].fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["publicField"]);
    }

    #[test]
    fn field_descriptions_included() {
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        let mut field = make_field(Some(ValueType::Number), Some("number".to_string()));
        field.description = Some("The item count.".to_string());
        register_class(&mut pg, "MyClass", vec![
            ("count", field),
        ], &root, &["count"]);
        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields.len(), 1);
        assert_eq!(docs[0].fields[0].desc.as_deref(), Some("The item count."));
    }

    #[test]
    fn field_descriptions_none_when_absent() {
        let mut pg = PreResolvedGlobals::empty();
        let root = PathBuf::from("/test/project");
        register_class(&mut pg, "MyClass", vec![
            ("count", make_field(Some(ValueType::Number), Some("number".to_string()))),
        ], &root, &["count"]);
        let docs = generate_docs(&pg, &root, None);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields.len(), 1);
        assert!(docs[0].fields[0].desc.is_none());
    }
}
