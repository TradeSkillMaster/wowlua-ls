use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::annotations::{AnnotationType, parse_overload, scan_all_annotations};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::*;
use super::Analysis;

// ── Annotation Pre-scan (Phase 0) ─────────────────────────────────────────────

#[derive(Debug)]
struct FunReturnInfo {
    return_annotations: Vec<ValueType>,
    return_annotations_raw: Vec<AnnotationType>,
    return_labels: Vec<Option<String>>,
    ret_symbols: Vec<SymbolIndex>,
    overloads: Vec<ResolvedOverload>,
}

/// Return type and parameter types extracted from a function argument,
/// for generic inference in the `Fun` arm of `infer_generics_from_annotation`.
pub(super) struct ArgFunctionTypeInfo {
    pub(super) ret: Option<ValueType>,
    pub(super) params: Option<Vec<ValueType>>,
}

impl ArgFunctionTypeInfo {
    const EMPTY: Self = Self { ret: None, params: None };
}

#[derive(Debug)]
struct DefclassFuncInfo {
    generic_name: String,
    constraint_table: Option<TableIndex>,
    parent_param_idx: Option<usize>,
    constraint_raw: Option<String>,
    parent_generic_name: Option<String>,
    param_annotations: Option<Vec<crate::annotations::AnnotationType>>,
    /// Call-argument positions (0-based, self excluded) where the backtick
    /// class-name string may appear — covers the primary signature and all overloads.
    backtick_param_positions: Vec<usize>,
}

/// Collect the call-argument positions (0-based, self excluded) at which the
/// backtick class-name string may appear for a `@defclass`-annotated function.
/// Covers both the primary signature (`func.param_annotations`) and all overloads
/// (`func.overloads`).
///
/// # Self-offset accounting
///
/// For colon-syntax functions, `pre_globals::build_function` prepends a synthetic
/// self entry at `param_annotations[0]`:
/// - `Simple("")` for non-generic class methods
/// - `Parameterized(class_name, type_params)` for methods on generic classes
///
/// Neither pattern appears naturally as a non-self first parameter, so when
/// `is_method_call` is true and the first annotation matches either pattern we
/// subtract 1 from annotation indices to obtain call-argument indices.
///
/// Overload params already carry an explicit `"self"` name when self is present,
/// so the overload offset is detected via `p.name == "self"`.
///
/// Positions are returned sorted in ascending call-argument index order.
fn collect_defclass_backtick_positions(
    func: &Function,
    is_method_call: bool,
    generic_name: &str,
) -> Vec<usize> {
    // Determine whether param_annotations[0] is the synthetic self entry.
    let pa_self_offset = usize::from(
        is_method_call
        && func.param_annotations.first().map(|a|
            matches!(a, crate::annotations::AnnotationType::Simple(s) if s.is_empty())
            || matches!(a, crate::annotations::AnnotationType::Parameterized(..))
        ).unwrap_or(false)
    );
    let mut seen = std::collections::BTreeSet::new();
    // Primary signature: check param_annotations[pa_self_offset..].
    for (i, ann) in func.param_annotations.iter().enumerate() {
        if i < pa_self_offset { continue; }
        if crate::annotations::annotation_contains_backtick(ann) {
            seen.insert(i - pa_self_offset);
        }
    }
    // Overloads: params are resolved ValueType; backtick resolves to TypeVariable.
    // We check for TypeVariable(generic_name) rather than AnnotationType::Backtick
    // because ResolvedOverloadParam.typ is already a resolved ValueType.
    for ov in &func.overloads {
        if ov.is_return_only { continue; }
        let ov_self_offset = usize::from(
            is_method_call
            && ov.params.first().map(|p| p.name == "self").unwrap_or(false)
        );
        for (i, param) in ov.params.iter().skip(ov_self_offset)
            .filter(|p| p.name != "...")
            .enumerate()
        {
            let is_bt = match &param.typ {
                Some(ValueType::TypeVariable(n)) => n == generic_name,
                Some(ValueType::Union(types)) => types.iter().any(|t|
                    matches!(t, ValueType::TypeVariable(n) if n == generic_name)
                ),
                _ => false,
            };
            if is_bt { seen.insert(i); }
        }
    }
    seen.into_iter().collect()
}

