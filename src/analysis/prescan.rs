use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::annotations::{AnnotationType, scan_all_annotations};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::types::*;
use super::Analysis;

// ── Annotation Pre-scan (Phase 0) ─────────────────────────────────────────────

impl Analysis {
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
        let scan = scan_all_annotations(&self.root);
        self.is_meta = scan.has_meta;

        // Pass 1: Register local class names with empty tables (local indices)
        for class in &scan.classes {
            let table_idx = self.ir.tables.len();
            self.ir.tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class.name.clone()),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
                value_type: None,
                accessors: class.accessors.iter().cloned().collect(),
                call_func: None,
            });
            self.ir.classes.insert(class.name.clone(), table_idx);
        }

        // Pass 2: Populate local class fields
        for class in &scan.classes {
            let table_idx = self.ir.classes[&class.name];
            let mut seen_fields: HashSet<String> = HashSet::new();
            for (field_name, annotation_type, visibility) in &class.fields {
                if !seen_fields.insert(field_name.clone()) {
                    // Duplicate field — find the second occurrence in comment tokens
                    if let Some((start, end)) = Self::find_field_comment_range(&self.root, &class.name, field_name, true) {
                        crate::diagnostics::duplicate_doc_field::check(
                            &mut self.diagnostics, field_name,
                            start as usize, end as usize,
                        );
                    }
                }
                if let Some(vt) = self.resolve_annotation_type_mut(annotation_type) {
                    let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                    self.ir.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_id,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text: None,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Pass 3: Resolve inheritance (transitive via fixpoint loop).
        // Parent may be external (>= EXT_BASE, already fully resolved) or local.
        loop {
            let mut changed = false;
            for class in &scan.classes {
                if class.parents.is_empty() { continue; }
                let child_idx = self.ir.classes[&class.name];
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = self.ir.classes.get(parent_name.as_str()) {
                        let parent_fields: Vec<(String, FieldInfo)> =
                            self.ir.table(parent_idx).fields.iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                        for (fname, field_info) in parent_fields {
                            if let std::collections::hash_map::Entry::Vacant(e) = self.ir.tables[child_idx].fields.entry(fname) {
                                e.insert(field_info);
                                changed = true;
                            }
                        }
                        let parent_accessors: Vec<(String, crate::annotations::Visibility)> =
                            self.ir.table(parent_idx).accessors.iter()
                                .map(|(k, v)| (k.clone(), *v))
                                .collect();
                        for (aname, vis) in parent_accessors {
                            if child_idx < EXT_BASE {
                                if let std::collections::hash_map::Entry::Vacant(e) = self.ir.tables[child_idx].accessors.entry(aname) {
                                    e.insert(vis);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            if !changed { break; }
        }

        // Store parent_classes on local class tables
        for class in &scan.classes {
            if class.parents.is_empty() { continue; }
            let child_idx = self.ir.classes[&class.name];
            let parent_indices: Vec<TableIndex> = class.parents.iter()
                .filter_map(|p| self.ir.classes.get(p.as_str()).copied())
                .collect();
            // Only set for local tables (not external)
            if child_idx < EXT_BASE {
                self.ir.tables[child_idx].parent_classes = parent_indices;
            }
        }

        // Register local aliases
        for alias in &scan.aliases {
            if let Some(vt) = self.resolve_annotation_type(&alias.typ) {
                self.ir.aliases.insert(alias.name.clone(), vt);
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

    pub(super) fn resolve_annotation_type(&self, at: &AnnotationType) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, &[], &self.ir.classes, &self.ir.aliases)
    }

    /// Like resolve_annotation_type but creates TableInfo entries for table<K,V> parameterized types,
    /// preserving the value type for bracket index resolution.
    pub(super) fn resolve_annotation_type_mut(&mut self, at: &AnnotationType) -> Option<ValueType> {
        if let AnnotationType::Parameterized(base, args) = at {
            if (base == "table" || self.ir.classes.contains_key(base.as_str())) && args.len() == 2 {
                let value_vt = self.resolve_annotation_type(&args[1]);
                let base_vt = crate::annotations::resolve_annotation_type(&AnnotationType::Simple(base.clone()), &[], &self.ir.classes, &self.ir.aliases);
                if let Some(vt) = value_vt {
                    // Create a new TableInfo with the value type
                    let table_idx = self.ir.tables.len();
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
                    self.ir.tables.push(TableInfo {
                        fields,
                        class_name,
                        parent_classes,
                        array_fields: Vec::new(),
                        value_type: Some(vt),
                        accessors,
                        call_func: None,
                    });
                    return Some(ValueType::Table(Some(table_idx)));
                }
                return base_vt;
            }
        }
        self.resolve_annotation_type(at)
    }

    pub(super) fn resolve_annotation_type_gen(&self, at: &AnnotationType, generics: &[(String, Option<String>)]) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, generics, &self.ir.classes, &self.ir.aliases)
    }

    /// Infer generic type variables from structured param annotations.
    /// E.g. for `T[]`, extract element types from the arg's table to infer T.
    pub(super) fn infer_generics_from_annotation(
        &mut self,
        annotation: &AnnotationType,
        generic_names: &[String],
        arg_expr_id: ExprId,
        subs: &mut HashMap<String, ValueType>,
    ) {
        match annotation {
            AnnotationType::Array(inner) => {
                // T[] — infer T from array element types
                if let AnnotationType::Simple(name) = inner.as_ref() {
                    if generic_names.contains(name) && !subs.contains_key(name) {
                        if let Some(elem_type) = self.infer_array_element_type(arg_expr_id) {
                            subs.insert(name.clone(), elem_type);
                        }
                    }
                }
            }
            AnnotationType::Parameterized(_base, args) => {
                // table<K, V> — infer K and V from table field types
                if args.len() == 2 {
                    if let (AnnotationType::Simple(k_name), AnnotationType::Simple(v_name)) = (&args[0], &args[1]) {
                        let k_is_generic = generic_names.contains(k_name) && !subs.contains_key(k_name);
                        let v_is_generic = generic_names.contains(v_name) && !subs.contains_key(v_name);
                        if k_is_generic || v_is_generic {
                            if let Some(table_idx) = self.ir.find_table_index(arg_expr_id) {
                                // Collect field data before calling resolve_expr (avoids borrow conflict)
                                let field_exprs: Vec<ExprId> = self.ir.table(table_idx).fields.values().map(|f| f.expr).collect();
                                let has_fields = !field_exprs.is_empty();
                                if v_is_generic && has_fields {
                                    let field_types: Vec<ValueType> = field_exprs.iter()
                                        .filter_map(|&expr_id| self.resolve_expr(expr_id))
                                        .collect();
                                    if let Some(union_type) = Self::union_of(field_types) {
                                        subs.insert(v_name.clone(), union_type);
                                    }
                                }
                                if k_is_generic && has_fields {
                                    subs.insert(k_name.clone(), ValueType::String);
                                }
                            }
                        }
                    }
                }
            }
            AnnotationType::Backtick(inner) => {
                // `T` — infer T from string literal value as a class name
                if let AnnotationType::Simple(name) = inner.as_ref() {
                    if generic_names.contains(name) {
                        if let Some(str_val) = self.ir.string_literals.get(&arg_expr_id) {
                            if let Some(&table_idx) = self.ir.classes.get(str_val.as_str()) {
                                subs.insert(name.clone(), ValueType::Table(Some(table_idx)));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Compute the element type of an array-like table from its positional fields.
    fn infer_array_element_type(&mut self, expr_id: ExprId) -> Option<ValueType> {
        let table_idx = self.ir.find_table_index(expr_id)?;
        let array_fields: Vec<ExprId> = self.ir.table(table_idx).array_fields.clone();
        if array_fields.is_empty() { return None; }
        let mut types: Vec<ValueType> = Vec::new();
        for &field_expr in &array_fields {
            if let Some(vt) = self.resolve_expr(field_expr) {
                if !types.contains(&vt) {
                    types.push(vt);
                }
            }
        }
        Self::union_of(types)
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

    /// Find the byte range of a `---@field name` comment token for a given class.
    /// If `second` is true, find the second occurrence (for duplicate detection).
    fn find_field_comment_range(root: &SyntaxNode, class_name: &str, field_name: &str, second: bool) -> Option<(u32, u32)> {
        let target = format!("---@field {}", field_name);
        let target_vis = [
            format!("---@field private {}", field_name),
            format!("---@field protected {}", field_name),
            format!("---@field public {}", field_name),
        ];
        let class_marker = format!("---@class {}", class_name);
        let mut in_class = false;
        let mut count = 0u32;
        for event in root.descendants_with_tokens() {
            let rowan::NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            if text.starts_with(&class_marker) {
                in_class = true;
                continue;
            }
            if in_class && text.starts_with("---@class") {
                in_class = false; // different class
                continue;
            }
            if in_class {
                let matches = text.starts_with(&target) || target_vis.iter().any(|t| text.starts_with(t.as_str()));
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
