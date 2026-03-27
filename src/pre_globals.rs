use std::collections::{HashMap, HashSet};

use crate::types::*;
use crate::annotations::{AnnotationType, ClassDecl, AliasDecl, parse_overload};
use crate::syntax::SyntaxNodePtr;

/// Check if an annotation type references any of the given type parameter names.
pub(crate) fn annotation_type_references_type_params(at: &AnnotationType, type_params: &[String]) -> bool {
    if type_params.is_empty() { return false; }
    match at {
        AnnotationType::Simple(name) => type_params.iter().any(|p| p == name),
        AnnotationType::Union(parts) => parts.iter().any(|p| annotation_type_references_type_params(p, type_params)),
        AnnotationType::Array(inner) => annotation_type_references_type_params(inner, type_params),
        AnnotationType::Parameterized(_, args) => args.iter().any(|a| annotation_type_references_type_params(a, type_params)),
        AnnotationType::Backtick(inner) => annotation_type_references_type_params(inner, type_params),
        AnnotationType::NonNil(inner) => annotation_type_references_type_params(inner, type_params),
        AnnotationType::Fun(params, returns, _) => {
            params.iter().any(|p| annotation_type_references_type_params(&p.typ, type_params))
            || returns.iter().any(|r| annotation_type_references_type_params(r, type_params))
        }
    }
}

/// Substitute type parameter references in an annotation type with resolved class names.
/// `subs` maps type param name → table index; `classes` maps class name → table index (reverse lookup).
fn substitute_annotation_type(
    at: &AnnotationType,
    subs: &HashMap<String, TableIndex>,
    classes: &HashMap<String, TableIndex>,
) -> AnnotationType {
    // Build reverse map: table_idx → class_name for substitution
    let reverse: HashMap<TableIndex, &String> = classes.iter().map(|(n, &i)| (i, n)).collect();
    substitute_annotation_type_inner(at, subs, &reverse)
}

fn substitute_annotation_type_inner(
    at: &AnnotationType,
    subs: &HashMap<String, TableIndex>,
    reverse: &HashMap<TableIndex, &String>,
) -> AnnotationType {
    match at {
        AnnotationType::Simple(name) => {
            if let Some(&table_idx) = subs.get(name) {
                if let Some(class_name) = reverse.get(&table_idx) {
                    AnnotationType::Simple((*class_name).clone())
                } else {
                    at.clone()
                }
            } else {
                at.clone()
            }
        }
        AnnotationType::Union(parts) => {
            AnnotationType::Union(parts.iter().map(|p| substitute_annotation_type_inner(p, subs, reverse)).collect())
        }
        AnnotationType::Array(inner) => {
            AnnotationType::Array(Box::new(substitute_annotation_type_inner(inner, subs, reverse)))
        }
        AnnotationType::Parameterized(base, args) => {
            AnnotationType::Parameterized(
                base.clone(),
                args.iter().map(|a| substitute_annotation_type_inner(a, subs, reverse)).collect(),
            )
        }
        AnnotationType::Backtick(inner) => {
            AnnotationType::Backtick(Box::new(substitute_annotation_type_inner(inner, subs, reverse)))
        }
        AnnotationType::NonNil(inner) => {
            AnnotationType::NonNil(Box::new(substitute_annotation_type_inner(inner, subs, reverse)))
        }
        AnnotationType::Fun(params, returns, is_vararg) => {
            let new_params: Vec<_> = params.iter().map(|p| crate::annotations::ParamInfo {
                name: p.name.clone(),
                typ: substitute_annotation_type_inner(&p.typ, subs, reverse),
                optional: p.optional,
                description: p.description.clone(),
            }).collect();
            let new_returns: Vec<_> = returns.iter().map(|r| substitute_annotation_type_inner(r, subs, reverse)).collect();
            AnnotationType::Fun(new_params, new_returns, *is_vararg)
        }
    }
}

// ── Pre-resolved External Globals ─────────────────────────────────────────────
//
// Built once at startup from workspace scan results. Contains pre-built
// Function/Symbol/Scope/Expr entries with 0-based internal indices.
// Injected into each file's Analysis with index offsets (~0.1ms vs ~35ms).

#[derive(Debug)]
pub struct PreResolvedGlobals {
    pub(crate) scopes: Vec<Scope>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) functions: Vec<Function>,
    pub(crate) exprs: Vec<Expr>,
    pub(crate) tables: Vec<TableInfo>,
    pub(crate) classes: HashMap<String, TableIndex>,
    pub(crate) aliases: HashMap<String, ValueType>,
    pub(crate) scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    pub(crate) framexml_scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    pub(crate) symbol_locations: HashMap<usize, ExternalLocation>,
    pub(crate) function_locations: HashMap<usize, ExternalLocation>,
    /// String literal values for global symbols (SymbolIndex → string value)
    pub(crate) string_values: HashMap<SymbolIndex, String>,
    /// Number literal values for global symbols (SymbolIndex → number text)
    pub(crate) number_values: HashMap<SymbolIndex, String>,
    pub addon_table_idx: Option<TableIndex>,
}

impl PreResolvedGlobals {
    pub fn symbols_len(&self) -> usize { self.symbols.len() }
    pub fn functions_len(&self) -> usize { self.functions.len() }
    pub fn tables_len(&self) -> usize { self.tables.len() }

    pub fn empty() -> PreResolvedGlobals {
        PreResolvedGlobals {
            scopes: Vec::new(),
            symbols: Vec::new(),
            functions: Vec::new(),
            exprs: Vec::new(),
            tables: Vec::new(),
            classes: HashMap::new(),
            aliases: HashMap::new(),
            scope0_symbols: HashMap::new(),
            framexml_scope0_symbols: HashMap::new(),
            symbol_locations: HashMap::new(),
            function_locations: HashMap::new(),
            string_values: HashMap::new(),
            number_values: HashMap::new(),
            addon_table_idx: None,
        }
    }