impl<'a> Analysis<'a> {
    pub(super) fn prescan_classes_and_aliases(&mut self) {
        // Import external classes/aliases from PreResolvedGlobals (cheap map clone)
        let ext = Arc::clone(&self.ir.ext);
        for (name, &table_idx) in &ext.classes {
            self.ir.classes.insert(name.clone(), table_idx);
        }
        for (name, vt) in &ext.aliases {
            self.ir.aliases.insert(name.clone(), vt.clone());
        }

        // Process file-local declarations only
        let scan = scan_all_annotations(self.root());
        self.is_meta = scan.has_meta;

        // Pass 1: Register local class names with empty tables (local indices).
        // Build a parallel Vec mapping scan.classes index → TableIndex so that
        // subsequent passes can look up the correct table for each declaration,
        // even when multiple @class declarations share the same name (which causes
        // ir.classes to overwrite with the last one).
        let mut class_table_indices: Vec<TableIndex> = Vec::with_capacity(scan.classes.len());
        for class in &scan.classes {
            let table_idx = self.ir.tables.len();
            // Inherit constructors from external class if the local annotation doesn't declare any.
            // This preserves @constructor registrations from the workspace scan (e.g. __init from
            // @constructor __init on a base class) that would otherwise be lost when the local
            // @class shadows the external one.
            let mut constructors: std::collections::HashSet<String> = class.constructor_methods.iter().cloned().collect();
            if constructors.is_empty()
                && let Some(&ext_idx) = self.ir.classes.get(&class.name)
            {
                for ctor in &self.ir.table(ext_idx).constructors {
                    constructors.insert(ctor.clone());
                }
            }
            self.ir.tables.push(TableInfo {
                class_name: Some(class.name.clone()),
                class_type_params: class.type_params.clone(),
                class_type_param_constraints: class.type_param_constraints.clone(),
                accessors: class.accessors.iter().cloned().collect(),
                constructors,
                enum_kind: class.initial_enum_kind(),
                is_key_enum: class.is_key_enum,
                correlated_groups: class.correlated_groups.clone(),
                see: class.see.clone(),
                ..Default::default()
            });
            let ti = TableIndex(table_idx);
            class_table_indices.push(ti);
            self.ir.classes.insert(class.name.clone(), ti);
            // Track definition range for local classes
            if let Some((start, end)) = class.def_range {
                self.ir.class_def_ranges.insert(class.name.clone(), (start, end));
                // Positional map for disambiguation when multiple @class share the same name
                self.ir.class_table_by_offset.insert(start, ti);
            }
        }

        // Register local aliases before populating fields so alias types
        // are available during field type resolution.
        for alias in &scan.aliases {
            if !alias.type_params.is_empty() {
                // Parameterized alias: store raw template for later instantiation
                self.ir.parameterized_aliases.insert(
                    alias.name.clone(),
                    (alias.type_params.clone(), alias.typ.clone()),
                );
            } else if crate::annotations::annotation_is_tuple_form(&alias.typ) {
                // Tuple / tuple-union alias — stored raw for use-site expansion in
                // `@return Name` / `fun(): Name` positions. Not registered in the
                // regular aliases map because tuples don't have a single ValueType.
                self.ir.tuple_form_aliases.insert(alias.name.clone(), alias.typ.clone());
            } else if let Some(vt) = self.resolve_annotation_type_mut(&alias.typ) {
                if matches!(&vt, ValueType::Function(None)) {
                    self.ir.alias_fun_types.insert(alias.name.clone(), alias.typ.clone());
                }
                let vt = if alias.is_opaque {
                    ValueType::OpaqueAlias(alias.name.clone(), Box::new(vt))
                } else {
                    vt
                };
                self.ir.aliases.insert(alias.name.clone(), vt);
            }
            if let Some((start, end)) = alias.def_range {
                self.ir.alias_def_ranges.insert(alias.name.clone(), (start, end));
            }
        }

        // Build lookup for local aliases whose resolved type is a function, so that
        // @field declarations using function aliases get proper annotation_text for
        // later materialization into concrete Function entries.
        let fun_alias_types: HashMap<&str, &AnnotationType> = scan.aliases.iter()
            .filter(|a| matches!(self.ir.aliases.get(&a.name), Some(ValueType::Function(None))))
            .map(|a| (a.name.as_str(), &a.typ))
            .collect();

        // Pass 2: Populate local class fields
        for (class_i, class) in scan.classes.iter().enumerate() {
            let table_idx = class_table_indices[class_i];
            for (field_name, annotation_type, visibility) in &class.fields {
                // Handle index signatures: @field [string] Type, @field [number] Type,
                // or @field [K] V where K is a class type param
                if field_name.starts_with('[') && field_name.ends_with(']') {
                    let inner = &field_name[1..field_name.len()-1];
                    let is_string = inner == "string";
                    let is_number = inner == "number";
                    let class_tps = &self.ir.tables[table_idx.val()].class_type_params;
                    let is_type_param = class_tps.iter().any(|tp| tp == inner);
                    if is_string || is_number || is_type_param {
                        let gen_context: Vec<(String, Option<String>)> = self.ir.tables[table_idx.val()].class_type_params.iter()
                            .map(|tp: &String| (tp.clone(), None)).collect();
                        if let Some(vt) = self.resolve_annotation_type_mut_gen(annotation_type, &gen_context) {
                            if is_string {
                                self.ir.tables[table_idx.val()].key_type = Some(ValueType::String(None));
                            } else if is_number {
                                self.ir.tables[table_idx.val()].key_type = Some(ValueType::Number);
                            } else {
                                self.ir.tables[table_idx.val()].key_type = Some(ValueType::TypeVariable(inner.to_string()));
                            }
                            self.ir.tables[table_idx.val()].value_type = Some(vt);
                        }
                        continue;
                    }
                }
                let is_lateinit = matches!(annotation_type, AnnotationType::NonNil(_));
                if let Some(vt) = self.resolve_annotation_type_mut(annotation_type) {
                    let annotation_text = match (&vt, annotation_type) {
                        (ValueType::Function(None), AnnotationType::Simple(s)) if s.starts_with("fun(") => Some(s.clone()),
                        (ValueType::Function(None), AnnotationType::Fun(..)) => Some(crate::annotations::format_annotation_type(annotation_type)),
                        // Preserve T! text for lateinit fields so hover shows the name, not expanded class
                        (_, AnnotationType::NonNil(_)) => Some(crate::annotations::format_annotation_type(annotation_type)),
                        _ => None,
                    };
                    // If annotation_text is still None but the type contains Function(None),
                    // try to resolve through aliases to find the underlying fun(...) type.
                    let annotation_text = annotation_text.or_else(|| {
                        Self::resolve_fun_text_from_alias(annotation_type, &fun_alias_types, &self.ir.ext.alias_fun_types)
                    });
                    let def_range = class.field_ranges.get(field_name.as_str()).copied();
                    let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                    // Store literal from enriched constructor fields for enum hover display
                    if let Some(val) = class.field_literals.get(field_name) {
                        if val.starts_with('"') || val.starts_with('\'') {
                            self.ir.string_literals.insert(expr_id, val.trim_matches(|c| c == '"' || c == '\'').to_string());
                        } else {
                            self.ir.number_literals.insert(expr_id, val.clone());
                        }
                    }
                    self.ir.tables[table_idx.val()].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_id,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text,
                        extra_exprs: Vec::new(),
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: is_lateinit,
                        def_range,
                        flavor_guard: 0,
                        description: class.field_descriptions.get(field_name).cloned(),
                        from_scan: false,
                    });
                } else {
                    let class_tps = &self.ir.tables[table_idx.val()].class_type_params;
                    if !class_tps.is_empty() && crate::pre_globals::annotation_type_references_type_params(annotation_type, class_tps) {
                        let def_range = class.field_ranges.get(field_name.as_str()).copied();
                        let expr_id = self.ir.push_expr(Expr::Literal(ValueType::Nil));
                        self.ir.tables[table_idx.val()].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_id,
                            visibility: *visibility,
                            annotation: None,
                            annotation_text: None,
                            extra_exprs: Vec::new(),
                            annotation_type_raw: Some(annotation_type.clone()),
                            lateinit: is_lateinit,
                            def_range,
                            flavor_guard: 0,
                            description: class.field_descriptions.get(field_name).cloned(),
                            from_scan: false,
                        });
                    }
                }
            }
        }

        // Mark classes that have explicit @field annotations in the source file.
        // Used by inject-field to distinguish classes with author-declared field
        // contracts from those where fields are inferred from runtime assignments.
        for (class_i, class) in scan.classes.iter().enumerate() {
            if !class.fields.is_empty() {
                let table_idx = class_table_indices[class_i];
                if !table_idx.is_external() {
                    self.ir.tables[table_idx.val()].has_source_fields = true;
                }
            }
        }

        // Propagate field annotations to duplicate @class tables.
        // When multiple @class declarations share the same name, the first one
        // with @field annotations defines the field contract. Subsequent tables
        // inherit those annotations so missing-fields and inject-field work correctly.
        for i in 0..class_table_indices.len() {
            let idx = class_table_indices[i];
            if idx.is_external() || self.ir.tables[idx.val()].has_source_fields { continue; }
            let class_name = &scan.classes[i].name;
            // Find the canonical table (first one with source fields) for this class name
            let canonical = (0..class_table_indices.len())
                .find(|&j| j != i
                    && scan.classes[j].name == *class_name
                    && self.ir.tables[class_table_indices[j].val()].has_source_fields);
            if let Some(j) = canonical {
                let canonical_idx = class_table_indices[j];
                let annotated_fields: Vec<(String, FieldInfo)> = self.ir.tables[canonical_idx.val()].fields.iter()
                    .filter(|(_, fi)| fi.annotation.is_some())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (fname, fi) in annotated_fields {
                    self.ir.tables[idx.val()].fields.entry(fname).or_insert(fi);
                }
                self.ir.tables[idx.val()].has_source_fields = true;
                // Also inherit constructors and parent classes
                let ctors: HashSet<String> = self.ir.tables[canonical_idx.val()].constructors.clone();
                self.ir.tables[idx.val()].constructors.extend(ctors);
                let parents: Vec<TableIndex> = self.ir.tables[canonical_idx.val()].parent_classes.clone();
                for &p in &parents {
                    if !self.ir.tables[idx.val()].parent_classes.contains(&p) {
                        self.ir.tables[idx.val()].parent_classes.push(p);
                    }
                }
            }
        }

        // Build call_func from @overload on local @class declarations
        for (class_i, class) in scan.classes.iter().enumerate() {
            if class.overloads.is_empty() { continue; }
            let table_idx = class_table_indices[class_i];
            if table_idx.is_external() { continue; }
            let overload = &class.overloads[0];
            let mut generics: Vec<(String, Option<String>)> = class.generics.clone();
            for tp in &class.type_params {
                if !generics.iter().any(|(n, _)| n == tp) {
                    generics.push((tp.clone(), None));
                }
            }
            let func_vt = self.materialize_fun_type(
                &overload.params, &overload.returns, overload.is_vararg, &generics,
            );
            if let ValueType::Function(Some(func_idx)) = func_vt {
                let resolved_generics: Vec<(String, Option<ValueType>)> = generics.iter()
                    .map(|(name, _)| (name.clone(), None))
                    .collect();
                self.ir.functions[func_idx.val()].generics = resolved_generics;
                self.ir.functions[func_idx.val()].generic_constraints_raw = generics.clone();
                let generic_names: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
                for (i, ret_ann) in overload.returns.iter().enumerate() {
                    if let Some(proj @ crate::types::ProjectionKind::Return(..)) =
                        crate::annotations::match_projection(ret_ann, &generic_names)
                    {
                        self.ir.functions[func_idx.val()].return_projections.insert(i, proj);
                    }
                }
                self.ir.tables[table_idx.val()].call_func = Some(func_idx);
            }
        }

        // Import fields and parents from external classes for @class overlays.
        // When a local @class re-declares a name that exists externally (e.g. from
        // @built-name), merge in the external fields not overridden by local @field,
        // and import parent_classes (e.g. BaseState from @return built : BaseState).
        for (class_i, class) in scan.classes.iter().enumerate() {
            let local_idx = class_table_indices[class_i];
            if local_idx.is_external() { continue; }
            if let Some(&ext_idx) = ext.classes.get(&class.name) {
                let ext_fields: Vec<(String, FieldInfo)> = self.ir.table(ext_idx).fields.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let local_has_field_contract = self.ir.tables[local_idx.val()].has_source_fields;
                // Two complementary filters control which external fields are imported:
                //
                // Filter 1 (unconditional): Skips unannotated table-typed fields —
                // speculative table placeholders from workspace scanning. These would
                // suppress inject-field checks if imported. Applies to all classes.
                //
                // Filter 2 (conditional on @field contract): When the local class has
                // explicit @field annotations, skips ALL workspace-scan discoveries
                // (strings, numbers, etc.) via the `from_scan` flag, preserving
                // inject-field diagnostics for undeclared fields. Authored fields
                // (annotations, function definitions) are always imported.
                for (fname, fi) in ext_fields {
                    if fi.annotation.is_none()
                        && fi.annotation_type_raw.is_none()
                        && matches!(self.ir.expr(fi.expr), Expr::Literal(ValueType::Table(..)))
                    {
                        continue;
                    }
                    if local_has_field_contract && fi.from_scan {
                        continue;
                    }
                    if let std::collections::hash_map::Entry::Vacant(e) = self.ir.tables[local_idx.val()].fields.entry(fname) {
                        e.insert(fi);
                    }
                }
                // Import parent_classes from the external class
                let ext_parents = self.ir.table(ext_idx).parent_classes.clone();
                for parent_idx in ext_parents {
                    if !self.ir.tables[local_idx.val()].parent_classes.contains(&parent_idx) {
                        self.ir.tables[local_idx.val()].parent_classes.push(parent_idx);
                    }
                }
            }
        }

        // Resolve direct `table<K,V>` parents before the fixpoint loop so
        // transitive inheritance can propagate key_type/value_type to children.
        for (class_i, class) in scan.classes.iter().enumerate() {
            let child_idx = class_table_indices[class_i];
            if child_idx.is_external() { continue; }
            for parent_name in &class.parents {
                if let Some((key_type, value_type)) = self.resolve_table_parent_types(parent_name) {
                    self.ir.tables[child_idx.val()].key_type = Some(key_type);
                    self.ir.tables[child_idx.val()].value_type = Some(value_type);
                }
            }
        }

        // Pass 3: Resolve inheritance (transitive via fixpoint loop).
        // Parent may be external (>= EXT_BASE, already fully resolved) or local.
        loop {
            let mut changed = false;
            for (class_i, class) in scan.classes.iter().enumerate() {
                if class.parents.is_empty() { continue; }
                let child_idx = class_table_indices[class_i];
                for parent_name in &class.parents {
                    let Some((lookup, _)) = crate::annotations::parent_link_with_bindings(parent_name) else { continue };
                    if let Some(&parent_idx) = self.ir.classes.get(lookup.as_str()) {
                        let parent_fields: Vec<(String, FieldInfo)> =
                            self.ir.table(parent_idx).fields.iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                        for (fname, field_info) in parent_fields {
                            if let std::collections::hash_map::Entry::Vacant(e) = self.ir.tables[child_idx.val()].fields.entry(fname) {
                                e.insert(field_info);
                                changed = true;
                            }
                        }
                        let parent_accessors: Vec<(String, crate::annotations::Visibility)> =
                            self.ir.table(parent_idx).accessors.iter()
                                .map(|(k, v)| (k.clone(), *v))
                                .collect();
                        for (aname, vis) in parent_accessors {
                            if !child_idx.is_external()
                                && let std::collections::hash_map::Entry::Vacant(e) = self.ir.tables[child_idx.val()].accessors.entry(aname) {
                                    e.insert(vis);
                                    changed = true;
                                }
                        }
                        if !child_idx.is_external() {
                            let parent_constructors: Vec<String> =
                                self.ir.table(parent_idx).constructors.iter().cloned().collect();
                            for cname in parent_constructors {
                                if self.ir.tables[child_idx.val()].constructors.insert(cname) {
                                    changed = true;
                                }
                            }
                        }
                        // Inherit @correlated groups from parent
                        if !child_idx.is_external() {
                            let parent_groups: Vec<Vec<String>> =
                                self.ir.table(parent_idx).correlated_groups.clone();
                            for group in parent_groups {
                                if !self.ir.tables[child_idx.val()].correlated_groups.contains(&group) {
                                    self.ir.tables[child_idx.val()].correlated_groups.push(group);
                                    changed = true;
                                }
                            }
                        }
                        // Inherit key_type/value_type from parent class
                        if !child_idx.is_external() {
                            let parent_kv = (
                                self.ir.table(parent_idx).key_type.clone(),
                                self.ir.table(parent_idx).value_type.clone(),
                            );
                            if let (Some(kt), Some(vt)) = parent_kv {
                                let child = &mut self.ir.tables[child_idx.val()];
                                if child.key_type.is_none() {
                                    child.key_type = Some(kt);
                                    child.value_type = Some(vt);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            if !changed { break; }
        }

        // Merge annotation parents into local class tables, preserving any
        // parents already imported from external data (e.g. defclass constraint
        // parents like Class<P>).
        for (class_i, class) in scan.classes.iter().enumerate() {
            if class.parents.is_empty() { continue; }
            let child_idx = class_table_indices[class_i];
            if child_idx.is_external() { continue; }
            let parent_indices: Vec<TableIndex> = class.parents.iter()
                .filter_map(|p| crate::annotations::parent_link_with_bindings(p))
                .filter_map(|(p, _)| self.ir.classes.get(p.as_str()).copied())
                .collect();
            // Record direct-parent type-arg bindings for ANY parameterized parent
            // (including renamed/non-identity ones like `Child<TCur,TShared> :
            // Parent<TCur>`), resolving each arg with the child's params as type
            // variables. This is independent of `parent_classes` linkage — method
            // resolution still relies on `parent_classes`/flattened fields (which
            // may come from defclass), while `parent_type_bindings` only drives
            // ancestor type-param translation at call resolution.
            let bindings = crate::annotations::collect_parent_type_bindings(
                &class.parents, &class.type_params, &self.ir.classes,
                |a, g| self.resolve_annotation_type_gen(a, g),
            );
            let table = &mut self.ir.tables[child_idx.val()];
            for parent_idx in parent_indices {
                if !table.parent_classes.contains(&parent_idx) {
                    table.parent_classes.push(parent_idx);
                }
            }
            for (parent_idx, b) in bindings {
                if !table.parent_type_bindings.iter().any(|(p, _)| *p == parent_idx) {
                    table.parent_type_bindings.push((parent_idx, b));
                }
            }
        }

    }


    /// Pre-scan for `local X = defclassFunc("ClassName")` patterns.
    /// When a call to a `@defclass` function is found with a string literal argument,
    /// auto-create the class table before Phase 1 so methods can be defined on it.
    pub(super) fn prescan_defclass_calls(&mut self) {
        use crate::ast::*;
        use crate::annotations::extract_annotations;
        let ext = std::sync::Arc::clone(&self.ir.ext);

        // Pass 0: Find local function definitions with @defclass annotations
        let mut local_defclass_funcs: HashMap<String, DefclassFuncInfo> = HashMap::new();
        {
            let Some(block) = Block::cast(self.root()) else { return };
            for stmt in block.statements() {
                let Statement::FunctionDefinition(func) = &stmt else { continue };
                if !func.is_local() { continue; }
                let Some(func_name) = func.name() else { continue };
                let annotations = extract_annotations(func.syntax());
                let Some(defclass_name) = annotations.defclass else { continue };
                // Find constraint from generics (handle parameterized: "BaseClass<P>" → "BaseClass")
                let constraint_entry = annotations.generics.iter()
                    .find(|(n, _)| *n == defclass_name);
                let constraint_raw = constraint_entry
                    .and_then(|(_, c)| c.clone());
                let constraint_table = constraint_raw.as_ref()
                    .and_then(|c| {
                        let base = c.split('<').next().unwrap_or(c);
                        self.ir.classes.get(base)
                    })
                    .copied();
                // Find which param index holds the parent class generic
                let parent_param_idx = annotations.defclass_parent.as_ref().and_then(|parent_name| {
                    annotations.params.iter()
                        .filter(|p| p.name != "...")
                        .position(|p| match &p.typ {
                            crate::annotations::AnnotationType::Simple(name) => name == parent_name,
                            crate::annotations::AnnotationType::Backtick(inner) => matches!(inner.as_ref(), crate::annotations::AnnotationType::Simple(name) if name == parent_name),
                            _ => false,
                        })
                });
                let parent_generic_name = annotations.defclass_parent.clone();
                let param_annotations: Vec<crate::annotations::AnnotationType> = annotations.params.iter()
                    .filter(|p| p.name != "...")
                    .map(|p| p.typ.clone())
                    .collect();
                // Collect positions where the backtick class-name string may appear.
                // For local functions, annotations.params never includes self, so
                // index i in param_annotations equals call-arg index i directly.
                let backtick_param_positions: Vec<usize> = param_annotations.iter().enumerate()
                    .filter_map(|(i, ann)| {
                        if crate::annotations::annotation_contains_backtick(ann) { Some(i) } else { None }
                    })
                    .collect();
                local_defclass_funcs.insert(func_name, DefclassFuncInfo {
                    generic_name: defclass_name, constraint_table, parent_param_idx,
                    constraint_raw, parent_generic_name,
                    param_annotations: Some(param_annotations),
                    backtick_param_positions,
                });
            }
        }
        let Some(block) = Block::cast(self.root()) else { return };
        for stmt in block.statements() {
            // Match: local X = func("ClassName"), ADDON.X = func("ClassName"):method(),
            // or bare func("ClassName") statement.
            let (var_name, call) = match &stmt {
                Statement::LocalAssign(la) => {
                    let Some(name_list) = la.name_list() else { continue };
                    let Some(expr_list) = la.expression_list() else { continue };
                    let names = name_list.names();
                    let exprs = expr_list.expressions();
                    if names.len() != 1 || exprs.len() != 1 { continue; }
                    let Expression::FunctionCall(c) = &exprs[0] else { continue };
                    (Some(names[0].clone()), *c)
                }
                Statement::Assign(a) => {
                    let Some(expr_list) = a.expression_list() else { continue };
                    let exprs = expr_list.expressions();
                    if exprs.len() != 1 { continue; }
                    let Expression::FunctionCall(c) = &exprs[0] else { continue };
                    (None, *c)
                }
                // Bare function-call statement: `addon:NewAddon("Name")` — no LHS variable.
                // @defclass still fires; the class is registered but not bound to a local.
                Statement::FunctionCall(fc) => (None, *fc),
                _ => continue,
            };

            // Walk through method chains to find the innermost defclass call
            let (call, chained) = Self::find_defclass_call_in_chain(&call);
            let Some(ident) = call.identifier() else { continue };
            let func_names = ident.names();
            if func_names.is_empty() { continue; }

            // Collect the call arguments; class-name extraction is deferred until after
            // we know the backtick positions from the function's annotation and overloads.
            let Some(arg_list) = call.arguments() else { continue };
            let call_args = arg_list.expressions();

            // Resolve the function to get constraint_table, parent_param_idx, and constraint_raw
            // (needed for both existing and new classes)
            let is_method_call = call.syntax().kind() == crate::syntax::syntax_kind::SyntaxKind::MethodCall;
            let dc_info = if func_names.len() == 1 {
                if let Some(info) = local_defclass_funcs.get(&func_names[0]) {
                    DefclassFuncInfo {
                        generic_name: info.generic_name.clone(),
                        constraint_table: info.constraint_table,
                        parent_param_idx: info.parent_param_idx,
                        constraint_raw: info.constraint_raw.clone(),
                        parent_generic_name: info.parent_generic_name.clone(),
                        param_annotations: info.param_annotations.clone(),
                        backtick_param_positions: info.backtick_param_positions.clone(),
                    }
                } else {
                    let func_sym_id = SymbolIdentifier::Name(func_names[0].clone());
                    let func_idx = if let Some(&sym_idx) = ext.scope0_symbols.get(&func_sym_id) {
                        match &ext.symbols[sym_idx.ext_offset()].versions.last() {
                            Some(ver) => match &ver.resolved_type {
                                Some(ValueType::Function(Some(idx))) => Some(*idx),
                                Some(ValueType::Table(Some(table_idx))) => {
                                    self.ir.table(*table_idx).call_func
                                }
                                _ => None,
                            },
                            None => None,
                        }
                    } else { None };
                    let Some(func_idx) = func_idx else { continue };
                    let func = self.ir.func(func_idx);
                    let Some(ref dc_name) = func.defclass else { continue };
                    let ct = func.generics.iter()
                        .find(|(n, _)| n == dc_name)
                        .and_then(|(_, c)| match c {
                            Some(ValueType::Table(Some(idx))) => Some(*idx),
                            _ => None,
                        });
                    let cr = func.generic_constraints_raw.iter()
                        .find(|(n, _)| n == dc_name)
                        .and_then(|(_, c)| c.clone());
                    let ppi = func.defclass_parent.as_ref().and_then(|parent_name| {
                        func.param_annotations.iter().position(|ann| {
                            match ann {
                                crate::annotations::AnnotationType::Simple(name) => name == parent_name,
                                crate::annotations::AnnotationType::Backtick(inner) => matches!(inner.as_ref(), crate::annotations::AnnotationType::Simple(name) if name == parent_name),
                                _ => false,
                            }
                        })
                    });
                    let backtick_param_positions =
                        collect_defclass_backtick_positions(func, is_method_call, dc_name);
                    DefclassFuncInfo {
                        generic_name: dc_name.clone(),
                        constraint_table: ct,
                        parent_param_idx: ppi,
                        constraint_raw: cr,
                        parent_generic_name: func.defclass_parent.clone(),
                        param_annotations: Some(func.param_annotations.clone()),
                        backtick_param_positions,
                    }
                }
            } else {
                continue; // For dotted paths, handled in the second loop below
            };

            // Extract the class name from whichever call-argument position carries
            // the backtick class-name string (primary or overload position).
            let class_name = dc_info.backtick_param_positions.iter().find_map(|&pos| {
                if let Some(Expression::Literal(lit)) = call_args.get(pos) {
                    lit.get_string()
                        .map(|s| s.trim_matches(|c: char| c == '"' || c == '\'').to_string())
                } else {
                    None
                }
            });
            let Some(class_name) = class_name else { continue };

            // Resolve specific parent from the call argument (if @defclass T : P)
            let specific_parent = dc_info.parent_param_idx.and_then(|idx| {
                call_args.get(idx).and_then(|arg| self.resolve_defclass_parent_arg(arg))
            });

            // If class already exists (from external data), create a local copy so
            // field injections (e.g. in __init) accumulate on a mutable local table.
            if let Some(&ext_table_idx) = self.ir.classes.get(&class_name) {
                let mut ext_table = self.ir.table(ext_table_idx).clone();
                // Remove placeholder fields (unresolved FunctionCall results registered as
                // Table(None)) so they don't block per-file builder chain resolution.
                ext_table.fields.retain(|_, fi| {
                    !(fi.annotation.is_none()
                      && fi.annotation_type_raw.is_none()
                      && matches!(self.ir.expr(fi.expr), Expr::Literal(ValueType::Table(None))))
                });
                let local_idx = TableIndex(self.ir.tables.len());
                self.ir.tables.push(ext_table);
                self.ir.tables[local_idx.val()].class_name = Some(class_name.clone());
                // Inherit from specific parent and narrow constraint-typed fields
                if let Some(parent_idx) = specific_parent {
                    if !self.ir.tables[local_idx.val()].parent_classes.contains(&parent_idx) {
                        self.ir.tables[local_idx.val()].parent_classes.push(parent_idx);
                    }
                    for (k, v) in &self.ir.table(parent_idx).fields.clone() {
                        self.ir.tables[local_idx.val()].fields.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                    for (k, v) in &self.ir.table(parent_idx).accessors.clone() {
                        self.ir.tables[local_idx.val()].accessors.entry(k.clone()).or_insert(*v);
                    }
                    if let Some(ct) = dc_info.constraint_table {
                        let mut func_generic_subs = HashMap::new();
                        if let Some(ref pgn) = dc_info.parent_generic_name {
                            func_generic_subs.insert(pgn.clone(), parent_idx);
                        }
                        self.substitute_class_type_params(local_idx, dc_info.constraint_raw.as_deref(), ct, &func_generic_subs);
                    }
                }
                // Absorb fields from table literal argument
                let literal_field_entries = Self::extract_defclass_table_literal_field_names(&dc_info.generic_name, dc_info.param_annotations.as_deref(), &call_args);
                let index_sig_type = dc_info.constraint_table.and_then(|idx| self.ir.table(idx).value_type.clone());
                let default_type = index_sig_type.as_ref().cloned().unwrap_or(ValueType::Any);
                for entry in &literal_field_entries {
                    if self.ir.tables[local_idx.val()].fields.contains_key(&entry.name) { continue; }
                    if !entry.children.is_empty() {
                        let sub_table_idx = Self::create_nested_placeholder_table(&entry.children, &mut self.ir, index_sig_type.as_ref(), self.implicit_protected_prefix);
                        let sub_type = ValueType::Table(Some(sub_table_idx));
                        let expr_id = self.ir.push_expr(Expr::Literal(sub_type.clone()));
                        self.ir.tables[local_idx.val()].fields.insert(entry.name.clone(), FieldInfo {
                            expr: expr_id,
                            extra_exprs: Vec::new(),
                            visibility: crate::annotations::default_visibility_for_name(&entry.name, self.implicit_protected_prefix),
                            annotation: Some(sub_type),
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            def_range: None,
                            flavor_guard: 0,
                            description: None,
                            from_scan: false,
                        });
                    } else {
                        let expr_id = self.ir.push_expr(Expr::Literal(default_type.clone()));
                        let annotation = if index_sig_type.is_some() { Some(default_type.clone()) } else { None };
                        self.ir.tables[local_idx.val()].fields.insert(entry.name.clone(), FieldInfo {
                            expr: expr_id,
                            extra_exprs: Vec::new(),
                            visibility: crate::annotations::default_visibility_for_name(&entry.name, self.implicit_protected_prefix),
                            annotation,
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            def_range: None,
                            flavor_guard: 0,
                            description: None,
                            from_scan: false,
                        });
                    }
                }
                self.ir.classes.insert(class_name, local_idx);
                if !chained
                    && let Some(ref vn) = var_name {
                        self.defclass_vars.insert(vn.clone(), local_idx);
                    }
                continue;
            }

            // Inherit fields and accessors from constraint parent
            let mut fields = HashMap::new();
            let mut accessors = HashMap::new();
            let mut parent_classes = Vec::new();
            if let Some(parent_idx) = dc_info.constraint_table {
                parent_classes.push(parent_idx);
                for (k, v) in &self.ir.table(parent_idx).fields {
                    fields.entry(k.clone()).or_insert_with(|| v.clone());
                }
                for (k, v) in &self.ir.table(parent_idx).accessors {
                    accessors.entry(k.clone()).or_insert(*v);
                }
            }
            // Inherit from specific parent (overrides constraint parent fields)
            if let Some(parent_idx) = specific_parent {
                if !parent_classes.contains(&parent_idx) {
                    parent_classes.push(parent_idx);
                }
                for (k, v) in &self.ir.table(parent_idx).fields {
                    fields.insert(k.clone(), v.clone());
                }
                for (k, v) in &self.ir.table(parent_idx).accessors {
                    accessors.insert(k.clone(), *v);
                }
            }

            // Absorb fields from table literal argument matching the defclass generic param
            let literal_field_names = Self::extract_defclass_table_literal_field_names(&dc_info.generic_name, dc_info.param_annotations.as_deref(), &call_args);
            let index_sig_type = dc_info.constraint_table.and_then(|idx| self.ir.table(idx).value_type.clone());
            Self::insert_placeholder_fields(&literal_field_names, &mut fields, &mut self.ir, index_sig_type.as_ref(), self.implicit_protected_prefix);

            let table_idx = TableIndex(self.ir.tables.len());
            self.ir.tables.push(TableInfo {
                fields, class_name: Some(class_name.clone()),
                parent_classes, accessors, ..Default::default()
            });
            // Substitute class type params using the specific parent
            if let Some(parent_idx) = specific_parent
                && let Some(ct) = dc_info.constraint_table {
                    let mut func_generic_subs = HashMap::new();
                    if let Some(ref pgn) = dc_info.parent_generic_name {
                        func_generic_subs.insert(pgn.clone(), parent_idx);
                    }
                    self.substitute_class_type_params(table_idx, dc_info.constraint_raw.as_deref(), ct, &func_generic_subs);
                }
            self.ir.classes.insert(class_name, table_idx);
            if !chained
                && let Some(ref vn) = var_name {
                    self.defclass_vars.insert(vn.clone(), table_idx);
                }
        }

        let Some(block) = Block::cast(self.root()) else { return };

        // Map local variables whose outermost call has a string arg matching a known class.
        // Enables the dotted-path loop below to resolve roots through local aliases.
        let mut local_class_vars: HashMap<String, TableIndex> = HashMap::new();
        for stmt in block.statements() {
            let Statement::LocalAssign(la) = &stmt else { continue };
            let Some(name_list) = la.name_list() else { continue };
            let Some(expr_list) = la.expression_list() else { continue };
            let var_names = name_list.names();
            let var_exprs = expr_list.expressions();
            if var_names.len() != 1 || var_exprs.len() != 1 { continue; }
            let Expression::FunctionCall(call) = &var_exprs[0] else { continue };
            let Some(arg_list) = call.arguments() else { continue };
            let call_args = arg_list.expressions();
            if call_args.is_empty() { continue; }
            let str_arg = match &call_args[0] {
                Expression::Literal(lit) => lit.get_string()
                    .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string()),
                _ => None,
            };
            let Some(str_arg) = str_arg else { continue };
            if let Some(&table_idx) = self.ir.classes.get(str_arg.as_str()) {
                local_class_vars.insert(var_names[0].clone(), table_idx);
            }
        }

        // Also handle dotted paths: local X = tbl.func("ClassName"), ADDON.X = tbl.func("ClassName"):method(),
        // or bare tbl.func("ClassName") / tbl:func("ClassName") statements.
        for stmt in block.statements() {
            let (var_name, call) = match &stmt {
                Statement::LocalAssign(la) => {
                    let Some(name_list) = la.name_list() else { continue };
                    let Some(expr_list) = la.expression_list() else { continue };
                    let names = name_list.names();
                    let exprs = expr_list.expressions();
                    if names.len() != 1 || exprs.len() != 1 { continue; }
                    let Expression::FunctionCall(c) = &exprs[0] else { continue };
                    (Some(names[0].clone()), *c)
                }
                Statement::Assign(a) => {
                    let Some(expr_list) = a.expression_list() else { continue };
                    let exprs = expr_list.expressions();
                    if exprs.len() != 1 { continue; }
                    let Expression::FunctionCall(c) = &exprs[0] else { continue };
                    (None, *c)
                }
                // Bare function-call statement — @defclass still fires.
                Statement::FunctionCall(fc) => (None, *fc),
                _ => continue,
            };

            // Walk through method chains to find the innermost defclass call.
            let (call, chained) = Self::find_defclass_call_in_chain(&call);
            let Some(ident) = call.identifier() else { continue };
            let func_names = ident.names();
            if func_names.len() < 2 { continue; }

            let Some(arg_list) = call.arguments() else { continue };
            let call_args = arg_list.expressions();

            // Resolve root to a table — check external globals, local_class_vars, and local classes
            let root_name = &func_names[0];
            let method_name = &func_names[func_names.len() - 1];
            let root_sym_id = SymbolIdentifier::Name(root_name.clone());
            let table_idx = if let Some(&sym_idx) = ext.scope0_symbols.get(&root_sym_id) {
                match &ext.symbols[sym_idx.ext_offset()].versions.last() {
                    Some(ver) => match &ver.resolved_type {
                        Some(ValueType::Table(Some(idx))) => Some(*idx),
                        _ => None,
                    },
                    None => None,
                }
            } else if let Some(&idx) = local_class_vars.get(root_name.as_str()) {
                Some(idx)
            } else {
                self.ir.classes.get(root_name.as_str()).copied()
            };
            let Some(table_idx) = table_idx else { continue };
            let field_expr = self.ir.get_field(table_idx, method_name).map(|f| f.expr);
            let Some(field_expr) = field_expr else {
                continue;
            };
            let func_idx = match &self.ir.expr(field_expr) {
                Expr::FunctionDef(idx) => Some(*idx),
                _ => None,
            };
            let Some(func_idx) = func_idx else { continue };
            let func = self.ir.func(func_idx);
            let Some(ref defclass_name) = func.defclass else {
                continue;
            };

            let is_method_call = call.syntax().kind() == crate::syntax::syntax_kind::SyntaxKind::MethodCall;
            let backtick_positions =
                collect_defclass_backtick_positions(func, is_method_call, defclass_name);

            let class_name = backtick_positions.iter().find_map(|&pos| {
                if let Some(Expression::Literal(lit)) = call_args.get(pos) {
                    lit.get_string()
                        .map(|s| s.trim_matches(|c: char| c == '"' || c == '\'').to_string())
                } else {
                    None
                }
            });
            let Some(class_name) = class_name else { continue };

            // If the class already exists, just register defclass_vars for access checks
            if let Some(&existing_idx) = self.ir.classes.get(&class_name) {
                if !chained
                    && let Some(ref vn) = var_name {
                        self.defclass_vars.entry(vn.clone()).or_insert(existing_idx);
                    }
                continue;
            }

            let constraint_table = func.generics.iter()
                .find(|(n, _)| n == defclass_name)
                .and_then(|(_, c)| match c {
                    Some(ValueType::Table(Some(idx))) => Some(*idx),
                    _ => None,
                });
            let constraint_raw = func.generic_constraints_raw.iter()
                .find(|(n, _)| n == defclass_name)
                .and_then(|(_, c)| c.clone());
            let parent_generic_name = func.defclass_parent.clone();

            // Find parent param index from the function's param_annotations
            let parent_param_idx = func.defclass_parent.as_ref().and_then(|parent_name| {
                func.param_annotations.iter().position(|ann| {
                    match ann {
                        crate::annotations::AnnotationType::Simple(name) => name == parent_name,
                        crate::annotations::AnnotationType::Backtick(inner) => matches!(inner.as_ref(), crate::annotations::AnnotationType::Simple(name) if name == parent_name),
                        _ => false,
                    }
                })
            });
            let specific_parent = parent_param_idx.and_then(|idx| {
                call_args.get(idx).and_then(|arg| self.resolve_defclass_parent_arg(arg))
            });

            let mut fields = HashMap::new();
            let mut accessors = HashMap::new();
            let mut parent_classes = Vec::new();
            if let Some(parent_idx) = constraint_table {
                parent_classes.push(parent_idx);
                for (k, v) in &self.ir.table(parent_idx).fields {
                    fields.entry(k.clone()).or_insert_with(|| v.clone());
                }
                for (k, v) in &self.ir.table(parent_idx).accessors {
                    accessors.entry(k.clone()).or_insert(*v);
                }
            }
            // Inherit from specific parent (overrides constraint parent fields)
            if let Some(parent_idx) = specific_parent {
                if !parent_classes.contains(&parent_idx) {
                    parent_classes.push(parent_idx);
                }
                for (k, v) in &self.ir.table(parent_idx).fields {
                    fields.insert(k.clone(), v.clone());
                }
                for (k, v) in &self.ir.table(parent_idx).accessors {
                    accessors.insert(k.clone(), *v);
                }
            }

            // Absorb fields from table literal argument matching the defclass generic param
            let defclass_pa: Vec<crate::annotations::AnnotationType> = self.ir.func(func_idx).param_annotations.clone();
            let literal_field_names = Self::extract_defclass_table_literal_field_names(defclass_name, Some(&defclass_pa), &call_args);
            // Use parent class index signature type for placeholder fields if available
            let index_sig_type = constraint_table.and_then(|idx| self.ir.table(idx).value_type.clone());
            Self::insert_placeholder_fields(&literal_field_names, &mut fields, &mut self.ir, index_sig_type.as_ref(), self.implicit_protected_prefix);

            let new_table_idx = TableIndex(self.ir.tables.len());
            self.ir.tables.push(TableInfo {
                fields, class_name: Some(class_name.clone()),
                parent_classes, accessors, ..Default::default()
            });
            // Substitute class type params using the specific parent
            if let Some(parent_idx) = specific_parent
                && let Some(ct) = constraint_table {
                    let mut func_generic_subs = HashMap::new();
                    if let Some(ref pgn) = parent_generic_name {
                        func_generic_subs.insert(pgn.clone(), parent_idx);
                    }
                    self.substitute_class_type_params(new_table_idx, constraint_raw.as_deref(), ct, &func_generic_subs);
                }
            self.ir.classes.insert(class_name, new_table_idx);
            if !chained
                && let Some(ref vn) = var_name {
                    self.defclass_vars.insert(vn.clone(), new_table_idx);
                }
        }
    }

    /// Extract named field keys from a table literal argument that matches the defclass generic param.
    ///
    /// When `@defclass T` is used with `@param values T`, and the call site passes a table
    /// literal like `{ RESET = EnumType.NewValue(), STARTED = EnumType.NewValue() }`, this
    /// returns the field names with recursive nested sub-field entries (for nested table constructors).
    fn extract_defclass_table_literal_field_names(
        defclass_generic_name: &str,
        param_annotations: Option<&[crate::annotations::AnnotationType]>,
        call_args: &[crate::ast::Expression],
    ) -> Vec<crate::annotations::DefclassFieldEntry> {
        use crate::ast::Expression;

        let Some(annotations) = param_annotations else { return Vec::new() };

        // Find the param index whose annotation type is Simple(defclass_generic_name)
        // (not the Backtick variant — that's the name param)
        let values_param_idx = annotations.iter().position(|ann| {
            matches!(ann, AnnotationType::Simple(name) if name == defclass_generic_name)
        });
        let Some(values_param_idx) = values_param_idx else { return Vec::new() };
        let Some(arg_expr) = call_args.get(values_param_idx) else { return Vec::new() };

        // Check if the argument is a table constructor
        let Expression::TableConstructor(tc) = arg_expr else { return Vec::new() };

        // Extract named field keys recursively
        crate::annotations::extract_table_literal_fields(tc)
    }

    /// Insert placeholder fields from table literal field entries into a fields map.
    /// If `index_sig_type` is provided (from parent class `@field [string] Type`),
    /// use that type instead of `Any` for the placeholder fields.
    /// For nested entries (sub-table constructors), creates intermediate tables whose
    /// fields are typed with the index signature type (recursively for deep nesting).
    fn insert_placeholder_fields(
        field_entries: &[crate::annotations::DefclassFieldEntry],
        fields: &mut HashMap<String, FieldInfo>,
        ir: &mut super::Ir,
        index_sig_type: Option<&ValueType>,
        implicit_protected_prefix: bool,
    ) {
        let default_type = index_sig_type.cloned().unwrap_or(ValueType::Any);
        for entry in field_entries {
            if fields.contains_key(&entry.name) { continue; }
            if !entry.children.is_empty() {
                // Nested table constructor: create a sub-table with the sub-fields (recursively)
                let sub_table_idx = Self::create_nested_placeholder_table(&entry.children, ir, index_sig_type, implicit_protected_prefix);
                let sub_type = ValueType::Table(Some(sub_table_idx));
                let expr_id = ir.push_expr(Expr::Literal(sub_type.clone()));
                fields.insert(entry.name.clone(), FieldInfo {
                    expr: expr_id,
                    extra_exprs: Vec::new(),
                    visibility: crate::annotations::default_visibility_for_name(&entry.name, implicit_protected_prefix),
                    annotation: Some(sub_type),
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
                });
            } else {
                let expr_id = ir.push_expr(Expr::Literal(default_type.clone()));
                let annotation = if index_sig_type.is_some() { Some(default_type.clone()) } else { None };
                fields.insert(entry.name.clone(), FieldInfo {
                    expr: expr_id,
                    extra_exprs: Vec::new(),
                    visibility: crate::annotations::default_visibility_for_name(&entry.name, implicit_protected_prefix),
                    annotation,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
                });
            }
        }
    }

    /// Create a sub-table for nested defclass fields (e.g. nested enum groups).
    /// The sub-table inherits from the index signature value type so that it can
    /// also be used as that type (e.g. a nested enum group is both a container
    /// for sub-values AND an EnumValue itself).
    /// Handles recursive nesting: children with their own children get sub-tables too.
    fn create_nested_placeholder_table(
        children: &[crate::annotations::DefclassFieldEntry],
        ir: &mut super::Ir,
        index_sig_type: Option<&ValueType>,
        implicit_protected_prefix: bool,
    ) -> TableIndex {
        let default_type = index_sig_type.cloned().unwrap_or(ValueType::Any);
        let mut sub_fields = HashMap::new();
        for child in children {
            if !child.children.is_empty() {
                // Recursively create sub-table for deeper nesting
                let nested_idx = Self::create_nested_placeholder_table(&child.children, ir, index_sig_type, implicit_protected_prefix);
                let nested_type = ValueType::Table(Some(nested_idx));
                let expr_id = ir.push_expr(Expr::Literal(nested_type.clone()));
                sub_fields.insert(child.name.clone(), FieldInfo {
                    expr: expr_id,
                    extra_exprs: Vec::new(),
                    visibility: crate::annotations::default_visibility_for_name(&child.name, implicit_protected_prefix),
                    annotation: Some(nested_type),
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
                });
            } else {
                let expr_id = ir.push_expr(Expr::Literal(default_type.clone()));
                let annotation = if index_sig_type.is_some() { Some(default_type.clone()) } else { None };
                sub_fields.insert(child.name.clone(), FieldInfo {
                    expr: expr_id,
                    extra_exprs: Vec::new(),
                    visibility: crate::annotations::default_visibility_for_name(&child.name, implicit_protected_prefix),
                    annotation,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
                });
            }
        }
        // Inherit from the index signature value type (e.g. EnumValue) so the
        // sub-table can be used wherever that type is expected.
        let mut parent_classes = Vec::new();
        if let Some(ValueType::Table(Some(parent_idx))) = index_sig_type {
            // Copy parent's fields into the sub-table so they're directly accessible
            for (k, v) in &ir.table(*parent_idx).fields.clone() {
                sub_fields.entry(k.clone()).or_insert_with(|| v.clone());
            }
            parent_classes.push(*parent_idx);
        }
        let sub_table_idx = TableIndex(ir.tables.len());
        ir.tables.push(TableInfo {
            fields: sub_fields, parent_classes,
            key_type: index_sig_type.as_ref().map(|_| ValueType::String(None)),
            value_type: index_sig_type.cloned(),
            ..Default::default()
        });
        sub_table_idx
    }

    /// Walk a FunctionCall chain to find the innermost call.
    /// For `DefineClass("X"):AddDep("y")`, returns the `DefineClass("X")` call.
    fn find_defclass_call_in_chain<'b>(call: &crate::ast::FunctionCall<'b>) -> (crate::ast::FunctionCall<'b>, bool) {
        use crate::ast::{AstNode, FunctionCall};
        let Some(ident) = call.identifier() else { return (*call, false) };
        if let Some(nested) = ident.syntax().children().find_map(FunctionCall::cast) {
            let (inner, _) = Self::find_defclass_call_in_chain(&nested);
            (inner, true)
        } else {
            (*call, false)
        }
    }

    /// Substitute class type parameters on inherited fields of a defclass-created table.
    ///
    /// Given: `@class BaseClass<S>` with `@field __super S?`
    ///        `@generic T: BaseClass<P>`, `@defclass T : P`
    ///        Call: `DefineClass("Dog", Animal)` → P=Animal
    ///
    /// Builds substitution {S → Animal} and re-resolves fields whose `annotation_type_raw`
    /// references class type params.
    ///
    /// `constraint_raw`: raw constraint string like `"BaseClass<P>"` (from generic_constraints_raw)
    /// `constraint_table`: table index of the constraint class (BaseClass)
    /// `func_generic_subs`: map from function generic names to concrete table indices (P → Animal)
    fn substitute_class_type_params(
        &mut self,
        table_idx: TableIndex,
        constraint_raw: Option<&str>,
        constraint_table: TableIndex,
        func_generic_subs: &HashMap<String, TableIndex>,
    ) {
        let Some(constraint_raw) = constraint_raw else { return };
        // Parse constraint type args: "BaseClass<P>" → ["P"]
        let constraint_type_args: Vec<String> = if let Some(open) = constraint_raw.find('<') {
            let args_str = constraint_raw[open+1..].trim_end_matches('>');
            args_str.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        } else {
            return; // No type args on constraint — nothing to substitute
        };
        // Get class type params from constraint table: ["S"]
        let class_type_params = self.ir.table(constraint_table).class_type_params.clone();
        if class_type_params.len() != constraint_type_args.len() { return; }
        // Build substitution: class_type_param → concrete table index
        // e.g. S → P → Animal (chain through func_generic_subs)
        let mut type_param_subs: HashMap<String, TableIndex> = HashMap::new();
        for (class_param, func_generic) in class_type_params.iter().zip(constraint_type_args.iter()) {
            if let Some(&concrete_idx) = func_generic_subs.get(func_generic) {
                type_param_subs.insert(class_param.clone(), concrete_idx);
            }
        }
        if type_param_subs.is_empty() { return; }
        // Collect fields whose raw annotation references any class type param
        let type_param_names: Vec<String> = type_param_subs.keys().cloned().collect();
        let fields_to_update: Vec<(String, crate::annotations::AnnotationType)> = self.ir.tables[table_idx.val()].fields.iter()
            .filter(|(_, fi)| fi.annotation_type_raw.as_ref()
                .is_some_and(|raw| crate::pre_globals::annotation_type_references_type_params(raw, &type_param_names)))
            .map(|(name, fi)| (name.clone(), fi.annotation_type_raw.clone().unwrap()))
            .collect();
        for (field_name, raw_type) in fields_to_update {
            let substituted = self.substitute_annotation_type(&raw_type, &type_param_subs);
            if let Some(vt) = self.resolve_annotation_type_mut(&substituted) {
                let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                if let Some(fi) = self.ir.tables[table_idx.val()].fields.get_mut(&field_name) {
                    fi.expr = expr_id;
                    fi.annotation = Some(vt);
                }
            }
        }
    }

    /// Substitute type parameter names in an AnnotationType with concrete class names.
    fn substitute_annotation_type(
        &self,
        at: &crate::annotations::AnnotationType,
        subs: &HashMap<String, TableIndex>,
    ) -> crate::annotations::AnnotationType {
        use crate::annotations::AnnotationType;
        match at {
            AnnotationType::Simple(name) => {
                if let Some(&table_idx) = subs.get(name) {
                    if let Some(class_name) = &self.ir.table(table_idx).class_name {
                        AnnotationType::Simple(class_name.clone())
                    } else {
                        at.clone()
                    }
                } else {
                    at.clone()
                }
            }
            AnnotationType::Union(parts) => {
                AnnotationType::Union(parts.iter().map(|p| self.substitute_annotation_type(p, subs)).collect())
            }
            AnnotationType::Array(inner) => {
                AnnotationType::Array(Box::new(self.substitute_annotation_type(inner, subs)))
            }
            AnnotationType::Parameterized(base, args) => {
                AnnotationType::Parameterized(
                    base.clone(),
                    args.iter().map(|a| self.substitute_annotation_type(a, subs)).collect(),
                )
            }
            AnnotationType::Backtick(inner) => {
                AnnotationType::Backtick(Box::new(self.substitute_annotation_type(inner, subs)))
            }
            AnnotationType::NonNil(inner) => {
                AnnotationType::NonNil(Box::new(self.substitute_annotation_type(inner, subs)))
            }
            AnnotationType::Intersection(parts) => {
                AnnotationType::Intersection(parts.iter().map(|p| self.substitute_annotation_type(p, subs)).collect())
            }
            AnnotationType::Fun(params, returns, is_vararg) => {
                let new_params: Vec<_> = params.iter().map(|p| crate::annotations::ParamInfo {
                    name: p.name.clone(),
                    typ: self.substitute_annotation_type(&p.typ, subs),
                    optional: p.optional,
                    description: p.description.clone(),
                }).collect();
                let new_returns: Vec<_> = returns.iter().map(|r| self.substitute_annotation_type(r, subs)).collect();
                AnnotationType::Fun(new_params, new_returns, *is_vararg)
            }
            AnnotationType::TableLiteral(fields) => {
                AnnotationType::TableLiteral(fields.iter().map(|(name, ft)| {
                    (name.clone(), self.substitute_annotation_type(ft, subs))
                }).collect())
            }
            AnnotationType::VarArgs(inner) => {
                AnnotationType::VarArgs(Box::new(self.substitute_annotation_type(inner, subs)))
            }
            AnnotationType::IndexedAccess(base, key) => {
                let substituted_base = if let Some(&table_idx) = subs.get(base) {
                    self.ir.table(table_idx).class_name.clone().unwrap_or_else(|| base.clone())
                } else {
                    base.clone()
                };
                AnnotationType::IndexedAccess(
                    substituted_base,
                    Box::new(self.substitute_annotation_type(key, subs)),
                )
            }
            AnnotationType::Tuple(positions, description) => {
                AnnotationType::Tuple(
                    positions.iter().map(|p| crate::annotations::TuplePosition {
                        typ: self.substitute_annotation_type(&p.typ, subs),
                        name: p.name.clone(),
                    }).collect(),
                    description.clone(),
                )
            }
        }
    }

    /// Resolve a defclass parent argument expression to a table index.
    /// Handles: Identifier expressions (local vars, defclass vars, classes, external symbols)
    /// and string literals (class name lookup).
    fn resolve_defclass_parent_arg(&self, arg: &crate::ast::Expression) -> Option<TableIndex> {
        use crate::ast::Expression;
        match arg {
            Expression::Identifier(ident) => {
                let names = ident.names();
                if names.len() != 1 { return None; }
                let name = &names[0];
                // Check defclass_vars first (local class variables from earlier DefineClass calls)
                if let Some(&idx) = self.defclass_vars.get(name) {
                    return Some(idx);
                }
                // Check ir.classes (known class names used as variable names)
                if let Some(&idx) = self.ir.classes.get(name) {
                    return Some(idx);
                }
                // Check external symbols
                let ext = &self.ir.ext;
                let sym_id = SymbolIdentifier::Name(name.clone());
                if let Some(&sym_idx) = ext.scope0_symbols.get(&sym_id)
                    && let Some(ver) = ext.symbols[sym_idx.ext_offset()].versions.last()
                        && let Some(ValueType::Table(Some(idx))) = &ver.resolved_type {
                            return Some(*idx);
                        }
                None
            }
            Expression::Literal(lit) => {
                // String literal → look up as class name
                let s = lit.get_string()?;
                let name = s.trim_matches(|c| c == '"' || c == '\'');
                self.ir.classes.get(name).copied()
            }
            _ => None,
        }
    }

    /// Recursively expand an annotation type through aliases to find a `Fun(...)` type,
    /// then format it as text for materialization. Handles `Simple("AliasName")`,
    /// `Union([AliasName, nil])` (optional aliases), and nested unions.
    fn resolve_fun_text_from_alias(
        at: &AnnotationType,
        local_aliases: &HashMap<&str, &AnnotationType>,
        ext_aliases: &HashMap<String, AnnotationType>,
    ) -> Option<String> {
        match at {
            AnnotationType::Fun(..) => Some(crate::annotations::format_annotation_type(at)),
            AnnotationType::Simple(name) => {
                if let Some(alias_at) = local_aliases.get(name.as_str()) {
                    Self::resolve_fun_text_from_alias(alias_at, local_aliases, ext_aliases)
                } else if let Some(alias_at) = ext_aliases.get(name.as_str()) {
                    Self::resolve_fun_text_from_alias(alias_at, local_aliases, ext_aliases)
                } else {
                    None
                }
            }
            AnnotationType::Union(parts) => {
                parts.iter().find_map(|p| Self::resolve_fun_text_from_alias(p, local_aliases, ext_aliases))
            }
            _ => None,
        }
    }

    /// Convert fun(...) field annotations into real Function entries.
    /// Runs after build_ir so that function/scope/symbol indices are stable.
    pub(super) fn materialize_fun_annotations(&mut self) {
        // Collect fields that need materialization (to avoid borrow conflicts)
        // `in_union` indicates Function(None) is inside a Union rather than top-level
        let mut to_materialize: Vec<(TableIndex, String, String, bool)> = Vec::new();
        // Build combined alias lookup for resolving function alias types
        let local_alias_refs: HashMap<&str, &AnnotationType> = self.ir.alias_fun_types.iter()
            .map(|(k, v)| (k.as_str(), v)).collect();
        for (table_idx_raw, table) in self.ir.tables.iter().enumerate() {
            let table_idx = TableIndex(table_idx_raw);
            for (field_name, fi) in &table.fields {
                if matches!(&fi.annotation, Some(ValueType::Function(None))) {
                    let text = fi.annotation_text.as_ref()
                        .filter(|t| t.starts_with("fun("))
                        .cloned()
                        .or_else(|| {
                            fi.annotation_type_raw.as_ref().and_then(|raw| {
                                Self::resolve_fun_text_from_alias(raw, &local_alias_refs, &self.ir.ext.alias_fun_types)
                            })
                        });
                    if let Some(text) = text {
                        to_materialize.push((table_idx, field_name.clone(), text, false));
                    }
                } else if let Some(ValueType::Union(types)) = &fi.annotation
                    && types.iter().any(|t| matches!(t, ValueType::Function(None))) {
                        // Use annotation_text (set during prescan for both inline fun()
                        // and alias types), or extract from raw annotation type
                        let text = fi.annotation_text.as_ref().filter(|t| t.starts_with("fun(")).cloned().or_else(|| {
                            fi.annotation_type_raw.as_ref().and_then(|raw| {
                                let fun_part = match raw {
                                    AnnotationType::Union(parts) => parts.iter().find(|p| matches!(p, AnnotationType::Fun(..))),
                                    _ => None,
                                };
                                if let Some(fun_at) = fun_part {
                                    Some(crate::annotations::format_annotation_type(fun_at))
                                } else {
                                    Self::resolve_fun_text_from_alias(raw, &local_alias_refs, &self.ir.ext.alias_fun_types)
                                }
                            })
                        });
                        if let Some(text) = text {
                            to_materialize.push((table_idx, field_name.clone(), text, true));
                        }
                    }
            }
        }
        // Also scan overlay fields (runtime-assigned fields on external tables)
        for (&table_idx, fields) in &self.ir.overlay_fields {
            for (field_name, fi) in fields {
                if matches!(&fi.annotation, Some(ValueType::Function(None))) {
                    let text = fi.annotation_text.clone().or_else(|| {
                        fi.annotation_type_raw.as_ref().and_then(|raw| {
                            Self::resolve_fun_text_from_alias(raw, &local_alias_refs, &self.ir.ext.alias_fun_types)
                        })
                    });
                    if let Some(text) = text {
                        to_materialize.push((table_idx, field_name.clone(), text, false));
                    }
                } else if let Some(ValueType::Union(types)) = &fi.annotation
                    && types.iter().any(|t| matches!(t, ValueType::Function(None))) {
                        let text = fi.annotation_text.as_ref().filter(|t| t.starts_with("fun(")).cloned().or_else(|| {
                            fi.annotation_type_raw.as_ref().and_then(|raw| {
                                let fun_part = match raw {
                                    AnnotationType::Union(parts) => parts.iter().find(|p| matches!(p, AnnotationType::Fun(..))),
                                    _ => None,
                                };
                                if let Some(fun_at) = fun_part {
                                    Some(crate::annotations::format_annotation_type(fun_at))
                                } else {
                                    Self::resolve_fun_text_from_alias(raw, &local_alias_refs, &self.ir.ext.alias_fun_types)
                                }
                            })
                        });
                        if let Some(text) = text {
                            to_materialize.push((table_idx, field_name.clone(), text, true));
                        }
                    }
            }
        }
        if to_materialize.is_empty() { return; }

        let dummy_node = DefNode::DUMMY;
        for (table_idx, field_name, fun_text, in_union) in to_materialize {
            let Some(sig) = parse_overload(&fun_text) else { continue };
            let func_scope = self.ir.insert_scope(None);
            let mut arg_symbols = Vec::new();
            let mut param_annotations = Vec::new();
            let mut param_optional = Vec::new();
            for p in &sig.params {
                if p.name == "..." { continue; }
                let resolved = self.resolve_annotation_type(&p.typ);
                let resolved = if p.optional {
                    resolved.map(|vt| ValueType::union(vt, ValueType::Nil))
                } else {
                    resolved
                };
                let sym_idx = SymbolIndex(self.ir.symbols.len());
                self.ir.symbols.push(Symbol {
                    id: SymbolIdentifier::Name(p.name.clone()),
                    scope_idx: func_scope,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type: resolved,
                        type_args: Vec::new(),
                        created_in_scope: func_scope,
                        creation_order: 0,
                        original_type_source: None,
                    }],
                    flavor_guard: 0,
                });
                self.ir.scopes[func_scope.val()].symbols.insert(
                    SymbolIdentifier::Name(p.name.clone()), sym_idx,
                );
                arg_symbols.push(sym_idx);
                param_annotations.push(p.typ.clone());
                param_optional.push(p.optional);
            }

            let func_idx = FunctionIndex(self.ir.functions.len());

            // Detect tuple-form returns: fun(): (true ok, number v) | (false, string)
            let tuple_form_flags: Vec<bool> = sig.returns.iter()
                .map(crate::annotations::annotation_is_tuple_form).collect();
            let is_tuple_form = sig.returns.len() == 1
                && tuple_form_flags.iter().all(|&b| b);

            let (return_annotations, return_annotations_raw, return_labels, overloads, has_vararg_return);
            let mut ret_symbols = Vec::new();

            // Helper: create a return symbol with the given resolved type
            let push_ret_symbol = |ir: &mut super::Ir, col: usize, resolved: Option<ValueType>| {
                let sym_idx = SymbolIndex(ir.symbols.len());
                ir.symbols.push(Symbol {
                    id: SymbolIdentifier::FunctionRet(func_idx, col),
                    scope_idx: func_scope,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type: resolved,
                        type_args: Vec::new(),
                        created_in_scope: func_scope,
                        creation_order: 0,
                        original_type_source: None,
                    }],
                    flavor_guard: 0,
                });
                ir.scopes[func_scope.val()].symbols.insert(
                    SymbolIdentifier::FunctionRet(func_idx, col), sym_idx,
                );
                sym_idx
            };

            if is_tuple_form {
                let cases = crate::annotations::tuple_form_cases(&sig.returns[0]);
                let any_vararg_tail = cases.iter().any(|(p, _)| {
                    matches!(p.last().map(|tp| &tp.typ), Some(crate::annotations::AnnotationType::VarArgs(_)))
                });
                has_vararg_return = any_vararg_tail;
                let (col_vts, col_raws, labels, synth) =
                    crate::annotations::lower_tuple_form_cases(&cases, |at| {
                        self.resolve_annotation_type_mut(at)
                    });
                for (col, vt) in col_vts.iter().enumerate() {
                    ret_symbols.push(push_ret_symbol(&mut self.ir, col, Some(vt.clone())));
                }
                return_annotations = col_vts;
                return_annotations_raw = col_raws;
                return_labels = labels;
                overloads = synth;
            } else {
                has_vararg_return = false;
                // Resolve each return type once and reuse for both symbols and annotations
                let resolved: Vec<Option<ValueType>> = sig.returns.iter()
                    .map(|rt| self.resolve_annotation_type_mut(rt))
                    .collect();
                return_annotations = resolved.iter().filter_map(|r| r.clone()).collect();
                return_annotations_raw = sig.returns.clone();
                return_labels = Vec::new();
                overloads = Vec::new();
                for (i, r) in resolved.into_iter().enumerate() {
                    ret_symbols.push(push_ret_symbol(&mut self.ir, i, r));
                }
            };

            self.ir.functions.push(Function {
                def_node: dummy_node,
                scope: func_scope,
                args: arg_symbols,
                rets: ret_symbols,
                return_annotations,
                return_annotations_raw,
                return_labels,
                return_descriptions: Vec::new(),
                overloads,
                doc: None,
                deprecated: false,
                nodiscard: false,
                generics: Vec::new(),
                generic_constraints_raw: Vec::new(),
                param_annotations,
                param_descriptions: Vec::new(),
                defclass: None,
                defclass_parent: None,
                is_vararg: sig.is_vararg,
                vararg_annotation: None,

                vararg_description: None,
                param_optional,
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
                has_vararg_return,
                see: Vec::new(),
                flavors: 0,
                flavor_guard: 0,
                return_projections: std::collections::HashMap::new(),
                vararg_projection: None,
                event_params: None,
                narrows_arg: None,
                requires_constraints: Vec::new(),
                returns_self_type_args: None,
            });

            // Update the field annotation and expr.
            // Fields may be in local tables or overlay fields (for external tables).
            // We must create exprs before borrowing `fi` mutably to avoid borrow conflicts.
            let func_vt = ValueType::Function(Some(func_idx));
            if in_union {
                // First pass: update annotation in place
                let fi = if !table_idx.is_external() {
                    self.ir.tables[table_idx.val()].fields.get_mut(&field_name)
                } else {
                    self.ir.overlay_fields.get_mut(&table_idx)
                        .and_then(|fields| fields.get_mut(&field_name))
                };
                let Some(fi) = fi else { continue };
                if let Some(ValueType::Union(ref mut types)) = fi.annotation {
                    for t in types.iter_mut() {
                        if matches!(t, ValueType::Function(None)) {
                            *t = func_vt.clone();
                            break;
                        }
                    }
                }
                // Re-borrow to get the updated annotation and create expr
                let new_vt = if !table_idx.is_external() {
                    self.ir.tables[table_idx.val()].fields[&field_name].annotation.clone().unwrap()
                } else {
                    self.ir.overlay_fields[&table_idx][&field_name].annotation.clone().unwrap()
                };
                let expr_id = self.ir.push_expr(Expr::Literal(new_vt));
                let fi = if !table_idx.is_external() {
                    self.ir.tables[table_idx.val()].fields.get_mut(&field_name).unwrap()
                } else {
                    self.ir.overlay_fields.get_mut(&table_idx).unwrap().get_mut(&field_name).unwrap()
                };
                fi.expr = expr_id;
            } else {
                let expr_id = self.ir.push_expr(Expr::Literal(func_vt.clone()));
                let fi = if !table_idx.is_external() {
                    self.ir.tables[table_idx.val()].fields.get_mut(&field_name)
                } else {
                    self.ir.overlay_fields.get_mut(&table_idx)
                        .and_then(|fields| fields.get_mut(&field_name))
                };
                let Some(fi) = fi else { continue };
                fi.annotation = Some(func_vt);
                fi.expr = expr_id;
            }
        }
    }

    /// Minimal per-file injection: only non-class global tables (a few dozen).
    /// Class tables and scope0 functions are handled via two-tier lookups.
    pub(super) fn inject_preresolved(&mut self) {
        // Non-class tables (math, string, table, etc.) are now fully built
        // in PreResolvedGlobals and accessible via scope0_symbols / EXT_BASE tables.
        // Nothing to inject per-file.
    }

    fn resolve_table_parent_types(&self, parent_name: &str) -> Option<(ValueType, ValueType)> {
        if !parent_name.contains('<') { return None; }
        let at = crate::annotations::parse_type(parent_name);
        if let AnnotationType::Parameterized(base, args) = &at
            && base == "table" && args.len() == 2 {
                let key_vt = self.resolve_annotation_type(&args[0])?;
                let value_vt = self.resolve_annotation_type(&args[1])?;
                return Some((key_vt, value_vt));
            }
        None
    }

    pub(super) fn resolve_annotation_type(&self, at: &AnnotationType) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, &[], &self.ir.classes, &self.ir.aliases)
    }

    /// Like resolve_annotation_type but creates TableInfo/Function entries for structured types
    /// (table<K,V>, T[], fun(x: T): R), preserving type info for display and substitution.
    pub(super) fn resolve_annotation_type_mut(&mut self, at: &AnnotationType) -> Option<ValueType> {
        if let AnnotationType::Array(inner) = at {
            if let Some(elem_vt) = self.resolve_annotation_type_mut(inner) {
                let table_idx = TableIndex(self.ir.tables.len());
                self.ir.tables.push(TableInfo {
                    key_type: Some(ValueType::Number),
                    value_type: Some(elem_vt),
                    value_type_annotated: true,
                    ..Default::default()
                });
                return Some(ValueType::Table(Some(table_idx)));
            }
            return Some(ValueType::Table(None));
        }
        if let AnnotationType::Parameterized(base, _) = at {
            // expression<C, R> is a built-in type for inline Lua expressions;
            // at the ValueType level it's just a string. The annotation is
            // preserved on param_annotations for call-site expression analysis.
            if base == "expression" {
                return Some(ValueType::String(None));
            }
        }
        if let AnnotationType::Parameterized(base, args) = at {
            // Check parameterized aliases (local then external)
            let alias_template = self.ir.parameterized_aliases.get(base)
                .or_else(|| self.ir.ext.parameterized_aliases.get(base))
                .cloned();
            if let Some((type_params, body)) = alias_template
                && type_params.len() == args.len() {
                    let substituted = crate::annotations::substitute_alias_type_params(&body, &type_params, args);
                    return self.resolve_annotation_type_mut(&substituted);
                }
            if (base == "table" || self.ir.classes.contains_key(base.as_str())) && args.len() == 2 {
                let key_vt = self.resolve_annotation_type(&args[0]);
                let value_vt = self.resolve_annotation_type(&args[1]);
                let base_vt = crate::annotations::resolve_annotation_type(&AnnotationType::Simple(base.clone()), &[], &self.ir.classes, &self.ir.aliases);
                if let Some(vt) = value_vt {
                    // Create a new TableInfo with the key and value types
                    let table_idx = TableIndex(self.ir.tables.len());
                    let (fields, class_name, parent_classes) = match &base_vt {
                        Some(ValueType::Table(Some(idx))) => {
                            let t = self.ir.table(*idx);
                            (t.fields.clone(), t.class_name.clone(), t.parent_classes.clone())
                        }
                        _ => (HashMap::new(), None, Vec::new()),
                    };
                    let accessors = match &base_vt {
                        Some(ValueType::Table(Some(idx))) => self.ir.table(*idx).accessors.clone(),
                        _ => HashMap::new(),
                    };
                    let is_explicit_map = base == "table" && class_name.is_none();
                    self.ir.tables.push(TableInfo {
                        fields, class_name, parent_classes,
                        key_type: key_vt, value_type: Some(vt),
                        accessors, is_explicit_map, value_type_annotated: true,
                        ..Default::default()
                    });
                    return Some(ValueType::Table(Some(table_idx)));
                }
                return base_vt;
            }
        }
        if let AnnotationType::Fun(..) = at {
            // Fun types are materialized into Function entries after build_ir
            // (in materialize_fun_annotations) to avoid scope index conflicts.
            return Some(ValueType::Function(None));
        }
        if let AnnotationType::NonNil(inner) = at {
            return self.resolve_annotation_type_mut(inner);
        }
        if let AnnotationType::TableLiteral(fields) = at {
            return Some(self.materialize_table_literal(fields, &[]));
        }
        if let AnnotationType::Intersection(parts) = at {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| self.resolve_annotation_type_mut(p)).collect();
            return match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => Some(ValueType::Intersection(converted)),
            };
        }
        if let AnnotationType::Union(parts) = at {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| self.resolve_annotation_type_mut(p)).collect();
            return match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => Some(ValueType::make_union(converted)),
            };
        }
        self.resolve_annotation_type(at)
    }

    pub(super) fn resolve_annotation_type_gen(&self, at: &AnnotationType, generics: &[(String, Option<String>)]) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, generics, &self.ir.classes, &self.ir.aliases)
    }

    /// Like resolve_annotation_type_mut but also supports generic type parameters.
    pub(super) fn resolve_annotation_type_mut_gen(&mut self, at: &AnnotationType, generics: &[(String, Option<String>)]) -> Option<ValueType> {
        if let AnnotationType::Array(inner) = at {
            if let Some(elem_vt) = self.resolve_annotation_type_mut_gen(inner, generics) {
                let table_idx = TableIndex(self.ir.tables.len());
                self.ir.tables.push(TableInfo {
                    key_type: Some(ValueType::Number),
                    value_type: Some(elem_vt),
                    value_type_annotated: true,
                    ..Default::default()
                });
                return Some(ValueType::Table(Some(table_idx)));
            }
            return Some(ValueType::Table(None));
        }
        if let AnnotationType::Fun(params, returns, is_vararg) = at {
            return Some(self.materialize_fun_type(params, returns, *is_vararg, generics));
        }
        if let AnnotationType::Parameterized(base, _) = at
            && base == "expression"
        {
            return Some(ValueType::String(None));
        }
        if let AnnotationType::Parameterized(base, args) = at {
            // Gap 4 utility-type projections. At declaration time (F unbound)
            // these have no resolvable ValueType — return Any as a placeholder
            // so the return/vararg slot exists and downstream substitution at
            // call-sites can replace it with the bound F's actual types.
            if (base == "params" || base == "returns")
                && args.len() == 1
                && matches!(&args[0], AnnotationType::Simple(n) if generics.iter().any(|(g, _)| g == n))
            {
                return Some(ValueType::Any);
            }
            // Check parameterized aliases (local then external)
            let alias_template = self.ir.parameterized_aliases.get(base)
                .or_else(|| self.ir.ext.parameterized_aliases.get(base))
                .cloned();
            if let Some((type_params, body)) = alias_template
                && type_params.len() == args.len() {
                    let substituted = crate::annotations::substitute_alias_type_params(&body, &type_params, args);
                    return self.resolve_annotation_type_mut_gen(&substituted, generics);
                }
            if base == "table" && args.len() == 2 {
                let key_vt = self.resolve_annotation_type_mut_gen(&args[0], generics);
                let val_vt = self.resolve_annotation_type_mut_gen(&args[1], generics);
                if key_vt.is_some() || val_vt.is_some() {
                    let vt_annotated = val_vt.is_some();
                    let table_idx = TableIndex(self.ir.tables.len());
                    self.ir.tables.push(TableInfo {
                        key_type: key_vt, value_type: val_vt,
                        is_explicit_map: true, value_type_annotated: vt_annotated,
                        ..Default::default()
                    });
                    return Some(ValueType::Table(Some(table_idx)));
                }
            }
        }
        if let AnnotationType::Union(parts) = at {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| self.resolve_annotation_type_mut_gen(p, generics))
                .collect();
            return match converted.len() {
                0 => None,
                1 => converted.into_iter().next(),
                _ => Some(ValueType::make_union(converted)),
            };
        }
        if let AnnotationType::NonNil(inner) = at {
            return self.resolve_annotation_type_mut_gen(inner, generics);
        }
        if let AnnotationType::TableLiteral(fields) = at {
            return Some(self.materialize_table_literal(fields, generics));
        }
        if let AnnotationType::Intersection(parts) = at {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| self.resolve_annotation_type_mut_gen(p, generics)).collect();
            return match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => Some(ValueType::Intersection(converted)),
            };
        }
        if let AnnotationType::IndexedAccess(base, _) = at {
            // If the base is a generic, return TypeVariable as placeholder.
            // Real resolution happens at call sites via generic substitution.
            if generics.iter().any(|(g, _)| g == base) {
                return Some(ValueType::TypeVariable(base.clone()));
            }
            return Some(ValueType::Any);
        }
        self.resolve_annotation_type_gen(at, generics)
    }

    /// Create a TableInfo IR entry from an anonymous table literal annotation type.
    /// Returns `ValueType::Table(Some(table_idx))` with fields populated.
    fn materialize_table_literal(
        &mut self,
        fields: &[(String, AnnotationType)],
        generics: &[(String, Option<String>)],
    ) -> ValueType {
        let table_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo::default());
        for (name, field_ann) in fields {
            let resolved = if generics.is_empty() {
                self.resolve_annotation_type_mut(field_ann)
            } else {
                self.resolve_annotation_type_mut_gen(field_ann, generics)
            };
            if let Some(vt) = resolved {
                let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                self.ir.tables[table_idx.val()].fields.insert(name.clone(), FieldInfo {
                    expr: expr_id,
                    visibility: crate::annotations::Visibility::Public,
                    annotation: Some(vt),
                    annotation_text: None,
                    extra_exprs: Vec::new(),
                    annotation_type_raw: Some(field_ann.clone()),
                    lateinit: false,
                    def_range: None,
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
                });
            }
        }
        ValueType::Table(Some(table_idx))
    }

    /// Create a Function IR entry from inline fun() annotation type components.
    /// Returns `ValueType::Function(Some(func_idx))` with proper param/return symbols.
    pub(super) fn materialize_fun_type(
        &mut self,
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        is_vararg: bool,
        generics: &[(String, Option<String>)],
    ) -> ValueType {
        let dummy_node = DefNode::DUMMY;
        let func_scope = self.ir.insert_scope(None);
        let mut arg_symbols = Vec::new();
        let mut param_annotations = Vec::new();
        let mut param_optional = Vec::new();
        let mut event_params_info: Option<(String, usize)> = None;
        let mut vararg_proj: Option<crate::types::ProjectionKind> = None;
        let mut vararg_ann: Option<AnnotationType> = None;
        let generic_names_owned: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
        for p in params {
            if p.name == "..." {
                vararg_ann = Some(p.typ.clone());
                // Detect `params<F>` projection on vararg slot when F is a generic
                if let Some(proj) = crate::annotations::match_projection(&p.typ, &generic_names_owned) {
                    // Also set event_params when the generic's constraint is an event type.
                    if let Some(ep) = crate::annotations::detect_event_params_from_generic(&proj, generics, params, &self.ir.ext.event_types) {
                        event_params_info = Some(ep);
                    }
                    vararg_proj = Some(proj);
                } else if let Some(ep) = crate::annotations::detect_event_params(&p.typ, params, &generic_names_owned) {
                    event_params_info = Some(ep);
                }
                continue;
            }
            let resolved = if generics.is_empty() {
                self.resolve_annotation_type_mut(&p.typ)
            } else {
                self.resolve_annotation_type_mut_gen(&p.typ, generics)
            };
            let resolved = if p.optional {
                resolved.map(|vt| ValueType::union(vt, ValueType::Nil))
            } else {
                resolved
            };
            let sym_idx = SymbolIndex(self.ir.symbols.len());
            self.ir.symbols.push(Symbol {
                id: SymbolIdentifier::Name(p.name.clone()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: resolved,
                    type_args: Vec::new(),
                    created_in_scope: func_scope,
                    creation_order: 0,
                    original_type_source: None,
                }],
                flavor_guard: 0,
            });
            self.ir.scopes[func_scope.val()].symbols.insert(
                SymbolIdentifier::Name(p.name.clone()), sym_idx,
            );
            arg_symbols.push(sym_idx);
            param_annotations.push(p.typ.clone());
            param_optional.push(p.optional);
        }

        let func_idx = FunctionIndex(self.ir.functions.len());

        // Detect tuple-union / single-tuple return form. `fun(): (A, B) | (C, D)`
        // parses as `returns == [Union([Tuple([A,B]), Tuple([C,D])])]` (one entry,
        // a union of tuples). `fun(): (A, B)` parses as `[Tuple([A, B])]`.
        let is_tuple_form = returns.len() == 1
            && crate::annotations::annotation_is_tuple_form(&returns[0]);

        let mut tuple_has_vararg_tail = false;
        let ret_info = if is_tuple_form {
            let cases = crate::annotations::tuple_form_cases(&returns[0]);
            tuple_has_vararg_tail = cases.iter().any(|(p, _)| {
                matches!(p.last().map(|tp| &tp.typ), Some(AnnotationType::VarArgs(_)))
            });
            let (col_vts, col_raws, labels, overloads) =
                crate::annotations::lower_tuple_form_cases(&cases, |at| {
                    if generics.is_empty() {
                        self.resolve_annotation_type_mut(at)
                    } else {
                        self.resolve_annotation_type_mut_gen(at, generics)
                    }
                });
            let mut ret_syms = Vec::with_capacity(col_vts.len());
            for (col, vt) in col_vts.iter().enumerate() {
                let sym_idx = SymbolIndex(self.ir.symbols.len());
                self.ir.symbols.push(Symbol {
                    id: SymbolIdentifier::FunctionRet(func_idx, col),
                    scope_idx: func_scope,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type: Some(vt.clone()),
                        type_args: Vec::new(),
                        created_in_scope: func_scope,
                        creation_order: 0,
                        original_type_source: None,
                    }],
                    flavor_guard: 0,
                });
                self.ir.scopes[func_scope.val()].symbols.insert(
                    SymbolIdentifier::FunctionRet(func_idx, col), sym_idx,
                );
                ret_syms.push(sym_idx);
            }
            FunReturnInfo {
                return_annotations: col_vts, return_annotations_raw: col_raws,
                return_labels: labels, ret_symbols: ret_syms, overloads,
            }
        } else {
            let mut vts = Vec::with_capacity(returns.len());
            let mut ret_syms = Vec::with_capacity(returns.len());
            for (i, rt) in returns.iter().enumerate() {
                let resolved = if generics.is_empty() {
                    self.resolve_annotation_type_mut(rt)
                } else {
                    self.resolve_annotation_type_mut_gen(rt, generics)
                };
                if let Some(vt) = resolved.clone() { vts.push(vt); }
                let sym_idx = SymbolIndex(self.ir.symbols.len());
                self.ir.symbols.push(Symbol {
                    id: SymbolIdentifier::FunctionRet(func_idx, i),
                    scope_idx: func_scope,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type: resolved,
                        type_args: Vec::new(),
                        created_in_scope: func_scope,
                        creation_order: 0,
                        original_type_source: None,
                    }],
                    flavor_guard: 0,
                });
                self.ir.scopes[func_scope.val()].symbols.insert(
                    SymbolIdentifier::FunctionRet(func_idx, i), sym_idx,
                );
                ret_syms.push(sym_idx);
            }
            FunReturnInfo {
                return_annotations: vts, return_annotations_raw: returns.to_vec(),
                return_labels: Vec::new(), ret_symbols: ret_syms, overloads: Vec::new(),
            }
        };

        // Detect `returns<F>` projections in return annotations
        let mut ret_projections: std::collections::HashMap<usize, crate::types::ProjectionKind> = std::collections::HashMap::new();
        if !generic_names_owned.is_empty() {
            for (i, rt) in returns.iter().enumerate() {
                match crate::annotations::match_projection(rt, &generic_names_owned) {
                    Some(crate::types::ProjectionKind::Params(_)) => {}
                    Some(proj @ crate::types::ProjectionKind::Return(..)) => {
                        ret_projections.insert(i, proj);
                    }
                    None => {}
                }
            }
        }

        let non_tuple_vararg_return = !is_tuple_form
            && returns.last().is_some_and(|r| matches!(r, AnnotationType::VarArgs(_)));

        // If we have a vararg projection, the fun() is effectively vararg
        let effective_is_vararg = is_vararg || vararg_proj.is_some();

        self.ir.functions.push(Function {
            def_node: dummy_node,
            scope: func_scope,
            args: arg_symbols,
            rets: ret_info.ret_symbols,
            return_annotations: ret_info.return_annotations,
            return_annotations_raw: ret_info.return_annotations_raw,
            return_labels: ret_info.return_labels,
            return_descriptions: Vec::new(),
            overloads: ret_info.overloads,
            doc: None,
            deprecated: false,
            nodiscard: false,
            generics: Vec::new(),
            generic_constraints_raw: Vec::new(),
            param_annotations,
            param_descriptions: Vec::new(),
            defclass: None,
            defclass_parent: None,
            is_vararg: effective_is_vararg,
            vararg_annotation: vararg_ann,

            vararg_description: None,
            param_optional,
            returns_self: false,
            explicit_void_return: returns.is_empty(),
            implicit_nil_return: false,
            constructor: false,
            builds_field: None,
            built_name: None,
            built_extends: false,
            returns_built: false,
            returns_built_parent: None,
            type_narrows: None,
            type_narrows_class: None,
            has_vararg_return: tuple_has_vararg_tail || non_tuple_vararg_return,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0, return_projections: ret_projections, vararg_projection: vararg_proj, event_params: event_params_info,
            narrows_arg: None,
            requires_constraints: Vec::new(),
            returns_self_type_args: None,
        });
        ValueType::Function(Some(func_idx))
    }

    /// If `at` reduces through alias chains to a `fun(...)` annotation — either
    /// directly (`@type FunAlias`), wrapped in `NonNil` (`@type FunAlias!`), or
    /// wrapped in `Union(T, nil)` (`@type FunAlias?`) — materialize a real
    /// `Function(Some(idx))`. Returning a concrete function index lets the
    /// signature survive propagation through `local copied = original`, so
    /// downstream sites (signature help, argument type-checking, declaration
    /// hover) see the full `fun(...)` signature instead of the collapsed
    /// `Function(None)`.
    ///
    /// Returns `None` for non-alias annotations, unions with multiple non-nil
    /// members, and aliases that don't resolve to a function type.
    pub(super) fn try_materialize_fun_alias(&mut self, at: &AnnotationType) -> Option<ValueType> {
        // Clone the terminal Fun annotation's pieces out before mutating IR —
        // `reduce_to_fun_alias` borrows the alias maps through &self.ir.
        let (params, returns, is_vararg, wraps_nil) = {
            let (fun_ann, wraps_nil) = crate::annotations::reduce_to_fun_alias(
                at, &self.ir.alias_fun_types, &self.ir.ext.alias_fun_types,
            )?;
            let AnnotationType::Fun(params, returns, is_vararg) = fun_ann else { return None; };
            (params.clone(), returns.clone(), *is_vararg, wraps_nil)
        };
        let func_vt = self.materialize_fun_type(&params, &returns, is_vararg, &[]);
        if wraps_nil {
            Some(ValueType::union(func_vt, ValueType::Nil))
        } else {
            Some(func_vt)
        }
    }

    /// Infer generic type variables from structured param annotations.
    /// E.g. for `T[]`, extract element types from the arg's table to infer T.
    pub(super) fn infer_generics_from_annotation(
        &mut self,
        annotation: &AnnotationType,
        generic_names: &[String],
        generics: &[(String, Option<ValueType>)],
        defclass: &Option<String>,
        arg_expr_id: ExprId,
        subs: &mut HashMap<String, ValueType>,
    ) {
        match annotation {
            AnnotationType::Array(inner) => {
                // T[] — infer T from array element types
                if let AnnotationType::Simple(name) = inner.as_ref()
                    && generic_names.contains(name) && !subs.contains_key(name)
                        && let Some(elem_type) = self.infer_array_element_type(arg_expr_id) {
                            subs.insert(name.clone(), elem_type);
                        }
            }
            AnnotationType::Parameterized(_base, args) => {
                // table<K, V> — infer K and V from table field types
                if args.len() == 2
                    && let (AnnotationType::Simple(k_name), AnnotationType::Simple(v_name)) = (&args[0], &args[1]) {
                        let k_is_generic = generic_names.contains(k_name) && !subs.contains_key(k_name);
                        let v_is_generic = generic_names.contains(v_name) && !subs.contains_key(v_name);
                        if k_is_generic || v_is_generic {
                            if let Some(table_idx) = self.ir.find_table_index(arg_expr_id) {
                                // Prefer explicit key_type/value_type (from table<K,V> inheritance)
                                let explicit_key = self.ir.table(table_idx).key_type.clone();
                                let explicit_val = self.ir.table(table_idx).value_type.clone();
                                if k_is_generic {
                                    if let Some(kt) = explicit_key {
                                        subs.insert(k_name.clone(), kt);
                                    } else if !self.ir.table(table_idx).fields.is_empty() {
                                        subs.insert(k_name.clone(), ValueType::String(None));
                                    }
                                }
                                if v_is_generic {
                                    if let Some(vt) = explicit_val {
                                        subs.insert(v_name.clone(), vt);
                                    } else {
                                        let field_exprs: Vec<ExprId> = self.ir.table(table_idx).fields.values().map(|f| f.expr).collect();
                                        if !field_exprs.is_empty() {
                                            let field_types: Vec<ValueType> = field_exprs.iter()
                                                .filter_map(|&expr_id| self.resolve_expr(expr_id))
                                                .collect();
                                            if let Some(union_type) = Self::union_of(field_types) {
                                                subs.insert(v_name.clone(), union_type);
                                            }
                                        }
                                    }
                                }
                            } else if let Some(arg_type) = self.resolve_expr(arg_expr_id) {
                                // Fallback for union-typed args (e.g. BranchMerge across if/else).
                                // Collect K/V types from all table members of the union.
                                let table_indices = super::table_indices_from_type(&arg_type);
                                if !table_indices.is_empty() {
                                    if k_is_generic {
                                        // For each table: use its explicit key_type if present;
                                        // for field-only tables (struct-like) approximate with
                                        // String since field keys are always string literals.
                                        // Both contributions are unioned so a mix of typed maps
                                        // and struct-like tables in the same union is handled.
                                        let key_types: Vec<ValueType> = table_indices.iter()
                                            .filter_map(|&ti| {
                                                let tbl = self.ir.table(ti);
                                                if let Some(kt) = tbl.key_type.clone() {
                                                    Some(kt)
                                                } else if !tbl.fields.is_empty() {
                                                    // Field keys are string literals; use String as approximation
                                                    Some(ValueType::String(None))
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect();
                                        if !key_types.is_empty() {
                                            subs.insert(k_name.clone(), ValueType::make_union(key_types));
                                        }
                                    }
                                    if v_is_generic {
                                        // V asymmetry vs K: field values can be any expression
                                        // type, so we resolve each field's expr when value_type
                                        // isn't set (unlike K where we approximate with String).
                                        let val_types: Vec<ValueType> = table_indices.iter()
                                            .filter_map(|&ti| self.ir.table(ti).value_type.clone())
                                            .collect();
                                        if !val_types.is_empty() {
                                            subs.insert(v_name.clone(), self.ir.dedupe_union_tables(ValueType::make_union(val_types)));
                                        } else {
                                            // Collect exprs first to avoid borrow conflict with resolve_expr
                                            let field_exprs: Vec<ExprId> = table_indices.iter()
                                                .flat_map(|&ti| self.ir.table(ti).fields.values().map(|f| f.expr).collect::<Vec<_>>())
                                                .collect();
                                            let field_types: Vec<ValueType> = field_exprs.iter()
                                                .filter_map(|&expr_id| self.resolve_expr(expr_id))
                                                .collect();
                                            if let Some(union_type) = Self::union_of(field_types) {
                                                subs.insert(v_name.clone(), self.ir.dedupe_union_tables(union_type));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
            }
            AnnotationType::Backtick(inner) => {
                // `T` — infer T from string literal value as a type name
                if let AnnotationType::Simple(name) = inner.as_ref()
                    && generic_names.contains(name)
                        && let Some(str_val) = self.ir.string_literals.get(&arg_expr_id).cloned() {
                            // Check primitives first so "string"→String, not stringlib class
                            if let Some(prim) = crate::annotations::resolve_primitive_type_name(&str_val) {
                                subs.insert(name.clone(), prim);
                            } else if let Some(&table_idx) = self.ir.classes.get(str_val.as_str()) {
                                subs.insert(name.clone(), ValueType::Table(Some(table_idx)));
                            } else if defclass.as_deref() == Some(name) {
                                // @defclass T: auto-create class from string literal
                                let parent_indices: Vec<TableIndex> = generics.iter()
                                    .filter(|(n, _)| n == name)
                                    .filter_map(|(_, c)| match c {
                                        Some(ValueType::Table(Some(idx))) => Some(*idx),
                                        _ => None,
                                    })
                                    .collect();
                                // Inherit fields and accessors from parent classes
                                let mut fields = HashMap::new();
                                let mut accessors = HashMap::new();
                                for &parent_idx in &parent_indices {
                                    for (k, v) in &self.ir.table(parent_idx).fields {
                                        fields.entry(k.clone()).or_insert_with(|| v.clone());
                                    }
                                    for (k, v) in &self.ir.table(parent_idx).accessors {
                                        accessors.entry(k.clone()).or_insert(*v);
                                    }
                                }
                                let table_idx = TableIndex(self.ir.tables.len());
                                self.ir.tables.push(TableInfo {
                                    fields, class_name: Some(str_val.clone()),
                                    parent_classes: parent_indices, accessors,
                                    ..Default::default()
                                });
                                self.ir.classes.insert(str_val, table_idx);
                                subs.insert(name.clone(), ValueType::Table(Some(table_idx)));
                            }
                        }
            }
            AnnotationType::NonNil(inner) => {
                self.infer_generics_from_annotation(inner, generic_names, generics, defclass, arg_expr_id, subs);
            }
            AnnotationType::Fun(params, returns, _) => {
                let type_info = self.arg_function_type_info(arg_expr_id);
                // fun(...): T — infer T from the argument's return type, or from
                // the argument itself if it's a value that matches T directly
                // (the union case `(fun(): T) | T` falls through to here via the
                // Union arm below).
                if let Some(ref arg_ret) = type_info.ret {
                    'ret_loop: for ret_ann in returns {
                        if let AnnotationType::Simple(name) = ret_ann
                            && generic_names.contains(name) && !subs.contains_key(name) {
                                subs.insert(name.clone(), arg_ret.clone());
                                break;
                            }
                        // Handle T? = Union([Simple("T"), Simple("nil")]): strip
                        // nil from the argument's return type and bind T.
                        if let AnnotationType::Union(members) = ret_ann
                            && members.iter().any(|m| matches!(m, AnnotationType::Simple(s) if s == "nil"))
                        {
                            for m in members {
                                if let AnnotationType::Simple(name) = m
                                    && generic_names.contains(name) && !subs.contains_key(name)
                                {
                                    let stripped = arg_ret.strip_nil();
                                    if !matches!(stripped, ValueType::Nil) {
                                        subs.insert(name.clone(), stripped);
                                    }
                                    break 'ret_loop;
                                }
                            }
                        }
                    }
                }
                // fun(x: T, y: A): ... — infer generics from the argument
                // function's parameter types. E.g. if the annotation says
                // `fun(value: T, arg: A): any` and the actual function has
                // `@param value number, @param sig? number`, bind A = number.
                //
                // Only `Simple(name)` annotation types are matched here — generics
                // in complex positions (e.g. `T[]`, `table<K, A>`) are not inferred
                // from function param types. This is consistent with the return-type
                // path above which also only handles `Simple(name)`.
                if let Some(ref param_types) = type_info.params {
                    for (i, param_info) in params.iter().enumerate() {
                        if let AnnotationType::Simple(name) = &param_info.typ
                            && generic_names.contains(name) && !subs.contains_key(name)
                            && let Some(pt) = param_types.get(i)
                        {
                            let stripped = pt.strip_nil();
                            if !matches!(&stripped, ValueType::Nil)
                                && !matches!(&stripped, ValueType::Union(t) if t.is_empty())
                            {
                                subs.insert(name.clone(), stripped);
                            }
                        }
                    }
                }
            }
            AnnotationType::Union(members) => {
                // Try every union alternative. Each member may bind a different
                // generic (e.g. `(fun(): T) | U` binds T from a function arg, U
                // from a non-function arg), so we don't short-circuit.
                for member in members {
                    self.infer_generics_from_annotation(member, generic_names, generics, defclass, arg_expr_id, subs);
                }
            }
            AnnotationType::Simple(name) => {
                // Bare `T` parameter — bind to arg's type. The direct-TypeVariable
                // path in resolve.rs handles this when param_type is a TypeVariable,
                // but this fallback covers cases where the structural path is used.
                if generic_names.contains(name) && !subs.contains_key(name)
                    && let Some(arg_type) = self.resolve_expr(arg_expr_id) {
                        let stripped = arg_type.strip_nil();
                        let is_nil_like = matches!(&stripped, ValueType::Nil)
                            || matches!(&stripped, ValueType::Union(t) if t.is_empty());
                        if !is_nil_like {
                            subs.insert(name.clone(), stripped);
                        }
                    }
            }
            _ => {}
        }
    }

    /// Extract return type and parameter types from a function argument in one
    /// `resolve_expr` call, for use by the `Fun` arm of `infer_generics_from_annotation`.
    ///
    /// Return type (`ret`):
    ///   - Function args: first return annotation (or FunctionRet symbol fallback).
    ///   - Named `@class` tables: the class type itself (constructor return).
    ///   - Plain non-class tables: None (prevents empty `{}` from silently binding T).
    ///
    /// Parameter types (`params`):
    ///   - Function args with `@param` annotations: resolved types per position.
    ///     Unresolvable annotations use `Any` as a positional placeholder — this
    ///     preserves index alignment but cannot leak into generic bindings because
    ///     the caller only matches `AnnotationType::Simple(name)` against these.
    ///   - Functions without annotations or non-function args: None.
    pub(super) fn arg_function_type_info(&mut self, arg_expr_id: ExprId) -> ArgFunctionTypeInfo {
        let Some(arg_type) = self.resolve_expr(arg_expr_id) else {
            return ArgFunctionTypeInfo::EMPTY;
        };
        match &arg_type {
            ValueType::Function(Some(fn_idx)) => {
                let fn_idx = *fn_idx;

                // Return type: prefer annotation, fall back to FunctionRet symbol.
                // Skip TypeVariable (unresolved generic placeholder) so we use
                // the actual body-inferred return type instead.
                let ret = {
                    let ann_ret = self.func(fn_idx).return_annotations.first().cloned();
                    if let Some(ref vt) = ann_ret
                        && !matches!(vt, ValueType::TypeVariable(_))
                    {
                        // When the annotation is Any (e.g. from contextual typing
                        // `@param map fun(): any`), prefer the FunctionRet symbol's
                        // body-inferred type if available, since `any` adds no info.
                        if matches!(vt, ValueType::Any) {
                            let func_scope = self.func(fn_idx).scope;
                            let ret_id = SymbolIdentifier::FunctionRet(fn_idx, 0);
                            let body_ret = self.get_symbol(&ret_id, func_scope).and_then(|ret_sym_idx| {
                                let ver = self.sym(ret_sym_idx).versions.first()?;
                                if ver.resolved_type.is_some() {
                                    return ver.resolved_type.clone();
                                }
                                ver.type_source.and_then(|src| self.resolve_expr(src))
                            });
                            body_ret.or(ann_ret)
                        } else {
                            ann_ret
                        }
                    } else {
                        let func_scope = self.func(fn_idx).scope;
                        let ret_id = SymbolIdentifier::FunctionRet(fn_idx, 0);
                        self.get_symbol(&ret_id, func_scope).and_then(|ret_sym_idx| {
                            let ver = self.sym(ret_sym_idx).versions.first()?;
                            if ver.resolved_type.is_some() {
                                return ver.resolved_type.clone();
                            }
                            ver.type_source.and_then(|src| self.resolve_expr(src))
                        })
                    }
                };

                // Parameter types
                let param_annotations = self.func(fn_idx).param_annotations.clone();
                let params = if param_annotations.is_empty() {
                    None
                } else {
                    let mut types = Vec::new();
                    for ann in &param_annotations {
                        types.push(self.resolve_annotation_type(ann).unwrap_or(ValueType::Any));
                    }
                    Some(types)
                };

                ArgFunctionTypeInfo { ret, params }
            }
            ValueType::Table(Some(idx)) => {
                let ret = if self.table(*idx).class_name.is_some() {
                    Some(arg_type)
                } else {
                    None
                };
                ArgFunctionTypeInfo { ret, params: None }
            }
            _ => ArgFunctionTypeInfo::EMPTY,
        }
    }

    /// Compute the element type of an array-like table from its positional fields.
    /// Resolve each expression in `fields` and collect unique types using structural
    /// deduplication (so two anonymous tables with identical shapes collapse to one).
    fn collect_unique_element_types(&mut self, fields: &[ExprId]) -> Vec<ValueType> {
        let mut types: Vec<ValueType> = Vec::new();
        for &field_expr in fields {
            if let Some(vt) = self.resolve_expr(field_expr)
                && !self.is_structurally_duplicate_type(&types, &vt) {
                    types.push(vt);
                }
        }
        types
    }

    pub(super) fn infer_array_element_type(&mut self, expr_id: ExprId) -> Option<ValueType> {
        // Try direct table index first (needed for table constructors with literal elements)
        if let Some(table_idx) = self.ir.find_table_index(expr_id) {
            let array_fields: Vec<ExprId> = self.ir.table(table_idx).array_fields.clone();
            if !array_fields.is_empty() {
                return Self::union_of(self.collect_unique_element_types(&array_fields));
            }
            // Fall back to annotated value_type (e.g. ---@type string[])
            if self.ir.table(table_idx).value_type.is_some() {
                return self.ir.table(table_idx).value_type.clone();
            }
            // Table found but no element info — fall through to resolve_expr
            // so annotated types (e.g. @type (string|number)[]) are used
        }
        // Resolve expression type for annotated variables and field accesses
        match self.resolve_expr(expr_id)? {
            ValueType::Table(Some(idx)) => {
                let array_fields: Vec<ExprId> = self.ir.table(idx).array_fields.clone();
                if !array_fields.is_empty() {
                    return Self::union_of(self.collect_unique_element_types(&array_fields));
                }
                if let Some(vt) = self.ir.table(idx).value_type.clone() {
                    return Some(vt);
                }
                // Bracket-keyed table whose value_type hasn't been set yet
                // (infer_bracket_field_types runs later in the fixpoint loop).
                // Resolve value expressions inline so generic binding (e.g.
                // table<T,R> overload params) can extract the value type now.
                // Uses resolve_expr (not resolve_expr_to_broad_type) to preserve
                // precision — generic binding benefits from exact types rather
                // than broadened categories.
                if let Some(bracket_fields) = self.ir.bracket_key_fields.get(&idx).cloned() {
                    let mut val_types: Vec<ValueType> = Vec::new();
                    for (_key_expr, val_expr) in &bracket_fields {
                        if let Some(vt) = self.resolve_expr(*val_expr)
                            && vt != ValueType::Nil && !self.is_structurally_duplicate_type(&val_types, &vt)
                        {
                            val_types.push(vt);
                        }
                    }
                    if !val_types.is_empty() {
                        return Self::union_of(val_types);
                    }
                }
                None
            }
            // Union of array types: e.g. string[] | ItemKey[] → string | ItemKey
            ValueType::Union(members) => {
                let mut elem_types: Vec<ValueType> = Vec::new();
                for member in &members {
                    if let ValueType::Table(Some(idx)) = member
                        && !self.table(*idx).is_explicit_map
                        && let Some(vt) = self.table(*idx).value_type.clone()
                            && !self.is_structurally_duplicate_type(&elem_types, &vt) {
                                elem_types.push(vt);
                            }
                }
                if !elem_types.is_empty() {
                    Self::union_of(elem_types)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub(super) fn union_of(types: Vec<ValueType>) -> Option<ValueType> {
        match types.len() {
            0 => None,
            1 => types.into_iter().next(),
            _ => {
                let mut iter = types.into_iter();
                let mut result = iter.next().unwrap();
                for vt in iter {
                    result = ValueType::union(result, vt);
                }
                Some(result)
            }
        }
    }

    /// Check all type names in an AnnotationType against known classes/aliases.
    /// Find the byte range of the Nth `---@annotation` comment token containing a specific substring.
    pub(crate) fn find_nth_annotation_comment_range(root: SyntaxNode<'_>, annotation_prefix: &str, name_hint: &str, n: u32) -> Option<(u32, u32)> {
        let mut count = 0u32;
        for event in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            if Self::comment_is_tag(text, annotation_prefix) && Self::contains_word(text, name_hint) {
                count += 1;
                if count == n {
                    let r = tok.text_range();
                    return Some((u32::from(r.start()), u32::from(r.end())));
                }
            }
        }
        None
    }

    /// Check if a comment starts with a given `---@tag` prefix, also matching
    /// the `--- @tag` (space after `---`) variant that
    /// `collect_preceding_annotation_ranges` and `extract_annotations` both accept.
    /// Works for both bare tags (`"---@param"`) and full annotation prefixes
    /// (`"---@class ClassName"`). Zero-allocation.
    pub(crate) fn comment_is_tag(text: &str, tag: &str) -> bool {
        text.starts_with(tag)
            || (text.starts_with("--- @") && text[5..].starts_with(&tag[4..]))
    }

    /// Check whether `word` appears in `text` as a whole word (not as a substring
    /// of a longer identifier).  Boundaries are any non-alphanumeric, non-`_` char,
    /// or start/end of string.
    pub(crate) fn contains_word(text: &str, word: &str) -> bool {
        let bytes = text.as_bytes();
        let mut start = 0;
        while let Some(pos) = text[start..].find(word) {
            let abs = start + pos;
            let before_ok = abs == 0 || {
                let b = bytes[abs - 1];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            let after = abs + word.len();
            let after_ok = after >= bytes.len() || {
                let b = bytes[after];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            if before_ok && after_ok {
                return true;
            }
            start = abs + 1;
        }
        false
    }

    pub(crate) fn find_field_comment_range(root: SyntaxNode<'_>, class_name: &str, field_name: &str, second: bool) -> Option<(u32, u32)> {
        let target = format!("---@field {}", field_name);
        let target_vis = [
            format!("---@field private {}", field_name),
            format!("---@field protected {}", field_name),
            format!("---@field public {}", field_name),
        ];
        let mut in_class = false;
        let mut count = 0u32;
        for event in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            if let Some(after) = text.strip_prefix("---@class ")
                .or_else(|| text.strip_prefix("--- @class "))
            {
                let after = crate::annotations::strip_class_modifier(after);
                let parsed_name = after.split(|c: char| c.is_whitespace() || c == '<' || c == ':')
                    .next().unwrap_or("");
                in_class = parsed_name == class_name;
                continue;
            }
            if in_class && Self::comment_is_tag(text, "---@class") {
                in_class = false; // different class
                continue;
            }
            if in_class {
                let matches = Self::comment_is_tag(text, &target) || target_vis.iter().any(|t| Self::comment_is_tag(text, t.as_str()));
                if matches {
                    count += 1;
                    if (second && count >= 2) || (!second && count >= 1) {
                        let r = tok.text_range();
                        return Some((u32::from(r.start()), u32::from(r.end())));
                    }
                }
            }
        }
        None
    }
}