    pub fn build(
        globals: &[crate::annotations::ExternalGlobal],
        external_classes: &[ClassDecl],
        external_aliases: &[AliasDecl],
    ) -> PreResolvedGlobals {
        use crate::annotations::{ExternalGlobalKind, FieldValueKind};

        // All indices in this method use EXT_BASE so they're directly usable
        // in the global index space without any per-file adjustment.

        let mut scopes = Vec::new();
        let mut symbols = Vec::new();
        let mut functions = Vec::new();
        let mut exprs: Vec<Expr> = Vec::new();
        let mut tables: Vec<TableInfo> = Vec::new();
        let mut classes: HashMap<String, TableIndex> = HashMap::new();
        let mut aliases: HashMap<String, ValueType> = HashMap::new();
        let mut symbol_locations: HashMap<usize, ExternalLocation> = HashMap::new();
        let mut function_locations: HashMap<usize, ExternalLocation> = HashMap::new();

        // Dummy SyntaxNodePtr (parse a trivial string to get a valid root node)
        let mut parser = crate::syntax::syntax::Generator::new("--");
        let green = parser.process_all();
        let root = crate::syntax::syntax::SyntaxNode::new_root(green);
        let dummy_node = SyntaxNodePtr::new(&root);

        // ── Step 1: Build classes and aliases ──────────────────────────────
        // Pass 1: Register all class names (table indices use EXT_BASE)
        for class in external_classes {
            let table_idx = EXT_BASE + tables.len();
            let accessors = class.accessors.iter().cloned().collect();
            tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class.name.clone()),
                class_type_params: class.type_params.clone(),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
                key_type: None,
                value_type: None,
                accessors,
                call_func: None,
                constructors: class.constructor_methods.iter().cloned().collect(),
                built_table: None,
                is_enum: class.is_enum,
            });
            classes.insert(class.name.clone(), table_idx);
        }

        // Register aliases before populating fields so alias types (e.g. fileID)
        // are available during field type resolution.
        for alias in external_aliases {
            if let Some(vt) = Self::resolve_annotation(&alias.typ, &classes, &aliases) {
                aliases.insert(alias.name.clone(), vt);
            }
        }

        // Pass 2: Populate @field entries (expr indices use EXT_BASE)
        for class in external_classes {
            let table_idx = classes[&class.name];
            let local_idx = table_idx - EXT_BASE;
            for (field_name, annotation_type, visibility) in &class.fields {
                // Handle index signatures: @field [string] Type or @field [number] Type
                if field_name == "[string]" || field_name == "[number]" {
                    if let Some(vt) = Self::resolve_annotation(annotation_type, &classes, &aliases) {
                        if field_name == "[string]" {
                            tables[local_idx].key_type = Some(ValueType::String(None));
                        } else {
                            tables[local_idx].key_type = Some(ValueType::Number);
                        }
                        tables[local_idx].value_type = Some(vt);
                    }
                    continue;
                }
                // Check if the annotation is a fun(...) type — if so, build a real Function entry
                let vt = if let AnnotationType::Simple(name) = annotation_type {
                    if let Some(sig) = parse_overload(name) {
                        let func_idx = Self::build_function(
                            &sig.params, &sig.returns, &[], None,
                            false, false, None, None, &[],
                            None, None, false, None, false,
                            dummy_node, &mut scopes, &mut symbols, &mut functions,
                            &mut tables, &classes, &aliases,
                        );
                        Some(ValueType::Function(Some(func_idx)))
                    } else {
                        Self::resolve_annotation(annotation_type, &classes, &aliases)
                    }
                } else {
                    Self::resolve_annotation(annotation_type, &classes, &aliases)
                };
                if let Some(vt) = vt {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text: None,
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                } else if annotation_type_references_type_params(annotation_type, &tables[local_idx].class_type_params) {
                    // Field type references a class type param (e.g., @field __super S?)
                    // Store with annotation: None but preserve the raw type for later substitution
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(ValueType::Nil));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: None,
                        annotation_text: None,
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Build call functions from @overload on @class declarations
        for class in external_classes {
            if class.overloads.is_empty() { continue; }
            let table_idx = classes[&class.name];
            let local_idx = table_idx - EXT_BASE;
            let overload = &class.overloads[0];
            let func_idx = Self::build_function(
                &overload.params, &overload.returns, &[], None,
                false, false, None, None, &class.generics,
                None, None, false, None, false,
                dummy_node, &mut scopes, &mut symbols, &mut functions,
                &mut tables, &classes, &aliases,
            );
            tables[local_idx].call_func = Some(func_idx);
        }

        // ── Step 2: Build external global entries ──────────────────────────

        // Create non-class tables in shared data (e.g. math, string, table)
        let mut non_class_tables: HashMap<String, TableIndex> = HashMap::new();
        let mut table_source_locations: HashMap<String, ExternalLocation> = HashMap::new();
        // Track class names that have a global `= {}` assignment (e.g. UIParent)
        let mut class_globals: HashSet<String> = HashSet::new();
        for g in globals {
            if let ExternalGlobalKind::Table = &g.kind {
                if classes.contains_key(&g.name) {
                    class_globals.insert(g.name.clone());
                    if let Some(path) = &g.source_path {
                        table_source_locations.insert(g.name.clone(), ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end,
                        });
                    }
                } else if !non_class_tables.contains_key(&g.name) {
                    let table_idx = EXT_BASE + tables.len();
                    tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                    non_class_tables.insert(g.name.clone(), table_idx);
                    if let Some(path) = &g.source_path {
                        table_source_locations.insert(g.name.clone(), ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end,
                        });
                    }
                }
            }
        }

        // Create shared addon namespace table if any files contribute to it
        let addon_table_idx = if globals.iter().any(|g| g.name == crate::annotations::ADDON_NS_NAME) {
            let table_idx = EXT_BASE + tables.len();
            tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
            non_class_tables.insert(crate::annotations::ADDON_NS_NAME.to_string(), table_idx);
            Some(table_idx)
        } else {
            None
        };

        // Auto-create tables for method/field targets that aren't already known
        // (e.g. classes created via @defclass in user code that have methods scanned by workspace)
        for g in globals {
            let target_name = match &g.kind {
                ExternalGlobalKind::Method(_, _) | ExternalGlobalKind::TableField(_, _) | ExternalGlobalKind::NestedMethod(_, _, _) => &g.name,
                _ => continue,
            };
            if classes.contains_key(target_name) || non_class_tables.contains_key(target_name) {
                continue;
            }
            let table_idx = EXT_BASE + tables.len();
            tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(target_name.clone()),
                class_type_params: Vec::new(),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
                key_type: None,
                value_type: None,
                accessors: HashMap::new(),
                call_func: None,
                constructors: HashSet::new(),
                built_table: None,
                is_enum: false,
            });
            classes.insert(target_name.clone(), table_idx);
        }

        // Build method function entries and add directly to class/table tables.
        // Done BEFORE inheritance so methods are inherited by child classes.
        let mut seen_methods: HashSet<(&str, &str)> = HashSet::new();
        for g in globals {
            if let ExternalGlobalKind::Method(method_name, is_colon) = &g.kind {
                let target_table = classes.get(&g.name).or_else(|| non_class_tables.get(&g.name));
                let Some(&table_idx) = target_table else { continue; };
                if !seen_methods.insert((&g.name, method_name)) { continue; }

                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, *is_colon,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &mut tables, &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    function_locations.insert(func_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
                let expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));

                let local_idx = table_idx - EXT_BASE;
                // Check if any intermediate path component is an accessor with visibility.
                // Walk parent classes too since accessors may be inherited (e.g. Class → LibTSMComponent).
                let accessor_vis = if !g.intermediates.is_empty() {
                    let mut vis = None;
                    // Check the table itself
                    for iname in &g.intermediates {
                        if let Some(&v) = tables[local_idx].accessors.get(iname.as_str()) {
                            vis = Some(v);
                            break;
                        }
                    }
                    // Check parent classes (by name lookup) if not found
                    if vis.is_none() {
                        if let Some(ref class_name) = tables[local_idx].class_name {
                            if let Some(parent_names) = external_classes.iter()
                                .find(|c| c.name == *class_name)
                                .map(|c| &c.parents) {
                                for pname in parent_names {
                                    if let Some(&pidx) = classes.get(pname.as_str()) {
                                        let plocal = pidx - EXT_BASE;
                                        for iname in &g.intermediates {
                                            if let Some(&v) = tables[plocal].accessors.get(iname.as_str()) {
                                                vis = Some(v);
                                                break;
                                            }
                                        }
                                        if vis.is_some() { break; }
                                    }
                                }
                            }
                        }
                    }
                    vis
                } else { None };
                let visibility = accessor_vis.unwrap_or(g.visibility);
                tables[local_idx].fields.entry(method_name.clone()).or_insert(FieldInfo {
                    expr: expr_id,
                    visibility,
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    extra_exprs: Vec::new(),
                });
                if g.constructor {
                    functions[func_idx].constructor = true;
                    tables[local_idx].constructors.insert(method_name.clone());
                }
            }
        }

        // Build addon table field entries (non-function fields like ns.version = 1)
        // Track sub-tables (parent_name, field_name) → table_idx for nested methods
        // Two passes: first typed fields (creating sub-tables), then Unknown fields (which may reuse sub-tables)
        let mut sub_tables: HashMap<(String, String), TableIndex> = HashMap::new();
        for g in globals {
            if let ExternalGlobalKind::TableField(field_name, value_kind) = &g.kind {
                if matches!(value_kind, FieldValueKind::Unknown) && g.returns.is_empty() { continue; }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                let value_type = if !g.returns.is_empty() {
                    Self::resolve_annotation(&g.returns[0], &classes, &aliases)
                } else {
                    match value_kind {
                        FieldValueKind::String => Some(ValueType::String(None)),
                        FieldValueKind::Number => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table => {
                            let sub_idx = EXT_BASE + tables.len();
                            tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                            sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        FieldValueKind::FunctionCall(..) => None, // deferred below
                        FieldValueKind::FieldRef(_) => None, // deferred below
                        FieldValueKind::Unknown => unreachable!(), // handled in second pass
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name),
                        annotation,
                        annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }
        // Second pass: resolve Unknown fields now that all sub-tables exist
        for g in globals {
            if let ExternalGlobalKind::TableField(field_name, value_kind) = &g.kind {
                if !matches!(value_kind, FieldValueKind::Unknown) || !g.returns.is_empty() { continue; }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                let value_type = if let Some(&idx) = classes.get(field_name) {
                    ValueType::Table(Some(idx))
                } else if let Some(&sub_idx) = sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), field_name.clone())) {
                    // Reuse addon sub-table (e.g. LibTSMApp.Locale shares ns.Locale's sub-table)
                    ValueType::Table(Some(sub_idx))
                } else {
                    // Register as untyped table so the field is at least visible
                    ValueType::Table(None)
                };
                let expr_idx = EXT_BASE + exprs.len();
                exprs.push(Expr::Literal(value_type.clone()));
                tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                    expr: expr_idx,
                    visibility: crate::annotations::default_visibility_for_name(field_name),
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    extra_exprs: Vec::new(),
                });
            }
        }

        // Build nested method entries (e.g., function ns.DB:Start())
        for g in globals {
            if let ExternalGlobalKind::NestedMethod(sub_field, method_name, is_colon) = &g.kind {
                let Some(&sub_idx) = sub_tables.get(&(g.name.clone(), sub_field.clone())) else { continue };
                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, *is_colon,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &mut tables, &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    function_locations.insert(func_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
                let expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));
                let local_idx = sub_idx - EXT_BASE;
                tables[local_idx].fields.entry(method_name.clone()).or_insert(FieldInfo {
                    expr: expr_id,
                    visibility: g.visibility,
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    extra_exprs: Vec::new(),
                });
            }
        }

        // Pass 3: Resolve inheritance via topological sort.
        // Instead of copying parent fields into children (expensive with FrameXML's
        // 19k+ classes), compute transitive parent_classes so get_field() can walk
        // the chain at lookup time.
        {
            // Build adjacency: parent_name → vec of child indices into external_classes
            let mut children_of: HashMap<&str, Vec<usize>> = HashMap::new();
            let mut in_degree: Vec<usize> = vec![0; external_classes.len()];
            let mut class_index: HashMap<&str, usize> = HashMap::new();
            for (i, class) in external_classes.iter().enumerate() {
                class_index.insert(&class.name, i);
            }
            for (i, class) in external_classes.iter().enumerate() {
                for parent_name in &class.parents {
                    if class_index.contains_key(parent_name.as_str()) {
                        children_of.entry(parent_name.as_str()).or_default().push(i);
                        in_degree[i] += 1;
                    }
                }
            }
            // Kahn's algorithm: start with roots (in_degree == 0)
            let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
            for (i, &deg) in in_degree.iter().enumerate() {
                if deg == 0 { queue.push_back(i); }
            }
            let mut order: Vec<usize> = Vec::with_capacity(external_classes.len());
            let mut processed_names: HashSet<&str> = HashSet::new();
            while let Some(idx) = queue.pop_front() {
                let name = external_classes[idx].name.as_str();
                // Skip duplicate class names (same class from multiple stub files)
                if !processed_names.insert(name) { continue; }
                order.push(idx);
                if let Some(kids) = children_of.get(name) {
                    for &kid in kids {
                        in_degree[kid] = in_degree[kid].saturating_sub(1);
                        if in_degree[kid] == 0 { queue.push_back(kid); }
                    }
                }
            }
            // Append any remaining (cycles) so they still get partial resolution
            for i in 0..external_classes.len() {
                if in_degree[i] > 0 && processed_names.insert(external_classes[i].name.as_str()) {
                    order.push(i);
                }
            }
            // Compute transitive parent_classes for each unique class (from topo order).
            // Then accumulate parents from any duplicate ClassDecl entries with the same name.
            for &idx in &order {
                let class = &external_classes[idx];
                if class.parents.is_empty() { continue; }
                let child_local = classes[&class.name] - EXT_BASE;
                let mut transitive_parents: Vec<TableIndex> = Vec::new();
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = classes.get(parent_name.as_str()) {
                        if !transitive_parents.contains(&parent_idx) {
                            transitive_parents.push(parent_idx);
                        }
                        // Add all of parent's ancestors (already computed due to topo order)
                        let parent_local = parent_idx - EXT_BASE;
                        for &ancestor in &tables[parent_local].parent_classes {
                            if !transitive_parents.contains(&ancestor) {
                                transitive_parents.push(ancestor);
                            }
                        }
                    }
                }
                tables[child_local].parent_classes = transitive_parents;
            }
            // Accumulate parents from duplicate ClassDecl entries (same name, different parents).
            // The topo sort only processed one entry per name, but duplicates may have
            // additional parents (e.g. defclass scan adds specific parent).
            for class in external_classes.iter() {
                if class.parents.is_empty() { continue; }
                let Some(&child_table_idx) = classes.get(class.name.as_str()) else { continue };
                let child_local = child_table_idx - EXT_BASE;
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = classes.get(parent_name.as_str()) {
                        if !tables[child_local].parent_classes.contains(&parent_idx) {
                            tables[child_local].parent_classes.push(parent_idx);
                            // Also add this parent's transitive ancestors
                            let parent_local = parent_idx - EXT_BASE;
                            for &ancestor in &tables[parent_local].parent_classes.clone() {
                                if !tables[child_local].parent_classes.contains(&ancestor) {
                                    tables[child_local].parent_classes.push(ancestor);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Pass 3b: Apply constraint type param substitutions for defclass-scanned classes.
        // For classes like `ReactivePublisherSchema` with constraint `T: Class<P>` where
        // P=ReactivePublisherSchemaBase, substitute the parent class's type params (S)
        // with the resolved values (ReactivePublisherSchemaBase) in inherited fields.
        for class in external_classes.iter() {
            if class.constraint_type_arg_subs.is_empty() { continue; }
            let child_local = classes[&class.name] - EXT_BASE;
            for (constraint_base, resolved_args) in &class.constraint_type_arg_subs {
                let Some(&parent_idx) = classes.get(constraint_base.as_str()) else { continue };
                let parent_local = parent_idx - EXT_BASE;
                let parent_type_params = tables[parent_local].class_type_params.clone();
                if parent_type_params.is_empty() || parent_type_params.len() != resolved_args.len() {
                    continue;
                }
                // Build substitution map: class_type_param → resolved class name → table index
                let mut subs: HashMap<String, TableIndex> = HashMap::new();
                for (tp, resolved_name) in parent_type_params.iter().zip(resolved_args.iter()) {
                    if let Some(&tidx) = classes.get(resolved_name.as_str()) {
                        subs.insert(tp.clone(), tidx);
                    }
                }
                if subs.is_empty() { continue; }
                // Walk parent tables to find fields needing type param substitution.
                // Copy only those specific fields to the child with substituted types.
                let parents = tables[child_local].parent_classes.clone();
                for &pi in &parents {
                    let pi_local = pi - EXT_BASE;
                    let parent_fields: Vec<(String, FieldInfo)> = tables[pi_local].fields.iter()
                        .filter(|(_, fi)| fi.annotation_type_raw.as_ref()
                            .is_some_and(|raw| annotation_type_references_type_params(raw, &parent_type_params)))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (fname, fi) in parent_fields {
                        if tables[child_local].fields.contains_key(&fname) { continue; }
                        let raw = fi.annotation_type_raw.as_ref().unwrap().clone();
                        let substituted = substitute_annotation_type(&raw, &subs, &classes);
                        if let Some(resolved) = crate::annotations::resolve_annotation_type(
                            &substituted, &[], &classes, &aliases,
                        ) {
                            let mut child_fi = fi;
                            child_fi.annotation = Some(resolved);
                            tables[child_local].fields.insert(fname, child_fi);
                        }
                    }
                }
            }
        }

        // Pass 3c: Substitute inherited field types based on field_built_names overrides.
        // When a child class (e.g. BaseFrame) overrides a parent's @built-name for a field
        // (e.g. _STATE_SCHEMA: "BaseFrameState" vs parent's "ElementState"), substitute
        // all inherited field types that reference the parent's built class with the child's.
        // Pre-build name → ClassDecl(s) index for O(1) lookups.
        // Multiple files may declare the same class name, so use a multimap.
        let mut built_extends_parents: Vec<(TableIndex, TableIndex)> = Vec::new();
        let mut class_decls_by_name: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, c) in external_classes.iter().enumerate() {
            class_decls_by_name.entry(c.name.as_str()).or_default().push(i);
        }
        for class in external_classes.iter() {
            if class.field_built_names.is_empty() { continue; }
            let child_local = classes[&class.name] - EXT_BASE;
            // Build substitution map: old_class_name → new_class_table_index
            let mut type_subs: HashMap<String, TableIndex> = HashMap::new();
            // Collect ALL ancestor class names by transitively walking the parent chain.
            // BaseFrame → Container → Element requires walking multiple levels.
            let mut ancestor_names: HashSet<String> = HashSet::new();
            let mut queue: Vec<String> = class.parents.clone();
            while let Some(parent_name) = queue.pop() {
                if !ancestor_names.insert(parent_name.clone()) { continue; }
                // Also add canonical class name from the table
                if let Some(&pidx) = classes.get(parent_name.as_str()) {
                    if let Some(cn) = tables[pidx - EXT_BASE].class_name.as_ref() {
                        if ancestor_names.insert(cn.clone()) {
                            queue.push(cn.clone());
                        }
                    }
                    // Walk this table's parent_classes (already resolved by pass 3)
                    for &gp_idx in &tables[pidx - EXT_BASE].parent_classes {
                        if let Some(gp_cn) = tables[gp_idx - EXT_BASE].class_name.as_ref() {
                            if !ancestor_names.contains(gp_cn) {
                                queue.push(gp_cn.clone());
                            }
                        }
                    }
                }
                // Walk ClassDecl parents for this ancestor
                if let Some(indices) = class_decls_by_name.get(parent_name.as_str()) {
                    for &idx in indices {
                        for p in &external_classes[idx].parents {
                            if !ancestor_names.contains(p) {
                                queue.push(p.clone());
                            }
                        }
                    }
                }
            }
            // For each field_built_name on the child, find an ancestor ClassDecl that has a
            // different built_name for the same field. The child's built_name overrides the ancestor's.
            for (field_name, child_built) in &class.field_built_names {
                for ancestor_name in &ancestor_names {
                    if let Some(indices) = class_decls_by_name.get(ancestor_name.as_str()) {
                        for &idx in indices {
                            if let Some(ancestor_built) = external_classes[idx].field_built_names.get(field_name) {
                                if ancestor_built != child_built {
                                    if let Some(&new_idx) = classes.get(child_built.as_str()) {
                                        type_subs.insert(ancestor_built.clone(), new_idx);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if type_subs.is_empty() { continue; }
            // Collect parent_classes additions for deferred application.

            for (old_class_name, &new_idx) in &type_subs {
                if let Some(&old_idx) = classes.get(old_class_name.as_str()) {
                    built_extends_parents.push((new_idx, old_idx));
                }
            }
            // Apply substitutions to inherited fields on the child.
            // Walk own fields (may include overrides from pass 3b) + parent fields.
            let mut fields_to_sub: Vec<(String, FieldInfo)> = Vec::new();
            // Check own fields first (from pass 3b overrides)
            for (fname, fi) in &tables[child_local].fields {
                if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                    if *tidx >= EXT_BASE {
                        let tidx_local = *tidx - EXT_BASE;
                        if let Some(old_class_name) = tables[tidx_local].class_name.as_ref() {
                            if type_subs.contains_key(old_class_name) {
                                fields_to_sub.push((fname.clone(), fi.clone()));
                            }
                        }
                    }
                }
            }
            // Check parent fields
            let parents = tables[child_local].parent_classes.clone();
            for &pi in &parents {
                let pi_local = pi - EXT_BASE;
                for (fname, fi) in &tables[pi_local].fields {
                    if tables[child_local].fields.contains_key(fname) { continue; }
                    if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                        if *tidx >= EXT_BASE {
                            let tidx_local = *tidx - EXT_BASE;
                            if let Some(old_class_name) = tables[tidx_local].class_name.as_ref() {
                                if type_subs.contains_key(old_class_name) {
                                    fields_to_sub.push((fname.clone(), fi.clone()));
                                }
                            }
                        }
                    }
                }
            }
            for (fname, fi) in fields_to_sub {
                if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                    let tidx_local = *tidx - EXT_BASE;
                    if let Some(old_class_name) = tables[tidx_local].class_name.as_ref() {
                        if let Some(&new_idx) = type_subs.get(old_class_name) {
                            let new_vt = ValueType::Table(Some(new_idx));
                            let new_expr_idx = EXT_BASE + exprs.len();
                            exprs.push(Expr::Literal(new_vt.clone()));
                            let mut child_fi = fi.clone();
                            child_fi.annotation = Some(new_vt);
                            child_fi.expr = new_expr_idx;
                            tables[child_local].fields.insert(fname, child_fi);
                        }
                    }
                }
            }
        }

        // Apply deferred @built-extends parent_classes.
        // E.g. ChildElemState gets ParentElemState as a parent so inherited fields are visible.
        for (new_idx, old_idx) in built_extends_parents {
            let new_local = new_idx - EXT_BASE;
            if !tables[new_local].parent_classes.contains(&old_idx) {
                tables[new_local].parent_classes.push(old_idx);
            }
        }
        // Build global function entries
        let mut scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex> = HashMap::new();
        let mut framexml_names: HashSet<String> = HashSet::new();
        let is_framexml = |path: &Option<std::path::PathBuf>| -> bool {
            path.as_ref().is_some_and(|p: &std::path::PathBuf| p.to_string_lossy().contains("/Annotations/FrameXML/"))
        };
        let register_global = |name: &str, resolved_type: Option<ValueType>, symbols: &mut Vec<Symbol>, scope0_symbols: &mut HashMap<SymbolIdentifier, SymbolIndex>| -> SymbolIndex {
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(name.to_string()),
                scope_idx: 0,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type,
                    type_args: Vec::new(),
                    created_in_scope: 0,
                }],
            });
            scope0_symbols.insert(SymbolIdentifier::Name(name.to_string()), sym_idx);
            sym_idx
        };
        let mut seen_functions: HashSet<&str> = HashSet::new();
        for g in globals {
            if let ExternalGlobalKind::Function = &g.kind {
                if !seen_functions.insert(&g.name) { continue; }

                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, false,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &mut tables, &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    let loc = ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    };
                    function_locations.insert(func_idx, loc.clone());
                    symbol_locations.insert(EXT_BASE + symbols.len(), loc);
                }
                let _expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));

                register_global(&g.name, Some(ValueType::Function(Some(func_idx))), &mut symbols, &mut scope0_symbols);
                if is_framexml(&g.source_path) { framexml_names.insert(g.name.clone()); }
            }
        }

        // Register simple global variables (e.g. WOW_PROJECT_ID = 0)
        let mut string_values: HashMap<SymbolIndex, String> = HashMap::new();
        let mut number_values: HashMap<SymbolIndex, String> = HashMap::new();
        for g in globals {
            if let ExternalGlobalKind::Variable(vk) = &g.kind {
                if scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) { continue; }
                // Skip variable stubs when a @class with the same name has a
                // global `= {}` assignment (e.g. MailFrame = nil in GlobalVariables
                // vs @class MailFrame : Frame in FrameXML stubs).
                if class_globals.contains(&g.name) { continue; }
                let resolved_type = match vk {
                    FieldValueKind::Number => Some(ValueType::Number),
                    FieldValueKind::String => Some(ValueType::String(None)),
                    FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                    FieldValueKind::Nil => Some(ValueType::Nil),
                    _ => None,
                };
                let sym_idx = register_global(&g.name, resolved_type, &mut symbols, &mut scope0_symbols);
                if let Some(ref sv) = g.string_value {
                    string_values.insert(sym_idx, sv.clone());
                }
                if let Some(ref nv) = g.number_value {
                    number_values.insert(sym_idx, nv.clone());
                }
                if is_framexml(&g.source_path) { framexml_names.insert(g.name.clone()); }
                if let Some(path) = &g.source_path {
                    symbol_locations.insert(sym_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
            }
        }

        // Register non-class tables as scope0 symbols
        for (name, &table_idx) in &non_class_tables {
            let sym_idx = register_global(name, Some(ValueType::Table(Some(table_idx))), &mut symbols, &mut scope0_symbols);
            if let Some(loc) = table_source_locations.get(name) {
                symbol_locations.insert(sym_idx, loc.clone());
            }
        }

        // Register callable class tables and class globals as scope0 symbols
        // (e.g. LibStub with @overload, UIParent with global `= {}` assignment)
        for (name, &table_idx) in &classes {
            if scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) { continue; }
            let local_idx = table_idx - EXT_BASE;
            if tables[local_idx].call_func.is_none() && !class_globals.contains(name) { continue; }
            let sym_idx = register_global(name, Some(ValueType::Table(Some(table_idx))), &mut symbols, &mut scope0_symbols);
            if let Some(loc) = table_source_locations.get(name) {
                symbol_locations.insert(sym_idx, loc.clone());
            }
        }

        // Register field-ref globals (e.g. `strmatch = str.match` → string.match)
        for g in globals {
            if let ExternalGlobalKind::FieldRef(table_name, field_name) = &g.kind {
                if scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) { continue; }
                let table_local_idx = non_class_tables.get(table_name)
                    .or_else(|| classes.get(table_name))
                    .map(|idx| idx - EXT_BASE);
                if let Some(local_idx) = table_local_idx {
                    if let Some(field) = tables[local_idx].fields.get(field_name) {
                        let resolved_type = match &exprs[field.expr - EXT_BASE] {
                            Expr::FunctionDef(func_idx) => Some(ValueType::Function(Some(*func_idx))),
                            _ => None,
                        };
                        if let Some(resolved_type) = resolved_type {
                            let sym_idx = register_global(&g.name, Some(resolved_type), &mut symbols, &mut scope0_symbols);
                            if is_framexml(&g.source_path) { framexml_names.insert(g.name.clone()); }
                            if let Some(path) = &g.source_path {
                                symbol_locations.insert(sym_idx, ExternalLocation {
                                    path: path.clone(), start: g.def_start, end: g.def_end,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Deferred: resolve FunctionCall table fields now that all functions/tables are built
        for g in globals {
            if let ExternalGlobalKind::TableField(field_name, FieldValueKind::FunctionCall(callee_chain, first_string_arg)) = &g.kind {
                if !g.returns.is_empty() {
                    // Has explicit @type annotation — use it directly
                    let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                    let local_idx = table_idx - EXT_BASE;
                    if tables[local_idx].fields.contains_key(field_name) { continue; }
                    if let Some(vt) = Self::resolve_annotation(&g.returns[0], &classes, &aliases) {
                        let expr_idx = EXT_BASE + exprs.len();
                        exprs.push(Expr::Literal(vt.clone()));
                        tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name),
                            annotation: Some(vt),
                            annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                            extra_exprs: Vec::new(),
                        });
                    }
                    continue;
                }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }

                // Walk the callee chain to find the function's return type
                let return_type = Self::resolve_funcall_chain(
                    callee_chain, &tables, &exprs, &functions,
                    &non_class_tables, &classes, &scope0_symbols, &symbols,
                );
                // Filter out TypeVariable — unresolved generics are not useful as field types
                let return_type = return_type.filter(|vt| !matches!(vt, ValueType::TypeVariable(_)));
                let vt = return_type.or_else(|| {
                    // Fallback: if the call had a string literal arg matching a known class
                    // (e.g. EnumType.New("BANKING_FRAME", ...) creates class BANKING_FRAME)
                    first_string_arg.as_ref()
                        .and_then(|name| classes.get(name.as_str()))
                        .map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    // Fallback: check if field name matches a known class
                    classes.get(field_name).map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    // Fallback: create empty sub-table for addon namespace fields
                    // (e.g. ns.LibTSMApp = ns.LibTSMCore.NewComponent("LibTSMApp"))
                    if g.name == crate::annotations::ADDON_NS_NAME {
                        let sub_idx = EXT_BASE + tables.len();
                        tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                        sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                        Some(ValueType::Table(Some(sub_idx)))
                    } else {
                        None
                    }
                });
                if let Some(vt) = vt {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name),
                        annotation: None,
                        annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Deferred: resolve FieldRef table fields by looking up the source table's field type
        for g in globals {
            if let ExternalGlobalKind::TableField(field_name, FieldValueKind::FieldRef(ref_chain)) = &g.kind {
                if !g.returns.is_empty() { continue; }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                // Walk the ref chain: ref_chain[0] is the source table, ref_chain[1..] are field names
                let source_table_idx = non_class_tables.get(&ref_chain[0])
                    .or_else(|| classes.get(&ref_chain[0]))
                    .or_else(|| sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), ref_chain[0].clone())));
                if let Some(&mut_src_idx) = source_table_idx {
                    let mut current = mut_src_idx;
                    let mut resolved = None;
                    for (i, name) in ref_chain[1..].iter().enumerate() {
                        let src_local = current - EXT_BASE;
                        if let Some(fi) = tables[src_local].fields.get(name) {
                            if i == ref_chain.len() - 2 {
                                // Last field — grab its type
                                if let Some(ref ann) = fi.annotation {
                                    resolved = Some(ann.clone());
                                } else {
                                    let expr = &exprs[fi.expr - EXT_BASE];
                                    if let Expr::Literal(vt) = expr {
                                        resolved = Some(vt.clone());
                                    }
                                }
                            } else {
                                // Intermediate field — follow to next table
                                if let Some(ref ann) = fi.annotation {
                                    if let ValueType::Table(Some(idx)) = ann {
                                        current = *idx;
                                        continue;
                                    }
                                }
                                let expr = &exprs[fi.expr - EXT_BASE];
                                if let Expr::Literal(ValueType::Table(Some(idx))) = expr {
                                    current = *idx;
                                } else {
                                    break;
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    if let Some(vt) = resolved {
                        let expr_idx = EXT_BASE + exprs.len();
                        exprs.push(Expr::Literal(vt.clone()));
                        tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name),
                            annotation: None,
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            extra_exprs: Vec::new(),
                        });
                    }
                }
            }
        }

        // Register addon sub-tables in non_class_tables so fields on them can be resolved
        // (e.g. ns.App created from a method chain, then ns.App.Locale = Locale)
        for ((parent, field), &idx) in &sub_tables {
            if parent == crate::annotations::ADDON_NS_NAME {
                non_class_tables.entry(field.clone()).or_insert(idx);
            }
        }
        // Re-process table field globals whose parent table was just created as a sub-table
        for g in globals {
            if let ExternalGlobalKind::TableField(field_name, value_kind) = &g.kind {
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                let value_type = if !g.returns.is_empty() {
                    Self::resolve_annotation(&g.returns[0], &classes, &aliases)
                } else {
                    match value_kind {
                        FieldValueKind::String => Some(ValueType::String(None)),
                        FieldValueKind::Number => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table => {
                            let sub_idx = EXT_BASE + tables.len();
                            tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                            sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        _ => None,
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name),
                        annotation,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Register _G (the global environment table) as a built-in global
        if !scope0_symbols.contains_key(&SymbolIdentifier::Name("_G".to_string())) {
            register_global("_G", Some(ValueType::Table(None)), &mut symbols, &mut scope0_symbols);
        }

        // Partition scope0_symbols: move FrameXML-only globals to a separate map
        let mut framexml_scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex> = HashMap::new();
        for name in &framexml_names {
            let key = SymbolIdentifier::Name(name.clone());
            if let Some(idx) = scope0_symbols.remove(&key) {
                framexml_scope0_symbols.insert(key, idx);
            }
        }

        PreResolvedGlobals {
            scopes, symbols, functions, exprs, tables,
            classes, aliases, scope0_symbols, framexml_scope0_symbols,
            symbol_locations, function_locations, string_values, number_values,
            addon_table_idx,
        }
    }

    /// Build a new PreResolvedGlobals by cloning an existing stubs base and adding
    /// workspace-specific globals, classes, and aliases on top. Much faster than
    /// rebuilding everything from scratch since the stubs portion (~95% of data)
    /// is just cloned rather than re-processed.
    pub fn build_on_stubs(
        stubs_base: &PreResolvedGlobals,
        ws_globals: &[crate::annotations::ExternalGlobal],
        ws_classes: &[ClassDecl],
        ws_aliases: &[AliasDecl],
    ) -> PreResolvedGlobals {
        // Merge stubs + workspace data and do a full build.
        // The key optimization: we only need to collect workspace data here,
        // not clone the much larger stubs data — build() handles everything.
        //
        // For the actual speedup, we clone the stubs base's vectors and extend
        // them with workspace data, avoiding the expensive rebuild of stub entries.
        use crate::annotations::{ExternalGlobalKind, FieldValueKind};

        // Clone all stubs data as our starting point
        let mut scopes = stubs_base.scopes.clone();
        let mut symbols = stubs_base.symbols.clone();
        let mut functions = stubs_base.functions.clone();
        let mut exprs = stubs_base.exprs.clone();
        let mut tables = stubs_base.tables.clone();
        let mut classes = stubs_base.classes.clone();
        let mut aliases = stubs_base.aliases.clone();
        let mut scope0_symbols = stubs_base.scope0_symbols.clone();
        let framexml_scope0_symbols = stubs_base.framexml_scope0_symbols.clone();
        let mut symbol_locations = stubs_base.symbol_locations.clone();
        let mut function_locations = stubs_base.function_locations.clone();
        let mut string_values = stubs_base.string_values.clone();
        let mut number_values = stubs_base.number_values.clone();
        let mut addon_table_idx = stubs_base.addon_table_idx;

        // Dummy SyntaxNodePtr
        let mut parser = crate::syntax::syntax::Generator::new("--");
        let green = parser.process_all();
        let root = crate::syntax::syntax::SyntaxNode::new_root(green);
        let dummy_node = SyntaxNodePtr::new(&root);

        // ── Process workspace classes ──────────────────────────────────────
        // Register new class names (skip duplicates already in stubs)
        for class in ws_classes {
            if classes.contains_key(&class.name) { continue; }
            let table_idx = EXT_BASE + tables.len();
            let accessors = class.accessors.iter().cloned().collect();
            tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class.name.clone()),
                class_type_params: class.type_params.clone(),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
                key_type: None,
                value_type: None,
                accessors,
                call_func: None,
                constructors: class.constructor_methods.iter().cloned().collect(),
                built_table: None,
                is_enum: class.is_enum,
            });
            classes.insert(class.name.clone(), table_idx);
        }

        // Register workspace aliases before populating fields so alias types
        // are available during field type resolution.
        for alias in ws_aliases {
            if let Some(vt) = Self::resolve_annotation(&alias.typ, &classes, &aliases) {
                aliases.insert(alias.name.clone(), vt);
            }
        }

        // Populate @field entries for workspace classes
        for class in ws_classes {
            let table_idx = classes[&class.name];
            let local_idx = table_idx - EXT_BASE;
            for (field_name, annotation_type, visibility) in &class.fields {
                if field_name == "[string]" || field_name == "[number]" {
                    if let Some(vt) = Self::resolve_annotation(annotation_type, &classes, &aliases) {
                        if field_name == "[string]" {
                            tables[local_idx].key_type = Some(ValueType::String(None));
                        } else {
                            tables[local_idx].key_type = Some(ValueType::Number);
                        }
                        tables[local_idx].value_type = Some(vt);
                    }
                    continue;
                }
                let vt = if let AnnotationType::Simple(name) = annotation_type {
                    if let Some(sig) = parse_overload(name) {
                        let func_idx = Self::build_function(
                            &sig.params, &sig.returns, &[], None,
                            false, false, None, None, &[],
                            None, None, false, None, false,
                            dummy_node, &mut scopes, &mut symbols, &mut functions,
                            &mut tables, &classes, &aliases,
                        );
                        Some(ValueType::Function(Some(func_idx)))
                    } else {
                        Self::resolve_annotation(annotation_type, &classes, &aliases)
                    }
                } else {
                    Self::resolve_annotation(annotation_type, &classes, &aliases)
                };
                if let Some(vt) = vt {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text: None,
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                } else if annotation_type_references_type_params(annotation_type, &tables[local_idx].class_type_params) {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(ValueType::Nil));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: None,
                        annotation_text: None,
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Build call functions from @overload on workspace @class declarations
        for class in ws_classes {
            if class.overloads.is_empty() { continue; }
            let table_idx = classes[&class.name];
            let local_idx = table_idx - EXT_BASE;
            let overload = &class.overloads[0];
            let func_idx = Self::build_function(
                &overload.params, &overload.returns, &[], None,
                false, false, None, None, &class.generics,
                None, None, false, None, false,
                dummy_node, &mut scopes, &mut symbols, &mut functions,
                &mut tables, &classes, &aliases,
            );
            tables[local_idx].call_func = Some(func_idx);
        }

        // ── Process workspace globals ──────────────────────────────────────
        let mut non_class_tables: HashMap<String, TableIndex> = HashMap::new();
        let mut table_source_locations: HashMap<String, ExternalLocation> = HashMap::new();
        let mut class_globals: HashSet<String> = HashSet::new();

        for g in ws_globals {
            if let ExternalGlobalKind::Table = &g.kind {
                if classes.contains_key(&g.name) {
                    class_globals.insert(g.name.clone());
                    if let Some(path) = &g.source_path {
                        table_source_locations.insert(g.name.clone(), ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end,
                        });
                    }
                } else if !non_class_tables.contains_key(&g.name) {
                    // Check if stubs already registered this as a scope0 symbol
                    if stubs_base.scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone()))
                        || stubs_base.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) {
                        continue;
                    }
                    let table_idx = EXT_BASE + tables.len();
                    tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                    non_class_tables.insert(g.name.clone(), table_idx);
                    if let Some(path) = &g.source_path {
                        table_source_locations.insert(g.name.clone(), ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end,
                        });
                    }
                }
            }
        }

        // Create/extend addon namespace table for workspace globals
        if ws_globals.iter().any(|g| g.name == crate::annotations::ADDON_NS_NAME) {
            if addon_table_idx.is_none() {
                let table_idx = EXT_BASE + tables.len();
                tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                non_class_tables.insert(crate::annotations::ADDON_NS_NAME.to_string(), table_idx);
                addon_table_idx = Some(table_idx);
            } else {
                non_class_tables.insert(crate::annotations::ADDON_NS_NAME.to_string(), addon_table_idx.unwrap());
            }
        }

        // Auto-create tables for workspace method/field targets
        for g in ws_globals {
            let target_name = match &g.kind {
                ExternalGlobalKind::Method(_, _) | ExternalGlobalKind::TableField(_, _) | ExternalGlobalKind::NestedMethod(_, _, _) => &g.name,
                _ => continue,
            };
            if classes.contains_key(target_name) || non_class_tables.contains_key(target_name) {
                continue;
            }
            let table_idx = EXT_BASE + tables.len();
            tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(target_name.clone()),
                class_type_params: Vec::new(),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
                key_type: None,
                value_type: None,
                accessors: HashMap::new(),
                call_func: None,
                constructors: HashSet::new(),
                built_table: None,
                is_enum: false,
            });
            classes.insert(target_name.clone(), table_idx);
        }

        // Build workspace method function entries
        let mut seen_methods: HashSet<(&str, &str)> = HashSet::new();
        for g in ws_globals {
            if let ExternalGlobalKind::Method(method_name, is_colon) = &g.kind {
                let target_table = classes.get(&g.name).or_else(|| non_class_tables.get(&g.name));
                let Some(&table_idx) = target_table else { continue; };
                if !seen_methods.insert((&g.name, method_name)) { continue; }

                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, *is_colon,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &mut tables, &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    function_locations.insert(func_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
                let expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));

                let local_idx = table_idx - EXT_BASE;
                let accessor_vis = if !g.intermediates.is_empty() {
                    let mut vis = None;
                    for iname in &g.intermediates {
                        if let Some(&v) = tables[local_idx].accessors.get(iname.as_str()) {
                            vis = Some(v);
                            break;
                        }
                    }
                    if vis.is_none() {
                        if let Some(ref class_name) = tables[local_idx].class_name {
                            // Check workspace classes first, then stubs
                            let parent_names = ws_classes.iter()
                                .find(|c| c.name == *class_name)
                                .map(|c| &c.parents);
                            if let Some(parent_names) = parent_names {
                                for pname in parent_names {
                                    if let Some(&pidx) = classes.get(pname.as_str()) {
                                        let plocal = pidx - EXT_BASE;
                                        for iname in &g.intermediates {
                                            if let Some(&v) = tables[plocal].accessors.get(iname.as_str()) {
                                                vis = Some(v);
                                                break;
                                            }
                                        }
                                        if vis.is_some() { break; }
                                    }
                                }
                            }
                        }
                    }
                    vis
                } else { None };
                let visibility = accessor_vis.unwrap_or(g.visibility);
                tables[local_idx].fields.entry(method_name.clone()).or_insert(FieldInfo {
                    expr: expr_id,
                    visibility,
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    extra_exprs: Vec::new(),
                });
                if g.constructor {
                    functions[func_idx].constructor = true;
                    tables[local_idx].constructors.insert(method_name.clone());
                }
            }
        }

        // Build workspace table field entries
        let mut sub_tables: HashMap<(String, String), TableIndex> = HashMap::new();
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(field_name, value_kind) = &g.kind {
                if matches!(value_kind, FieldValueKind::Unknown) && g.returns.is_empty() { continue; }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                let value_type = if !g.returns.is_empty() {
                    Self::resolve_annotation(&g.returns[0], &classes, &aliases)
                } else {
                    match value_kind {
                        FieldValueKind::String => Some(ValueType::String(None)),
                        FieldValueKind::Number => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table => {
                            let sub_idx = EXT_BASE + tables.len();
                            tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                            sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        FieldValueKind::FunctionCall(..) => None,
                        FieldValueKind::FieldRef(_) => None,
                        FieldValueKind::Unknown => unreachable!(),
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name),
                        annotation,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }
        // Second pass: resolve Unknown fields
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(field_name, value_kind) = &g.kind {
                if !matches!(value_kind, FieldValueKind::Unknown) || !g.returns.is_empty() { continue; }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                let value_type = if let Some(&idx) = classes.get(field_name) {
                    ValueType::Table(Some(idx))
                } else if let Some(&sub_idx) = sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), field_name.clone())) {
                    ValueType::Table(Some(sub_idx))
                } else {
                    ValueType::Table(None)
                };
                let expr_idx = EXT_BASE + exprs.len();
                exprs.push(Expr::Literal(value_type.clone()));
                tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                    expr: expr_idx,
                    visibility: crate::annotations::default_visibility_for_name(field_name),
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    extra_exprs: Vec::new(),
                });
            }
        }

        // Build workspace nested method entries
        for g in ws_globals {
            if let ExternalGlobalKind::NestedMethod(sub_field, method_name, is_colon) = &g.kind {
                let Some(&sub_idx) = sub_tables.get(&(g.name.clone(), sub_field.clone())) else { continue };
                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, *is_colon,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &mut tables, &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    function_locations.insert(func_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
                let expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));
                let local_idx = sub_idx - EXT_BASE;
                tables[local_idx].fields.entry(method_name.clone()).or_insert(FieldInfo {
                    expr: expr_id,
                    visibility: g.visibility,
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    extra_exprs: Vec::new(),
                });
            }
        }

        // Resolve inheritance for workspace classes via topological sort.
        // Without topo sort, a child processed before its parent would miss
        // transitive ancestors (e.g. DestroyingScrollTable → ScrollTable → List → Element).
        {
            let mut ws_class_index: HashMap<&str, usize> = HashMap::new();
            for (i, class) in ws_classes.iter().enumerate() {
                ws_class_index.insert(&class.name, i);
            }
            let mut children_of: HashMap<&str, Vec<usize>> = HashMap::new();
            let mut in_degree: Vec<usize> = vec![0; ws_classes.len()];
            for (i, class) in ws_classes.iter().enumerate() {
                for parent_name in &class.parents {
                    // Only count in-degree for parents that are also workspace classes
                    if ws_class_index.contains_key(parent_name.as_str()) {
                        children_of.entry(parent_name.as_str()).or_default().push(i);
                        in_degree[i] += 1;
                    }
                }
            }
            // Kahn's algorithm
            let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
            for (i, &deg) in in_degree.iter().enumerate() {
                if deg == 0 { queue.push_back(i); }
            }
            let mut order: Vec<usize> = Vec::with_capacity(ws_classes.len());
            let mut processed_names: HashSet<&str> = HashSet::new();
            while let Some(idx) = queue.pop_front() {
                let name = ws_classes[idx].name.as_str();
                if !processed_names.insert(name) { continue; }
                order.push(idx);
                if let Some(kids) = children_of.get(name) {
                    for &kid in kids {
                        in_degree[kid] = in_degree[kid].saturating_sub(1);
                        if in_degree[kid] == 0 { queue.push_back(kid); }
                    }
                }
            }
            // Append any remaining (cycles)
            for i in 0..ws_classes.len() {
                if in_degree[i] > 0 && processed_names.insert(ws_classes[i].name.as_str()) {
                    order.push(i);
                }
            }
            // Compute transitive parent_classes in topo order
            for &idx in &order {
                let class = &ws_classes[idx];
                if class.parents.is_empty() { continue; }
                let Some(&child_table_idx) = classes.get(class.name.as_str()) else { continue };
                let child_local = child_table_idx - EXT_BASE;
                let mut transitive_parents: Vec<TableIndex> = tables[child_local].parent_classes.clone();
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = classes.get(parent_name.as_str()) {
                        if !transitive_parents.contains(&parent_idx) {
                            transitive_parents.push(parent_idx);
                        }
                        let parent_local = parent_idx - EXT_BASE;
                        for &ancestor in &tables[parent_local].parent_classes {
                            if !transitive_parents.contains(&ancestor) {
                                transitive_parents.push(ancestor);
                            }
                        }
                    }
                }
                tables[child_local].parent_classes = transitive_parents;
            }
        }

        // Pass 3b: constraint type param substitutions for workspace classes
        for class in ws_classes.iter() {
            if class.constraint_type_arg_subs.is_empty() { continue; }
            let child_local = classes[&class.name] - EXT_BASE;
            for (constraint_base, resolved_args) in &class.constraint_type_arg_subs {
                let Some(&parent_idx) = classes.get(constraint_base.as_str()) else { continue };
                let parent_local = parent_idx - EXT_BASE;
                let parent_type_params = tables[parent_local].class_type_params.clone();
                if parent_type_params.is_empty() || parent_type_params.len() != resolved_args.len() {
                    continue;
                }
                let mut subs: HashMap<String, TableIndex> = HashMap::new();
                for (tp, resolved_name) in parent_type_params.iter().zip(resolved_args.iter()) {
                    if let Some(&tidx) = classes.get(resolved_name.as_str()) {
                        subs.insert(tp.clone(), tidx);
                    }
                }
                if subs.is_empty() { continue; }
                let parents = tables[child_local].parent_classes.clone();
                for &pi in &parents {
                    let pi_local = pi - EXT_BASE;
                    let parent_fields: Vec<(String, FieldInfo)> = tables[pi_local].fields.iter()
                        .filter(|(_, fi)| fi.annotation_type_raw.as_ref()
                            .is_some_and(|raw| annotation_type_references_type_params(raw, &parent_type_params)))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (fname, fi) in parent_fields {
                        if tables[child_local].fields.contains_key(&fname) { continue; }
                        let raw = fi.annotation_type_raw.as_ref().unwrap().clone();
                        let substituted = substitute_annotation_type(&raw, &subs, &classes);
                        if let Some(resolved) = crate::annotations::resolve_annotation_type(
                            &substituted, &[], &classes, &aliases,
                        ) {
                            let mut child_fi = fi;
                            child_fi.annotation = Some(resolved);
                            tables[child_local].fields.insert(fname, child_fi);
                        }
                    }
                }
            }
        }

        // Pass 3c: Substitute inherited field types based on field_built_names overrides.
        // (Mirrors the same pass in build().)
        {
            let mut built_extends_parents: Vec<(TableIndex, TableIndex)> = Vec::new();
            let mut class_decls_by_name: HashMap<&str, Vec<usize>> = HashMap::new();
            for (i, c) in ws_classes.iter().enumerate() {
                class_decls_by_name.entry(c.name.as_str()).or_default().push(i);
            }
            for class in ws_classes.iter() {
                if class.field_built_names.is_empty() { continue; }
                let Some(&child_table_idx) = classes.get(class.name.as_str()) else { continue };
                let child_local = child_table_idx - EXT_BASE;
                let mut type_subs: HashMap<String, TableIndex> = HashMap::new();
                let mut ancestor_names: HashSet<String> = HashSet::new();
                let mut queue: Vec<String> = class.parents.clone();
                while let Some(parent_name) = queue.pop() {
                    if !ancestor_names.insert(parent_name.clone()) { continue; }
                    if let Some(&pidx) = classes.get(parent_name.as_str()) {
                        if let Some(cn) = tables[pidx - EXT_BASE].class_name.as_ref() {
                            if ancestor_names.insert(cn.clone()) {
                                queue.push(cn.clone());
                            }
                        }
                        for &gp_idx in &tables[pidx - EXT_BASE].parent_classes {
                            if let Some(gp_cn) = tables[gp_idx - EXT_BASE].class_name.as_ref() {
                                if !ancestor_names.contains(gp_cn) {
                                    queue.push(gp_cn.clone());
                                }
                            }
                        }
                    }
                    if let Some(indices) = class_decls_by_name.get(parent_name.as_str()) {
                        for &idx in indices {
                            for p in &ws_classes[idx].parents {
                                if !ancestor_names.contains(p) {
                                    queue.push(p.clone());
                                }
                            }
                        }
                    }
                }
                for (field_name, child_built) in &class.field_built_names {
                    for ancestor_name in &ancestor_names {
                        if let Some(indices) = class_decls_by_name.get(ancestor_name.as_str()) {
                            for &idx in indices {
                                if let Some(ancestor_built) = ws_classes[idx].field_built_names.get(field_name) {
                                    if ancestor_built != child_built {
                                        if let Some(&new_idx) = classes.get(child_built.as_str()) {
                                            type_subs.insert(ancestor_built.clone(), new_idx);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if type_subs.is_empty() { continue; }
                for (old_class_name, &new_idx) in &type_subs {
                    if let Some(&old_idx) = classes.get(old_class_name.as_str()) {
                        built_extends_parents.push((new_idx, old_idx));
                    }
                }
                let mut fields_to_sub: Vec<(String, FieldInfo)> = Vec::new();
                for (fname, fi) in &tables[child_local].fields {
                    if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                        if *tidx >= EXT_BASE {
                            let tidx_local = *tidx - EXT_BASE;
                            if let Some(old_class_name) = tables[tidx_local].class_name.as_ref() {
                                if type_subs.contains_key(old_class_name) {
                                    fields_to_sub.push((fname.clone(), fi.clone()));
                                }
                            }
                        }
                    }
                }
                let parents = tables[child_local].parent_classes.clone();
                for &pi in &parents {
                    let pi_local = pi - EXT_BASE;
                    for (fname, fi) in &tables[pi_local].fields {
                        if tables[child_local].fields.contains_key(fname) { continue; }
                        if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                            if *tidx >= EXT_BASE {
                                let tidx_local = *tidx - EXT_BASE;
                                if let Some(old_class_name) = tables[tidx_local].class_name.as_ref() {
                                    if type_subs.contains_key(old_class_name) {
                                        fields_to_sub.push((fname.clone(), fi.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
                for (fname, fi) in fields_to_sub {
                    if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                        let tidx_local = *tidx - EXT_BASE;
                        if let Some(old_class_name) = tables[tidx_local].class_name.as_ref() {
                            if let Some(&new_idx) = type_subs.get(old_class_name) {
                                let new_vt = ValueType::Table(Some(new_idx));
                                let new_expr_idx = EXT_BASE + exprs.len();
                                exprs.push(Expr::Literal(new_vt.clone()));
                                let mut child_fi = fi.clone();
                                child_fi.annotation = Some(new_vt);
                                child_fi.expr = new_expr_idx;
                                tables[child_local].fields.insert(fname, child_fi);
                            }
                        }
                    }
                }
            }
            for (new_idx, old_idx) in built_extends_parents {
                let new_local = new_idx - EXT_BASE;
                if !tables[new_local].parent_classes.contains(&old_idx) {
                    tables[new_local].parent_classes.push(old_idx);
                }
            }
        }

        // Build workspace global function entries
        let register_global = |name: &str, resolved_type: Option<ValueType>, symbols: &mut Vec<Symbol>, scope0_symbols: &mut HashMap<SymbolIdentifier, SymbolIndex>| -> SymbolIndex {
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(name.to_string()),
                scope_idx: 0,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type,
                    type_args: Vec::new(),
                    created_in_scope: 0,
                }],
            });
            scope0_symbols.insert(SymbolIdentifier::Name(name.to_string()), sym_idx);
            sym_idx
        };
        let mut seen_functions: HashSet<&str> = HashSet::new();
        for g in ws_globals {
            if let ExternalGlobalKind::Function = &g.kind {
                if !seen_functions.insert(&g.name) { continue; }
                // Skip if already in stubs
                if scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone()))
                    || framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) {
                    continue;
                }

                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, false,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &mut tables, &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    let loc = ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    };
                    function_locations.insert(func_idx, loc.clone());
                    symbol_locations.insert(EXT_BASE + symbols.len(), loc);
                }
                let _expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));

                register_global(&g.name, Some(ValueType::Function(Some(func_idx))), &mut symbols, &mut scope0_symbols);
            }
        }

        // Register workspace simple global variables
        for g in ws_globals {
            if let ExternalGlobalKind::Variable(vk) = &g.kind {
                if scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone()))
                    || framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) {
                    continue;
                }
                if class_globals.contains(&g.name) { continue; }
                let resolved_type = match vk {
                    FieldValueKind::Number => Some(ValueType::Number),
                    FieldValueKind::String => Some(ValueType::String(None)),
                    FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                    FieldValueKind::Nil => Some(ValueType::Nil),
                    _ => None,
                };
                let sym_idx = register_global(&g.name, resolved_type, &mut symbols, &mut scope0_symbols);
                if let Some(ref sv) = g.string_value {
                    string_values.insert(sym_idx, sv.clone());
                }
                if let Some(ref nv) = g.number_value {
                    number_values.insert(sym_idx, nv.clone());
                }
                if let Some(path) = &g.source_path {
                    symbol_locations.insert(sym_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
            }
        }

        // Register workspace non-class tables as scope0 symbols
        for (name, &table_idx) in &non_class_tables {
            let sym_idx = register_global(name, Some(ValueType::Table(Some(table_idx))), &mut symbols, &mut scope0_symbols);
            if let Some(loc) = table_source_locations.get(name) {
                symbol_locations.insert(sym_idx, loc.clone());
            }
        }

        // Register workspace callable class tables and class globals
        for (name, &table_idx) in &classes {
            if scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone()))
                || framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) {
                continue;
            }
            let local_idx = table_idx - EXT_BASE;
            if tables[local_idx].call_func.is_none() && !class_globals.contains(name) { continue; }
            let sym_idx = register_global(name, Some(ValueType::Table(Some(table_idx))), &mut symbols, &mut scope0_symbols);
            if let Some(loc) = table_source_locations.get(name) {
                symbol_locations.insert(sym_idx, loc.clone());
            }
        }

        // Resolve workspace FunctionCall table fields
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(field_name, FieldValueKind::FunctionCall(callee_chain, first_string_arg)) = &g.kind {
                if !g.returns.is_empty() {
                    let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                    let local_idx = table_idx - EXT_BASE;
                    if tables[local_idx].fields.contains_key(field_name) { continue; }
                    if let Some(vt) = Self::resolve_annotation(&g.returns[0], &classes, &aliases) {
                        let expr_idx = EXT_BASE + exprs.len();
                        exprs.push(Expr::Literal(vt.clone()));
                        tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name),
                            annotation: Some(vt),
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            extra_exprs: Vec::new(),
                        });
                    }
                    continue;
                }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                let return_type = Self::resolve_funcall_chain(
                    callee_chain, &tables, &exprs, &functions,
                    &non_class_tables, &classes, &scope0_symbols, &symbols,
                );
                let return_type = return_type.filter(|vt| !matches!(vt, ValueType::TypeVariable(_)));
                let vt = return_type.or_else(|| {
                    first_string_arg.as_ref()
                        .and_then(|name| classes.get(name.as_str()))
                        .map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    classes.get(field_name).map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    if g.name == crate::annotations::ADDON_NS_NAME {
                        let sub_idx = EXT_BASE + tables.len();
                        tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                        sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                        Some(ValueType::Table(Some(sub_idx)))
                    } else {
                        None
                    }
                });
                if let Some(vt) = vt {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name),
                        annotation: None,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Resolve workspace FieldRef table fields
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(field_name, FieldValueKind::FieldRef(ref_chain)) = &g.kind {
                if !g.returns.is_empty() { continue; }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                let source_table_idx = non_class_tables.get(&ref_chain[0])
                    .or_else(|| classes.get(&ref_chain[0]))
                    .or_else(|| sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), ref_chain[0].clone())));
                if let Some(&mut_src_idx) = source_table_idx {
                    let mut current = mut_src_idx;
                    let mut resolved = None;
                    for (i, name) in ref_chain[1..].iter().enumerate() {
                        let src_local = current - EXT_BASE;
                        if let Some(fi) = tables[src_local].fields.get(name) {
                            if i == ref_chain.len() - 2 {
                                if let Some(ref ann) = fi.annotation {
                                    resolved = Some(ann.clone());
                                } else {
                                    let expr = &exprs[fi.expr - EXT_BASE];
                                    if let Expr::Literal(vt) = expr {
                                        resolved = Some(vt.clone());
                                    }
                                }
                            } else {
                                if let Some(ref ann) = fi.annotation {
                                    if let ValueType::Table(Some(idx)) = ann {
                                        current = *idx;
                                        continue;
                                    }
                                }
                                let expr = &exprs[fi.expr - EXT_BASE];
                                if let Expr::Literal(ValueType::Table(Some(idx))) = expr {
                                    current = *idx;
                                } else {
                                    break;
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    if let Some(vt) = resolved {
                        let expr_idx = EXT_BASE + exprs.len();
                        exprs.push(Expr::Literal(vt.clone()));
                        tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name),
                            annotation: None,
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            extra_exprs: Vec::new(),
                        });
                    }
                }
            }
        }

        // Register addon sub-tables and re-process
        for ((parent, field), &idx) in &sub_tables {
            if parent == crate::annotations::ADDON_NS_NAME {
                non_class_tables.entry(field.clone()).or_insert(idx);
            }
        }
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(field_name, value_kind) = &g.kind {
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                let value_type = if !g.returns.is_empty() {
                    Self::resolve_annotation(&g.returns[0], &classes, &aliases)
                } else {
                    match value_kind {
                        FieldValueKind::String => Some(ValueType::String(None)),
                        FieldValueKind::Number => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table => {
                            let sub_idx = EXT_BASE + tables.len();
                            tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                            sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        _ => None,
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name),
                        annotation,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Register workspace field-ref globals
        for g in ws_globals {
            if let ExternalGlobalKind::FieldRef(table_name, field_name) = &g.kind {
                if scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone()))
                    || framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) {
                    continue;
                }
                let table_local_idx = non_class_tables.get(table_name)
                    .or_else(|| classes.get(table_name))
                    .map(|idx| idx - EXT_BASE);
                if let Some(local_idx) = table_local_idx {
                    if let Some(field) = tables[local_idx].fields.get(field_name) {
                        let resolved_type = match &exprs[field.expr - EXT_BASE] {
                            Expr::FunctionDef(func_idx) => Some(ValueType::Function(Some(*func_idx))),
                            _ => None,
                        };
                        if let Some(resolved_type) = resolved_type {
                            let sym_idx = register_global(&g.name, Some(resolved_type), &mut symbols, &mut scope0_symbols);
                            if let Some(path) = &g.source_path {
                                symbol_locations.insert(sym_idx, ExternalLocation {
                                    path: path.clone(), start: g.def_start, end: g.def_end,
                                });
                            }
                        }
                    }
                }
            }
        }

        PreResolvedGlobals {
            scopes, symbols, functions, exprs, tables,
            classes, aliases, scope0_symbols, framexml_scope0_symbols,
            symbol_locations, function_locations, string_values, number_values,
            addon_table_idx,
        }
    }

    pub(crate) fn resolve_annotation(
        at: &AnnotationType,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
    ) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, &[], classes, aliases)
    }

    fn resolve_annotation_gen(
        at: &AnnotationType,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
        generics: &[(String, Option<String>)],
        tables: &mut Vec<TableInfo>,
    ) -> Option<ValueType> {
        // Handle Array types (e.g. T[], string[]) by materializing a TableInfo
        if let AnnotationType::Array(inner) = at {
            if let Some(elem_vt) = Self::resolve_annotation_gen(inner, classes, aliases, generics, tables) {
                let table_idx = EXT_BASE + tables.len();
                tables.push(TableInfo {
                    fields: HashMap::new(),
                    class_name: None,
                    parent_classes: Vec::new(),
                    array_fields: Vec::new(),
                    key_type: Some(ValueType::Number),
                    value_type: Some(elem_vt),
                    accessors: HashMap::new(),
                    call_func: None,
                    class_type_params: Vec::new(),
                    constructors: HashSet::new(),
                    built_table: None,
                    is_enum: false,
                });
                return Some(ValueType::Table(Some(table_idx)));
            }
            return Some(ValueType::Table(None));
        }
        crate::annotations::resolve_annotation_type(at, generics, classes, aliases)
    }

    /// Create a Function entry from an inline fun() annotation type.
    fn materialize_fun_type(
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        is_vararg: bool,
        generics: &[(String, Option<String>)],
        dummy_node: SyntaxNodePtr,
        scopes: &mut Vec<Scope>,
        symbols: &mut Vec<Symbol>,
        functions: &mut Vec<Function>,
        tables: &mut Vec<TableInfo>,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
    ) -> ValueType {
        let func_scope_local = scopes.len();
        let func_scope = EXT_BASE + func_scope_local;
        scopes.push(Scope { parent: Some(0), symbols: HashMap::new() });

        let mut arg_symbols = Vec::new();
        let mut param_annotations = Vec::new();
        let mut param_optional = Vec::new();
        for p in params {
            if p.name == "..." { continue; }
            let resolved = Self::resolve_annotation_gen(&p.typ, classes, aliases, generics, tables)
                .map(|vt| if p.optional { ValueType::union(vt, ValueType::Nil) } else { vt });
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(p.name.clone()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion { def_node: dummy_node, type_source: None, resolved_type: resolved, type_args: Vec::new(), created_in_scope: func_scope }],
            });
            scopes[func_scope_local].symbols.insert(SymbolIdentifier::Name(p.name.clone()), sym_idx);
            arg_symbols.push(sym_idx);
            param_annotations.push(p.typ.clone());
            param_optional.push(p.optional);
        }

        let func_idx = EXT_BASE + functions.len();
        let return_annotations: Vec<ValueType> = returns.iter()
            .filter_map(|rt| Self::resolve_annotation_gen(rt, classes, aliases, generics, tables))
            .collect();
        functions.push(Function {
            def_node: dummy_node,
            scope: func_scope,
            args: arg_symbols,
            rets: Vec::new(),
            return_annotations,
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            generics: Vec::new(),
            generic_constraints_raw: Vec::new(),
            param_annotations: param_annotations.iter().map(|at| at.clone()).collect(),
            param_descriptions: Vec::new(),
            defclass: None,
            defclass_parent: None,
            is_vararg,
            param_optional,
            returns_self: false,
            explicit_void_return: returns.is_empty(),
            constructor: false,
            builds_field: None,
            built_name: None,
            built_extends: false,
            returns_built: false,
            returns_built_parent: None,
            dot_defined: false,
            type_narrows: None,
        });
        ValueType::Function(Some(func_idx))
    }

    /// Walk a callee chain (e.g. ["__addon_ns__", "Bar", "NewComponent"]) through
    /// the built tables/functions to find the return type of the function at the end.
    fn resolve_funcall_chain(
        chain: &[String],
        tables: &[TableInfo],
        exprs: &[Expr],
        functions: &[Function],
        non_class_tables: &HashMap<String, TableIndex>,
        classes: &HashMap<String, TableIndex>,
        scope0_symbols: &HashMap<SymbolIdentifier, SymbolIndex>,
        symbols: &[Symbol],
    ) -> Option<ValueType> {
        if chain.is_empty() { return None; }

        // Single-name chain: global function call like CreateFrame()
        if chain.len() == 1 {
            let sym_id = SymbolIdentifier::Name(chain[0].clone());
            let sym_idx = scope0_symbols.get(&sym_id)?;
            let sym = &symbols[sym_idx - EXT_BASE];
            let vt = sym.versions.last()?.resolved_type.as_ref()?;
            if let ValueType::Function(Some(func_idx)) = vt {
                return functions[func_idx - EXT_BASE].return_annotations.first().cloned();
            }
            return None;
        }

        // Multi-name chain: walk tables to find the function
        // Start from the root table
        let root = &chain[0];
        let mut current_table = *non_class_tables.get(root)
            .or_else(|| classes.get(root))?;

        // Walk intermediate names (all but last) as table fields
        for name in &chain[1..chain.len()-1] {
            let local_idx = current_table - EXT_BASE;
            let field = tables[local_idx].fields.get(name)?;
            let expr = &exprs[field.expr - EXT_BASE];
            match expr {
                Expr::Literal(ValueType::Table(Some(idx))) => { current_table = *idx; }
                _ => {
                    // Also check annotation for the table type
                    if let Some(ValueType::Table(Some(idx))) = &field.annotation {
                        current_table = *idx;
                    } else {
                        return None;
                    }
                }
            }
        }

        // Last name should be a function on the current table
        let func_name = &chain[chain.len()-1];
        let local_idx = current_table - EXT_BASE;
        let field = tables[local_idx].fields.get(func_name)?;
        let expr = &exprs[field.expr - EXT_BASE];
        if let Expr::FunctionDef(func_idx) = expr {
            functions[func_idx - EXT_BASE].return_annotations.first().cloned()
        } else {
            None
        }
    }

    /// Build a Function entry. All returned indices use EXT_BASE so they're
    /// directly usable in the global index space without per-file adjustment.
    fn build_function(
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        overload_sigs: &[crate::annotations::OverloadSig],
        doc: Option<String>,
        deprecated: bool,
        nodiscard: bool,
        defclass: Option<String>,
        defclass_parent: Option<String>,
        generic_annotations: &[(String, Option<String>)],
        builds_field_raw: Option<&(usize, AnnotationType)>,
        built_name_raw: Option<usize>,
        built_extends: bool,
        type_narrows_raw: Option<(usize, usize)>,
        is_colon: bool,
        dummy_node: SyntaxNodePtr,
        scopes: &mut Vec<Scope>,
        symbols: &mut Vec<Symbol>,
        functions: &mut Vec<Function>,
        tables: &mut Vec<TableInfo>,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
    ) -> FunctionIndex {
        let func_scope_local = scopes.len();
        let func_scope = EXT_BASE + func_scope_local;
        scopes.push(Scope {
            parent: Some(0),
            symbols: HashMap::new(),
        });

        let mut arg_symbols = Vec::new();
        // Inject implicit self param for colon-defined methods, matching
        // insert_function_definition in build_ir.rs.  Without this, dot-calls
        // to stub colon methods (e.g. GameTooltip.Show(frame)) would report a
        // false-positive redundant-parameter diagnostic.
        if is_colon {
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name("self".to_string()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: None,
                    type_args: Vec::new(),
                    created_in_scope: func_scope,
                }],
            });
            scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::Name("self".to_string()), sym_idx,
            );
            arg_symbols.push(sym_idx);
        }
        let mut has_vararg_param = false;
        for p in params {
            if p.name == "..." {
                has_vararg_param = true;
                continue;
            }
            let resolved = Self::resolve_annotation_gen(&p.typ, classes, aliases, generic_annotations, tables)
                .map(|vt| if p.optional { ValueType::union(vt, ValueType::Nil) } else { vt });
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(p.name.clone()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: resolved,
                    type_args: Vec::new(),
                    created_in_scope: func_scope,
                }],
            });
            scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::Name(p.name.clone()), sym_idx,
            );
            arg_symbols.push(sym_idx);
        }

        let returns_self = returns.iter().any(|rt| matches!(rt, AnnotationType::Simple(s) if s == "self"));
        let returns_built_entry = returns.iter().find(|rt| matches!(rt, AnnotationType::Simple(s) if s == "built" || s.starts_with("built:")));
        let returns_built = returns_built_entry.is_some();
        let returns_built_parent = returns_built_entry.and_then(|rt| {
            if let AnnotationType::Simple(s) = rt {
                s.strip_prefix("built:").map(|p| p.to_string())
            } else { None }
        });
        let non_self_returns: Vec<&AnnotationType> = returns.iter()
            .filter(|rt| !matches!(rt, AnnotationType::Simple(s) if s == "self" || s == "built" || s.starts_with("built:")))
            .collect();
        let return_annotations: Vec<ValueType> = non_self_returns.iter()
            .filter_map(|rt| {
                if let AnnotationType::Fun(inner_params, inner_returns, inner_vararg) = rt {
                    Some(Self::materialize_fun_type(
                        inner_params, inner_returns, *inner_vararg, generic_annotations,
                        dummy_node, scopes, symbols, functions, tables, classes, aliases,
                    ))
                } else {
                    Self::resolve_annotation_gen(rt, classes, aliases, generic_annotations, tables)
                }
            })
            .collect();

        // Build overloads BEFORE computing func_idx, since materialize_fun_type
        // may push new Function entries that would shift the index.
        let overloads: Vec<ResolvedOverload> = overload_sigs.iter().map(|sig| {
            let params = sig.params.iter().map(|p| {
                let vt = if let AnnotationType::Fun(inner_params, inner_returns, inner_vararg) = &p.typ {
                    Some(Self::materialize_fun_type(
                        inner_params, inner_returns, *inner_vararg, generic_annotations,
                        dummy_node, scopes, symbols, functions, tables, classes, aliases,
                    ))
                } else {
                    Self::resolve_annotation_gen(&p.typ, classes, aliases, generic_annotations, tables)
                };
                crate::types::ResolvedOverloadParam {
                    name: p.name.clone(),
                    typ: vt,
                    optional: p.optional,
                }
            }).collect();
            let returns = sig.returns.iter()
                .filter_map(|at| {
                    if let AnnotationType::Fun(inner_params, inner_returns, inner_vararg) = at {
                        Some(Self::materialize_fun_type(
                            inner_params, inner_returns, *inner_vararg, generic_annotations,
                            dummy_node, scopes, symbols, functions, tables, classes, aliases,
                        ))
                    } else {
                        Self::resolve_annotation_gen(at, classes, aliases, generic_annotations, tables)
                    }
                })
                .collect();
            ResolvedOverload { params, returns, is_return_only: sig.is_return_only }
        }).collect();

        let func_idx = EXT_BASE + functions.len();
        let mut ret_symbols = Vec::new();
        for (i, _rt) in non_self_returns.iter().enumerate() {
            let resolved = return_annotations.get(i).cloned();
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::FunctionRet(func_idx, i),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: resolved,
                    type_args: Vec::new(),
                    created_in_scope: func_scope,
                }],
            });
            scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::FunctionRet(func_idx, i), sym_idx,
            );
            ret_symbols.push(sym_idx);
        }

        // Resolve generic constraints
        // Handle parameterized constraints like "BaseClass<P>" — resolve base name only
        let resolved_generics: Vec<(String, Option<ValueType>)> = generic_annotations.iter().map(|(name, constraint)| {
            let resolved_constraint = constraint.as_ref().and_then(|c| {
                let base_name = c.split('<').next().unwrap_or(c);
                Self::resolve_annotation(&AnnotationType::Simple(base_name.to_string()), classes, aliases)
            });
            (name.clone(), resolved_constraint)
        }).collect();

        // Detect vararg from overloads or @param ...
        let is_vararg = has_vararg_param || overload_sigs.iter().any(|s| s.is_vararg);

        // Build param_optional vec from ParamInfo (excluding vararg)
        let non_vararg_params = params.iter().filter(|p| p.name != "...");
        let mut param_optional_vec: Vec<bool> = non_vararg_params.clone().map(|p| p.optional).collect();
        let mut param_descriptions_vec: Vec<Option<String>> = non_vararg_params.clone().map(|p| p.description.clone()).collect();
        let mut param_annotations_vec: Vec<AnnotationType> = non_vararg_params.map(|p| p.typ.clone()).collect();
        // Prepend self entry for colon methods (matching the injected self in arg_symbols)
        if is_colon {
            param_optional_vec.insert(0, false);
            param_descriptions_vec.insert(0, None);
            param_annotations_vec.insert(0, AnnotationType::Simple(String::new()));
        }

        functions.push(Function {
            def_node: dummy_node,
            scope: func_scope,
            args: arg_symbols,
            rets: ret_symbols,
            return_annotations,
            overloads,
            doc,
            deprecated,
            nodiscard,
            generics: resolved_generics,
            generic_constraints_raw: generic_annotations.to_vec(),
            param_annotations: param_annotations_vec,
            param_descriptions: param_descriptions_vec,
            defclass,
            defclass_parent,
            is_vararg,
            param_optional: param_optional_vec,
            returns_self,
            explicit_void_return: false, constructor: false,
            builds_field: builds_field_raw.and_then(|(idx, at)| {
                Self::resolve_annotation_gen(at, classes, aliases, generic_annotations, tables)
                    .map(|vt| (*idx, vt))
            }),
            built_name: built_name_raw,
            built_extends,
            returns_built,
            returns_built_parent,
            dot_defined: !is_colon,
            type_narrows: type_narrows_raw,
        });

        func_idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{ClassDecl, AnnotationType};

    fn make_class(name: &str, parents: &[&str], fields: &[(&str, &str)]) -> ClassDecl {
        ClassDecl {
            name: name.to_string(),
            type_params: Vec::new(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            fields: fields.iter().map(|(n, t)| {
                (n.to_string(), AnnotationType::Simple(t.to_string()), crate::annotations::default_visibility_for_name(n))
            }).collect(),
            accessors: Vec::new(),
            overloads: Vec::new(),
            generics: Vec::new(),
            constructor_methods: Vec::new(),
            constraint_type_arg_subs: Vec::new(),
            field_built_names: std::collections::HashMap::new(),
            is_enum: false,
        }
    }

    #[test]
    fn build_on_stubs_deep_workspace_inheritance() {
        // Regression test: build_on_stubs must use topological sort for workspace
        // class inheritance, otherwise children processed before parents miss
        // transitive ancestors (e.g. D → C → B → A, D only gets C).
        let stubs_base = PreResolvedGlobals::empty();

        // Deliberately order classes child-first to expose the bug
        let ws_classes = vec![
            make_class("D", &["C"], &[("dField", "string")]),
            make_class("C", &["B"], &[("cField", "string")]),
            make_class("B", &["A"], &[("bField", "string")]),
            make_class("A", &[], &[("aField", "string")]),
        ];

        let result = PreResolvedGlobals::build_on_stubs(
            &stubs_base, &[], &ws_classes, &[],
        );

        let d_idx = result.classes["D"];
        let c_idx = result.classes["C"];
        let b_idx = result.classes["B"];
        let a_idx = result.classes["A"];

        let d_parents = &result.tables[d_idx - EXT_BASE].parent_classes;
        assert!(d_parents.contains(&c_idx), "D should have C as parent");
        assert!(d_parents.contains(&b_idx), "D should have B as ancestor");
        assert!(d_parents.contains(&a_idx), "D should have A as ancestor");

        let c_parents = &result.tables[c_idx - EXT_BASE].parent_classes;
        assert!(c_parents.contains(&b_idx), "C should have B as parent");
        assert!(c_parents.contains(&a_idx), "C should have A as ancestor");
    }

    #[test]
    fn build_on_stubs_field_built_names_substitution() {
        // Regression test: build_on_stubs must substitute inherited field types
        // based on field_built_names overrides (Pass 3c). When a child class
        // overrides a parent's @built-name for a field (e.g. _STATE_SCHEMA),
        // inherited constructor fields (e.g. _state) that reference the parent's
        // built class should be substituted with the child's.
        let stubs_base = PreResolvedGlobals::empty();

        let mut parent = make_class("Element", &[], &[("_state", "ElementState")]);
        parent.field_built_names.insert("_STATE_SCHEMA".to_string(), "ElementState".to_string());

        let mut child = make_class("ItemList", &["Element"], &[]);
        child.field_built_names.insert("_STATE_SCHEMA".to_string(), "ItemListState".to_string());

        let elem_state = make_class("ElementState", &[], &[("acquired", "boolean")]);
        let item_list_state = make_class("ItemListState", &["ElementState"], &[("moveText", "string")]);

        let ws_classes = vec![parent, child, elem_state, item_list_state];
        let result = PreResolvedGlobals::build_on_stubs(
            &stubs_base, &[], &ws_classes, &[],
        );

        let item_list_idx = result.classes["ItemList"];
        let item_list_local = item_list_idx - EXT_BASE;
        let state_field = result.tables[item_list_local].fields.get("_state")
            .expect("ItemList should have _state field from inheritance substitution");
        if let Some(ValueType::Table(Some(tidx))) = &state_field.annotation {
            let class_name = result.tables[*tidx - EXT_BASE].class_name.as_deref();
            assert_eq!(class_name, Some("ItemListState"),
                "_state should be substituted to ItemListState, got {:?}", class_name);
        } else {
            panic!("_state should have Table annotation, got {:?}", state_field.annotation);
        }
    }
}
