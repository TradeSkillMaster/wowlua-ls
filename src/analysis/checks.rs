use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::types::*;
use super::Analysis;

// ── Deferred Diagnostic Checks ──────────────────────────────────────────────────

impl Analysis {
    pub(super) fn check_undefined_field_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.undefined_field_checks);
        for UndefinedFieldCheck { table_expr, field, start, end } in checks {
            let Some(table_type) = self.resolve_expr(table_expr) else { continue };
            if matches!(table_type, ValueType::Any) { continue; }
            let table_indices: Vec<TableIndex> = match &table_type {
                ValueType::Table(Some(idx)) => vec![*idx],
                ValueType::Union(types) => types.iter().filter_map(|t| match t {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                }).collect(),
                _ => continue,
            };
            if table_indices.is_empty() { continue; }
            // Re-check: does the field exist now?
            let found = table_indices.iter().any(|&idx| self.ir.has_field(idx, &field));
            if found { continue; }
            // Check parent classes
            let parent_found = table_indices.iter().any(|&idx| {
                self.table(idx).parent_classes.iter().any(|&pi| self.ir.has_field(pi, &field))
            });
            if parent_found { continue; }
            let first_idx = table_indices[0];
            if let Some(class_name) = self.table(first_idx).class_name.clone() {
                crate::diagnostics::undefined_field::check(
                    &mut self.diagnostics,
                    &field, &class_name,
                    start as usize, end as usize,
                );
            }
        }
    }

    pub(super) fn check_return_type_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.return_type_checks);
        for ReturnTypeCheck { func_id, ret_index, rhs_expr, scope_idx, start, end } in checks {
            let func = &self.ir.functions[func_id];
            // Explicitly void function (e.g. inline callback with fun(x: number) annotation)
            if func.explicit_void_return {
                crate::diagnostics::redundant_return_value::check(
                    &mut self.diagnostics,
                    0, ret_index + 1,
                    start as usize, end as usize,
                );
                continue;
            }
            let Some(expected) = func.return_annotations.get(ret_index) else { continue };
            let expected = expected.clone();
            let Some(actual) = self.resolve_expr(rhs_expr) else { continue };
            // Apply narrowing from assert/if guards
            let actual = if actual.contains_nil() || matches!(&actual, ValueType::Union(ts) if ts.contains(&ValueType::Boolean(Some(false)))) {
                if let Some(sym_idx) = self.ir.find_root_symbol(rhs_expr) {
                    if self.is_symbol_falsy_narrowed(sym_idx, scope_idx) {
                        actual.strip_falsy()
                    } else if self.is_symbol_narrowed(sym_idx, scope_idx) {
                        actual.strip_nil()
                    } else if let Expr::FieldAccess { field, .. } = self.expr(rhs_expr) {
                        let field = field.clone();
                        if self.is_field_narrowed(sym_idx, &field, scope_idx) {
                            actual.strip_nil()
                        } else { actual }
                    } else { actual }
                } else { actual }
            } else { actual };
            if actual.is_assignable_to(&expected) {
                continue;
            }
            if self.is_table_subtype(&actual, &expected) {
                self.check_excess_structural_fields(&actual, &expected, start as usize, end as usize);
                continue;
            }
            let expected_str = self.format_value_type_depth(&expected, 1);
            let actual_str = self.format_value_type_depth(&actual, 1);
            crate::diagnostics::return_mismatch::check(
                &mut self.diagnostics,
                &expected_str, &actual_str,
                start as usize, end as usize,
            );
        }
    }

    // ── Field type diagnostics ──────────────────────────────────────────────────

    pub(super) fn check_field_type_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.field_type_checks);
        for FieldTypeCheck { expected, actual_expr, field_name, start, end, lateinit } in checks {
            let Some(actual) = self.resolve_expr(actual_expr) else { continue };
            // Allow nil assignment to lateinit (T!) fields
            if lateinit && matches!(actual, ValueType::Nil) { continue; }
            if actual.is_assignable_to(&expected) {
                continue;
            }
            if self.is_table_subtype(&actual, &expected) {
                self.check_excess_structural_fields(&actual, &expected, start as usize, end as usize);
                continue;
            }
            let expected_str = self.format_value_type_depth(&expected, 1);
            let actual_str = self.format_value_type_depth(&actual, 1);
            crate::diagnostics::field_type_mismatch::check(
                &mut self.diagnostics,
                &field_name, &expected_str, &actual_str,
                start as usize, end as usize,
            );
        }
    }

    // ── Access diagnostics ──────────────────────────────────────────────────────

    /// Walk all Identifier nodes looking for field accesses to private/protected fields.
    pub(super) fn check_access_diagnostics(&mut self) {
        use crate::ast::{AstNode, Identifier};

        for ident_node in self.root.descendants()
            .filter(|n| n.kind() == SyntaxKind::Identifier) {
            let Some(ident) = Identifier::cast(ident_node.clone()) else { continue };
            let names = ident.names();
            if names.len() < 2 { continue; }

            // For each non-root Name in the chain, check access
            let name_tokens: Vec<_> = ident_node.children_with_tokens()
                .filter_map(|it| it.into_token())
                .filter(|t| t.kind() == SyntaxKind::Name)
                .collect();
            if name_tokens.len() < 2 { continue; }

            // Resolve the root to a table
            let root_token = &name_tokens[0];
            let root_offset = rowan::TextSize::from(u32::from(root_token.text_range().start()));
            let Some(scope_idx) = self.scope_at_offset(root_offset) else { continue };
            let Some(root_sym) = self.get_symbol(&SymbolIdentifier::Name(root_token.text().to_string()), scope_idx) else { continue };
            let Some(ver) = self.sym(root_sym).versions.last() else { continue };
            let Some(ValueType::Table(Some(start_table_idx))) = ver.resolved_type.as_ref() else { continue };
            let mut table_idx = *start_table_idx;

            for i in 1..name_tokens.len() {
                let field_name = name_tokens[i].text().to_string();

                // Skip transparent @accessor names
                if self.ir.has_accessor(table_idx, &field_name) {
                    continue;
                }

                let field_vis = self.get_field(table_idx, &field_name).map(|f| f.visibility);

                if let Some(vis) = field_vis {
                    if vis != crate::annotations::Visibility::Public
                        && self.table(table_idx).class_name.is_some()
                    {
                        let enclosing_class = self.find_enclosing_class(&ident_node);
                        let same_class = enclosing_class.is_some_and(|ec| self.same_class(ec, table_idx));
                        let mut is_subclass = enclosing_class.is_some_and(|ec| self.is_subclass_of(ec, table_idx));
                        // If the root variable is a defclass-created instance in this file,
                        // allow protected access at file scope (e.g. CancelScan:OnModuleLoad()).
                        // Private access still requires being inside a colon method.
                        if !is_subclass && vis == crate::annotations::Visibility::Protected {
                            let root_name = root_token.text().to_string();
                            if let Some(&dc_table) = self.defclass_vars.get(&root_name) {
                                is_subclass = self.is_subclass_of(dc_table, table_idx);
                            }
                        }
                        let range = name_tokens[i].text_range();
                        crate::diagnostics::access::check(
                            &mut self.diagnostics, vis, same_class, is_subclass,
                            &field_name,
                            u32::from(range.start()) as usize,
                            u32::from(range.end()) as usize,
                        );
                    }
                }

                // Walk to next table in the chain
                if i < name_tokens.len() - 1 {
                    let Some(field_expr_id) = self.get_field(table_idx, &field_name).map(|f| f.expr) else { break };
                    let Some(ValueType::Table(Some(next_idx))) = self.resolve_expr_type(field_expr_id) else { break };
                    table_idx = next_idx;
                }
            }
        }
    }

    /// Find the class table index of the nearest enclosing method.
    /// Walks up the AST from `node` to find `function Foo:Bar()` or
    /// `function Foo.bar()` / `function Foo.__accessor.bar()` and resolves `Foo`.
    pub(crate) fn find_enclosing_class(&self, node: &SyntaxNode) -> Option<TableIndex> {
        use crate::ast::{AstNode, FunctionDefinition};

        let mut current = node.parent();
        while let Some(n) = current {
            if n.kind() == SyntaxKind::FunctionDefinition {
                if let Some(func_def) = FunctionDefinition::cast(n.clone()) {
                    if let Some(ident) = func_def.identifier() {
                        let names = ident.names();
                        // Match both colon methods (Foo:Bar) and dot-defined functions (Foo.bar, Foo.__static.bar)
                        if names.len() >= 2 {
                            let first_name_token = ident.syntax().children_with_tokens()
                                .filter_map(|it| it.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name)?;
                            let offset = rowan::TextSize::from(u32::from(first_name_token.text_range().start()));
                            let scope_idx = self.scope_at_offset(offset)?;
                            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                            let ver = self.sym(sym_idx).versions.last()?;
                            if let Some(ValueType::Table(Some(idx))) = &ver.resolved_type {
                                return Some(*idx);
                            }
                        }
                    }
                }
            }
            current = n.parent();
        }
        None
    }

    /// Check if two table indices refer to the same class (possibly across local/external).
    pub(crate) fn same_class(&self, a: TableIndex, b: TableIndex) -> bool {
        if a == b { return true; }
        // Check if both resolve to the same class name
        let a_name = self.table(a).class_name.as_deref();
        let b_name = self.table(b).class_name.as_deref();
        a_name.is_some() && a_name == b_name
    }

    /// Check if `child_idx` is the same class as or inherits from `parent_idx`.
    pub(crate) fn is_subclass_of(&self, child_idx: TableIndex, parent_idx: TableIndex) -> bool {
        let mut visited = HashSet::new();
        self.is_subclass_of_inner(child_idx, parent_idx, &mut visited)
    }

    fn is_subclass_of_inner(&self, child_idx: TableIndex, parent_idx: TableIndex, visited: &mut HashSet<TableIndex>) -> bool {
        if self.same_class(child_idx, parent_idx) { return true; }
        if !visited.insert(child_idx) { return false; }
        for &p in &self.table(child_idx).parent_classes {
            if self.is_subclass_of_inner(p, parent_idx, visited) { return true; }
        }
        false
    }

    /// Check if a table type is an `@enum` (numeric enum compatible with `number`).
    fn is_enum_table(&self, idx: TableIndex) -> bool {
        self.table(idx).is_enum
    }

    /// Check if a ValueType contains unresolved type variables (directly or inside tables).
    /// Used to relax overload compatibility checks for generic functions where type variables
    /// haven't been substituted yet (e.g. `T[]` → table with `value_type: TypeVariable("T")`).
    pub(super) fn type_involves_type_variable(&self, vt: &ValueType) -> bool {
        match vt {
            ValueType::TypeVariable(_) => true,
            ValueType::Table(Some(idx)) => {
                let table = self.table(*idx);
                table.value_type.as_ref().is_some_and(|v| self.type_involves_type_variable(v))
                    || table.key_type.as_ref().is_some_and(|k| self.type_involves_type_variable(k))
            }
            ValueType::Union(types) => types.iter().any(|t| self.type_involves_type_variable(t)),
            _ => false,
        }
    }

    /// Check if actual table type is a subtype of expected table type (via class inheritance,
    /// structural field matching, or structural array equivalence).
    pub(super) fn is_table_subtype(&self, actual: &ValueType, expected: &ValueType) -> bool {
        match (actual, expected) {
            // Enum ↔ number: @enum types are integers at runtime
            (ValueType::Table(Some(a)), ValueType::Number) if self.is_enum_table(*a) => true,
            (ValueType::Number, ValueType::Table(Some(b))) if self.is_enum_table(*b) => true,
            (ValueType::Table(Some(a)), ValueType::Table(Some(b))) => {
                if self.is_subclass_of(*a, *b) { return true; }
                let at = self.table(*a);
                let bt = self.table(*b);
                // Structural field matching: table literal → @class type
                // A table literal (no class_name) can satisfy a @class type if it has
                // all required fields with compatible types.
                if at.class_name.is_none() && bt.class_name.is_some() && !at.fields.is_empty() {
                    if self.fields_structurally_match(*a, *b) {
                        return true;
                    }
                }
                // Structural array comparison: both are unnamed array types with matching key/value types
                if at.class_name.is_none() && bt.class_name.is_none() {
                    // A table with no array type info (no key/value types, no array
                    // elements) is compatible with any typed array. In Lua, tables
                    // can serve as both maps and arrays simultaneously, so named
                    // fields don't prevent array usage (e.g. tinsert on {meta=true}).
                    if at.key_type.is_none() && at.value_type.is_none()
                        && at.array_fields.is_empty()
                        && bt.key_type.is_some()
                    {
                        return true;
                    }
                    // Infer key/value types from array_fields for table constructors
                    // like { "a", "b", "c" } that don't have explicit key_type/value_type.
                    let (ak, av) = if at.key_type.is_some() {
                        (at.key_type.clone(), at.value_type.clone())
                    } else if !at.array_fields.is_empty() {
                        let mut types: Vec<ValueType> = Vec::new();
                        let mut resolved_count = 0usize;
                        for &field_expr in &at.array_fields {
                            let vt = match self.expr(field_expr) {
                                Expr::Literal(vt) => Some(vt.clone()),
                                _ => self.resolved_expr_cache.get(&field_expr)
                                    .and_then(|v| v.clone()),
                            };
                            if let Some(vt) = vt {
                                resolved_count += 1;
                                if !types.contains(&vt) {
                                    types.push(vt);
                                }
                            }
                        }
                        // If some elements couldn't be resolved, be conservative
                        if resolved_count < at.array_fields.len() {
                            return true;
                        }
                        (Some(ValueType::Number), Self::union_of(types))
                    } else {
                        (None, None)
                    };
                    if let (Some(ak), Some(av), Some(bk), Some(bv)) =
                        (&ak, &av, &bt.key_type, &bt.value_type)
                    {
                        return (ak.is_assignable_to(bk) || self.is_table_subtype(ak, bk))
                            && (av.is_assignable_to(bv) || self.is_table_subtype(av, bv));
                    }
                }
                false
            }
            // Check if actual table is subtype of any member in expected union
            (ValueType::Table(Some(_)), ValueType::Union(types)) => {
                types.iter().any(|t| self.is_table_subtype(actual, t))
            }
            // All members of actual union must be assignable/subtype of expected
            (ValueType::Union(types), expected) => {
                types.iter().all(|t| t.is_assignable_to(expected) || self.is_table_subtype(t, expected))
            }
            _ => false,
        }
    }

    /// Check if a table literal's fields structurally match a @class type's fields.
    /// Returns true when the literal has all required fields with compatible types.
    fn fields_structurally_match(&self, actual_idx: TableIndex, expected_idx: TableIndex) -> bool {
        // Collect all expected fields including from parent classes
        let expected_fields = self.collect_class_fields(expected_idx);

        for (field_name, expected_type) in &expected_fields {
            let is_optional = matches!(expected_type, ValueType::Union(types) if types.iter().any(|t| *t == ValueType::Nil));
            let at = self.table(actual_idx);
            if let Some(actual_field) = at.fields.get(field_name.as_str()) {
                // Field exists in literal — check type compatibility
                let actual_type = actual_field.annotation.clone().or_else(|| {
                    match self.expr(actual_field.expr) {
                        Expr::Literal(vt) => Some(vt.clone()),
                        _ => self.resolved_expr_cache.get(&actual_field.expr)
                            .and_then(|v| v.clone()),
                    }
                });
                if let Some(actual_type) = actual_type {
                    if !actual_type.is_assignable_to(expected_type)
                        && !self.is_table_subtype(&actual_type, expected_type)
                    {
                        return false;
                    }
                }
                // If actual_type is None (unresolved), be permissive
            } else if !is_optional {
                // Required field missing from literal
                return false;
            }
        }
        true
    }

    /// Emit inject-field diagnostics for excess fields in a table literal that
    /// structurally matched a @class type. Call after is_table_subtype succeeds.
    pub(super) fn check_excess_structural_fields(
        &mut self,
        actual: &ValueType,
        expected: &ValueType,
        range_start: usize,
        range_end: usize,
    ) {
        let (actual_idx, expected_idx) = match (actual, expected) {
            (ValueType::Table(Some(a)), ValueType::Table(Some(b))) => (*a, *b),
            (ValueType::Table(Some(a)), ValueType::Union(types)) => {
                // Find the union member that the structural match succeeded against
                if let Some(b) = types.iter().find_map(|t| match t {
                    ValueType::Table(Some(b)) => {
                        let at = self.table(*a);
                        let bt = self.table(*b);
                        if at.class_name.is_none() && bt.class_name.is_some() && !at.fields.is_empty()
                            && self.fields_structurally_match(*a, *b)
                        {
                            Some(*b)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }) {
                    (*a, b)
                } else {
                    return;
                }
            }
            _ => return,
        };
        let at = self.table(actual_idx);
        let bt = self.table(expected_idx);
        // Only check table literal → @class structural matches
        if at.class_name.is_some() || bt.class_name.is_none() { return; }
        if at.fields.is_empty() { return; }

        let expected_fields = self.collect_class_fields(expected_idx);
        let expected_names: HashSet<&str> = expected_fields.iter().map(|(n, _)| n.as_str()).collect();
        let class_name = bt.class_name.clone().unwrap_or_default();

        let excess: Vec<String> = self.table(actual_idx).fields.keys()
            .filter(|name| !expected_names.contains(name.as_str()))
            .cloned()
            .collect();

        for field_name in excess {
            crate::diagnostics::inject_field::check(
                &mut self.diagnostics,
                &field_name, &class_name,
                range_start, range_end,
            );
        }
    }

    /// Collect all fields for a @class table, including inherited fields from parents.
    fn collect_class_fields(&self, table_idx: TableIndex) -> Vec<(String, ValueType)> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        self.collect_class_fields_inner(table_idx, &mut result, &mut visited);
        result
    }

    fn collect_class_fields_inner(
        &self,
        table_idx: TableIndex,
        result: &mut Vec<(String, ValueType)>,
        visited: &mut HashSet<TableIndex>,
    ) {
        if !visited.insert(table_idx) { return; }
        let table = self.table(table_idx);
        // Add parent fields first (child fields override)
        for &parent_idx in &table.parent_classes {
            self.collect_class_fields_inner(parent_idx, result, visited);
        }
        // Add this table's fields
        for (name, field) in &table.fields {
            // Skip private/protected internal fields like __super
            if name.starts_with("__") { continue; }
            let field_type = field.annotation.clone().or_else(|| {
                match self.expr(field.expr) {
                    Expr::Literal(vt) => Some(vt.clone()),
                    _ => self.resolved_expr_cache.get(&field.expr)
                        .and_then(|v| v.clone()),
                }
            });
            if let Some(ft) = field_type {
                // Remove existing entry if overridden
                result.retain(|(n, _)| n != name);
                result.push((name.clone(), ft));
            }
        }
    }

    /// Structural function compatibility: when both sides are known functions,
    /// check that parameter counts are compatible. A function with fewer parameters
    /// is compatible with one expecting more (extra args are safely discarded in Lua).
    pub(super) fn is_function_compatible(&self, actual: &ValueType, expected: &ValueType) -> bool {
        let (ValueType::Function(Some(actual_idx)), ValueType::Function(Some(expected_idx))) = (actual, expected) else {
            return true; // not both known functions — no structural check
        };
        let actual_fn = self.func(*actual_idx);
        let expected_fn = self.func(*expected_idx);
        // If either is vararg, don't enforce param count
        if actual_fn.is_vararg || expected_fn.is_vararg {
            return true;
        }
        // Fewer params is always safe (extra args discarded), but more params is not
        actual_fn.args.len() <= expected_fn.args.len()
    }

    pub(super) fn check_undefined_global_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.unresolved_globals);
        for UnresolvedGlobal { name, scope_idx, start, end } in checks {
            if self.allowed_read_globals.contains(&name) {
                continue;
            }
            // Re-check: the symbol may have been created later in the file (e.g. global assignment)
            if self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_none() {
                crate::diagnostics::undefined_global::check(
                    &mut self.diagnostics, &name,
                    start as usize, end as usize,
                );
            }
        }
    }

    pub(super) fn check_create_global_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.created_globals);
        for CreatedGlobal { name, start, end } in checks {
            if self.allowed_write_globals.contains(&name) {
                continue;
            }
            // Skip if the name is a known external global (stubs)
            if self.ir.ext.scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) {
                continue;
            }
            if self.ir.framexml_enabled {
                if self.ir.ext.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) {
                    continue;
                }
            }
            // Skip underscore-prefixed names (convention for intentionally ignored)
            if name.starts_with('_') {
                continue;
            }
            crate::diagnostics::create_global::check(
                &mut self.diagnostics, &name,
                start as usize, end as usize,
            );
        }
    }

    pub(super) fn check_unused_local_diagnostics(&mut self) {
        let local_defs = std::mem::take(&mut self.deferred.local_defs);
        for LocalDef { sym_idx, start, end } in local_defs {
            if self.referenced_symbols.contains(&sym_idx) { continue; }
            let name = match &self.ir.symbols[sym_idx].id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => continue,
            };
            // Skip underscore-prefixed names (Lua convention for intentionally unused)
            if name.starts_with('_') { continue; }
            // Emit more specific unused-function for function definitions
            let is_func = self.ir.symbols[sym_idx].versions.last()
                .and_then(|v| v.type_source)
                .map(|e| matches!(self.expr(e), Expr::FunctionDef(_)))
                .unwrap_or(false);
            if is_func {
                crate::diagnostics::unused_function::check(
                    &mut self.diagnostics, &name,
                    start as usize, end as usize,
                );
            } else {
                crate::diagnostics::unused_local::check(
                    &mut self.diagnostics, &name,
                    start as usize, end as usize,
                );
            }
        }
    }

    pub(super) fn check_duplicate_set_field_diagnostics(&mut self) {
        let sites = std::mem::take(&mut self.deferred.field_assignment_sites);
        // Track (table_idx, field_name, scope_idx) -> index in sites vec
        let mut seen: HashMap<(TableIndex, String, ScopeIndex), usize> = HashMap::new();
        for (i, site) in sites.iter().enumerate() {
            let FieldAssignmentSite { table_idx, field_name, scope_idx, block_stmt_index, start, end } = site;
            // Only check @class tables
            let class_name = match &self.table(*table_idx).class_name {
                Some(n) => n.clone(),
                None => continue,
            };
            let key = (*table_idx, field_name.clone(), *scope_idx);
            if let Some(&first_idx) = seen.get(&key) {
                // Two guards prevent false positives:
                // 1. Bracket pattern: don't flag when other fields on the same
                //    table are set between the two assignments (e.g.
                //    state.flag = true; state.other = ...; state.flag = false).
                let has_intervening = sites[first_idx + 1..i].iter().any(|s| {
                    s.table_idx == *table_idx && s.scope_idx == *scope_idx && s.field_name != *field_name
                });
                // 2. Runtime re-assignment: don't flag when there are non-field-
                //    assignment statements (function calls, control flow, etc.)
                //    between the two assignments.
                let stmt_gap = *block_stmt_index as usize - sites[first_idx].block_stmt_index as usize;
                let intervening_in_scope = sites[first_idx + 1..i].iter()
                    .filter(|s| s.scope_idx == *scope_idx)
                    .count();
                let all_intervening_are_field_assigns = stmt_gap == intervening_in_scope + 1;
                if !has_intervening && all_intervening_are_field_assigns {
                    crate::diagnostics::duplicate_set_field::check(
                        &mut self.diagnostics,
                        field_name, &class_name,
                        *start as usize, *end as usize,
                    );
                }
                seen.insert(key, i);
            } else {
                seen.insert(key, i);
            }
        }
    }

    pub(super) fn check_assign_type_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.assign_type_checks);
        for AssignTypeCheck { expected, actual_expr, var_name, start, end } in checks {
            let Some(actual) = self.resolve_expr(actual_expr) else { continue };
            if actual.is_assignable_to(&expected) {
                continue;
            }
            if self.is_table_subtype(&actual, &expected) {
                self.check_excess_structural_fields(&actual, &expected, start as usize, end as usize);
                continue;
            }
            let expected_str = self.format_value_type_depth(&expected, 1);
            let actual_str = self.format_value_type_depth(&actual, 1);
            crate::diagnostics::assign_type_mismatch::check(
                &mut self.diagnostics,
                &var_name, &expected_str, &actual_str,
                start as usize, end as usize,
            );
        }
    }

    pub(super) fn check_nil_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.nil_check_sites);
        let mut seen = HashSet::new();
        for NilCheckSite { scope_idx, table_expr: table_expr_id, start, end } in checks {
            if !seen.insert((start, end)) { continue; }
            let Some(vt) = self.resolve_expr(table_expr_id) else { continue };
            let is_nullable = match &vt {
                ValueType::Union(types) => types.iter().any(|t| *t == ValueType::Nil),
                ValueType::Nil => true,
                _ => false,
            };
            if !is_nullable { continue; }

            if let Some(sym_idx) = self.ir.find_root_symbol(table_expr_id) {
                if self.is_symbol_narrowed(sym_idx, scope_idx) {
                    continue;
                }
                // Check field-level narrowing (e.g. assert(self.field) or if self.field then)
                if let Expr::FieldAccess { field, .. } = self.expr(table_expr_id) {
                    let field = field.clone();
                    if self.is_field_narrowed(sym_idx, &field, scope_idx) {
                        continue;
                    }
                }
            }

            let type_str = self.format_value_type_depth(&vt, 0);
            crate::diagnostics::need_check_nil::check(
                &mut self.diagnostics,
                &type_str,
                start as usize, end as usize,
            );
        }
    }

    pub(super) fn check_missing_return_diagnostics(&mut self) {
        for func_idx in 0..self.ir.functions.len() {
            let func = &self.ir.functions[func_idx];
            if func.return_annotations.is_empty() { continue; }
            // All-optional returns: falling off the end returns nil, which matches Type?
            if func.return_annotations.iter().all(|t| t.contains_nil()) { continue; }
            let Some(func_node) = func.def_node.try_to_node(&self.root) else { continue };
            let Some(block) = func_node.children().find_map(Block::cast) else { continue };
            if !Self::block_ends_with_return(&block) {
                let r = func_node.text_range();
                // Highlight just the first line (function signature)
                let start = u32::from(r.start()) as usize;
                let end = std::cmp::min(start + 40, u32::from(r.end()) as usize);
                crate::diagnostics::missing_return::check(
                    &mut self.diagnostics,
                    start, end,
                );
            }
        }
    }

    pub(super) fn check_malformed_annotations(&mut self) {
        const KNOWN_TAGS: &[&str] = &[
            "class", "field", "alias", "param", "return", "type", "enum",
            "meta", "overload", "defclass", "deprecated", "nodiscard", "constructor",
            "generic", "private", "protected", "accessor", "diagnostic",
            "builds-field", "built-name", "built-extends", "type-narrows",
            "see", "vararg", "as", "cast", "operator", "module", "source",
            "version", "package", "async", "nodoc", "public",
        ];

        for event in self.root.descendants_with_tokens() {
            let rowan::NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            let Some(after_at) = text.strip_prefix("---@") else { continue };
            // Skip @diagnostic — handled by check_diagnostic_codes
            if after_at.starts_with("diagnostic") { continue; }

            let r = tok.text_range();
            let tok_start = u32::from(r.start()) as usize;
            let tok_end = u32::from(r.end()) as usize;

            // Extract the tag name (first word after @)
            let tag = after_at.split(|c: char| c.is_whitespace()).next().unwrap_or("");
            if tag.is_empty() { continue; }

            // Check if the tag is known
            if !KNOWN_TAGS.contains(&tag) {
                // Offset of the tag within the token: "---@" is 4 bytes
                let tag_start = tok_start + 4;
                let tag_end = tag_start + tag.len();
                crate::diagnostics::malformed_annotation::check(
                    &mut self.diagnostics,
                    format!("unknown annotation '@{}'", tag),
                    tag_start, tag_end,
                );
                continue;
            }

            // Check for known tags that are missing required content
            let rest = after_at[tag.len()..].trim();
            let msg = match tag {
                "class" | "enum" if rest.is_empty() || rest.split_whitespace().next().is_none() =>
                    Some(format!("@{} requires a name", tag)),
                "param" if rest.is_empty() =>
                    Some("@param requires a name and type".to_string()),
                "param" if !rest.contains(char::is_whitespace) =>
                    Some("@param requires a type after the parameter name".to_string()),
                "field" => {
                    // Strip optional visibility prefix
                    let rest = rest.strip_prefix("private").map(|r| r.trim_start())
                        .or_else(|| rest.strip_prefix("protected").map(|r| r.trim_start()))
                        .or_else(|| rest.strip_prefix("public").map(|r| r.trim_start()))
                        .unwrap_or(rest);
                    if rest.is_empty() {
                        Some("@field requires a name and type".to_string())
                    } else if !rest.contains(char::is_whitespace) {
                        Some("@field requires a type after the field name".to_string())
                    } else {
                        None
                    }
                }
                "alias" if rest.is_empty() =>
                    Some("@alias requires a name and type".to_string()),
                "alias" if !rest.contains(char::is_whitespace) => {
                    // Name-only @alias is valid when followed by ---| continuation lines
                    let has_continuation = {
                        let mut next = tok.next_token();
                        let mut found = false;
                        while let Some(ref t) = next {
                            let k = t.kind();
                            if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline {
                                next = t.next_token();
                                continue;
                            }
                            if k == SyntaxKind::Comment && t.text().starts_with("---|") {
                                found = true;
                            }
                            break;
                        }
                        found
                    };
                    if has_continuation { None }
                    else { Some("@alias requires a type after the alias name".to_string()) }
                }
                "cast" if rest.is_empty() =>
                    Some("@cast requires a variable name and type".to_string()),
                "cast" if !rest.contains(char::is_whitespace) =>
                    Some("@cast requires a type after the variable name".to_string()),
                "type" if rest.is_empty() =>
                    Some("@type requires a type".to_string()),
                "return" if rest.is_empty() =>
                    Some("@return requires a type".to_string()),
                "overload" if rest.is_empty() =>
                    Some("@overload requires 'fun(...)' signature or 'return:' type list".to_string()),
                "overload" if !rest.starts_with("fun(") && !rest.starts_with("return:") =>
                    Some("@overload requires 'fun(...)' signature or 'return:' type list".to_string()),
                "builds-field" => {
                    if rest.is_empty() {
                        Some("@builds-field requires a parameter index and type (e.g. @builds-field 1 string)".to_string())
                    } else if !rest.contains(char::is_whitespace) {
                        // Has index but no type
                        if rest.parse::<usize>().is_err() {
                            Some("@builds-field requires a numeric parameter index (e.g. @builds-field 1 string)".to_string())
                        } else {
                            Some("@builds-field requires a type after the parameter index (e.g. @builds-field 1 string)".to_string())
                        }
                    } else {
                        let idx_str = rest.split_whitespace().next().unwrap_or("");
                        if idx_str.parse::<usize>().is_err() {
                            Some("@builds-field requires a numeric parameter index (e.g. @builds-field 1 string)".to_string())
                        } else if idx_str == "0" {
                            Some("@builds-field parameter index must be >= 1 (1-based)".to_string())
                        } else {
                            None
                        }
                    }
                }
                "built-name" => {
                    if rest.is_empty() {
                        Some("@built-name requires a parameter index (e.g. @built-name 1)".to_string())
                    } else if let Ok(idx) = rest.trim().parse::<usize>() {
                        if idx == 0 {
                            Some("@built-name parameter index must be >= 1 (1-based)".to_string())
                        } else {
                            None
                        }
                    } else {
                        Some("@built-name requires a numeric parameter index (e.g. @built-name 1)".to_string())
                    }
                }
                _ => None,
            };
            if let Some(message) = msg {
                // Underline the annotation tag
                let tag_start = tok_start + 4; // "---@"
                let tag_end = tag_start + tag.len();
                crate::diagnostics::malformed_annotation::check(
                    &mut self.diagnostics,
                    message,
                    tag_start, std::cmp::min(tag_end, tok_end),
                );
            }
        }
    }

    pub(super) fn check_diagnostic_codes(&mut self) {
        use crate::diagnostics::KNOWN_CODES;
        for event in self.root.descendants_with_tokens() {
            let rowan::NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            let Some(rest) = text.strip_prefix("---@diagnostic") else { continue };
            let rest = rest.trim();
            // Find codes after the colon
            let Some((_keyword, codes_str)) = rest.split_once(':') else {
                // No colon — warn if it looks like codes follow the keyword
                if let Some(space_pos) = rest.find(|c: char| c.is_whitespace()) {
                    let kw = rest[..space_pos].trim();
                    if matches!(kw, "disable" | "enable" | "disable-line" | "disable-next-line") {
                        let r = tok.text_range();
                        let tok_start = u32::from(r.start()) as usize;
                        // Point at the space where the colon should be
                        let directive_offset = text.find("@diagnostic").unwrap_or(0) + "@diagnostic".len();
                        let colon_pos = text[directive_offset..].find(kw).map(|p| directive_offset + p + kw.len());
                        if let Some(pos) = colon_pos {
                            let start = tok_start + pos;
                            crate::diagnostics::malformed_annotation::check(
                                &mut self.diagnostics,
                                format!("Missing ':' after @diagnostic {kw}"),
                                start, start + 1,
                            );
                        }
                    }
                }
                continue;
            };
            let r = tok.text_range();
            let tok_start = u32::from(r.start()) as usize;
            let tok_text = text;
            for code in codes_str.split(',') {
                let code = code.trim();
                if code.is_empty() { continue; }
                if !KNOWN_CODES.contains(&code) {
                    // Find the byte offset of this code within the token
                    if let Some(offset) = tok_text.find(code) {
                        let start = tok_start + offset;
                        let end = start + code.len();
                        crate::diagnostics::unknown_diag_code::check(
                            &mut self.diagnostics, code, start, end,
                        );
                    }
                }
            }
        }
    }

    pub(super) fn check_missing_fields_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.missing_fields_checks);
        for MissingFieldsCheck { class_table_idx, provided_fields, start, end } in checks {
            let table = self.table(class_table_idx);
            let class_name = match &table.class_name {
                Some(n) => n.clone(),
                None => continue,
            };
            // Collect required annotated fields (non-optional, non-function)
            let mut missing: Vec<&str> = Vec::new();
            let field_snapshot: Vec<(String, Option<ValueType>)> = table.fields.iter()
                .map(|(k, v)| (k.clone(), v.annotation.clone()))
                .collect();
            for (field_name, annotation) in &field_snapshot {
                let Some(ann) = annotation else { continue };
                // Optional fields: type includes nil
                let is_nullable = match ann {
                    ValueType::Nil => true,
                    ValueType::Union(types) => types.iter().any(|t| *t == ValueType::Nil),
                    _ => false,
                };
                if is_nullable { continue; }
                // Skip function-typed fields (methods)
                if matches!(ann, ValueType::Function(_)) { continue; }
                // Check if this field was provided in the constructor
                if !provided_fields.iter().any(|p| p == field_name) {
                    missing.push(field_name);
                }
            }
            if !missing.is_empty() {
                missing.sort();
                let missing_refs: Vec<&str> = missing.into_iter().collect();
                crate::diagnostics::missing_fields::check(
                    &mut self.diagnostics,
                    &class_name, &missing_refs,
                    start as usize, end as usize,
                );
            }
        }
    }

    pub(super) fn check_grouped_return_diagnostics(&mut self) {
        let checks = std::mem::take(&mut self.deferred.grouped_return_checks);
        for GroupedReturnCheck { func_id, return_exprs, start, end } in checks {
            let return_only_overloads: Vec<_> = self.ir.func(func_id).overloads.iter()
                .filter(|o| o.is_return_only)
                .cloned()
                .collect();
            if return_only_overloads.is_empty() { continue; }

            // Resolve the actual return types
            let actual_types: Vec<Option<ValueType>> = return_exprs.iter()
                .map(|&expr_id| self.resolve_expr(expr_id))
                .collect();

            // Check if the return values match ANY return-only overload
            let matches_any = return_only_overloads.iter().any(|overload| {
                // Overload with empty returns matches bare return / all nil
                if overload.returns.is_empty() {
                    return actual_types.iter().all(|t| {
                        matches!(t, None | Some(ValueType::Nil))
                    });
                }
                if overload.returns.len() == 1 && overload.returns[0] == ValueType::Nil {
                    return actual_types.iter().all(|t| {
                        matches!(t, None | Some(ValueType::Nil))
                    });
                }
                // Check each position matches the overload's type
                if actual_types.len() != overload.returns.len() { return false; }
                actual_types.iter().zip(overload.returns.iter()).all(|(actual, expected)| {
                    match actual {
                        Some(actual) => actual.is_assignable_to(expected) || self.is_table_subtype(actual, expected),
                        None => true, // unresolved — don't warn
                    }
                })
            });

            if !matches_any {
                let overload_desc: Vec<String> = return_only_overloads.iter()
                    .map(|o| {
                        if o.returns.is_empty() || (o.returns.len() == 1 && o.returns[0] == ValueType::Nil) {
                            "nil".to_string()
                        } else {
                            o.returns.iter()
                                .map(|vt| self.format_value_type_depth(vt, 1))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    })
                    .collect();
                let desc = overload_desc.join(" | ");
                crate::diagnostics::grouped_return_mismatch::check(
                    &mut self.diagnostics,
                    &desc,
                    start as usize, end as usize,
                );
            }
        }
    }

    pub(super) fn block_ends_with_return(block: &Block) -> bool {
        Self::block_always_exits(block)
    }

    pub(super) fn block_always_exits(block: &Block) -> bool {
        let statements = block.statements();
        let Some(last) = statements.last() else { return false };
        match last {
            Statement::Return(_) => true,
            Statement::FunctionCall(call) => {
                // error() never returns
                if let Some(ident) = call.identifier() {
                    let names = ident.names();
                    names.len() == 1 && names[0] == "error"
                } else {
                    false
                }
            }
            Statement::If(if_chain) => {
                // All branches must exit, and there must be an else
                let branches = if_chain.if_branches();
                let else_branch = if_chain.else_branch();
                if else_branch.is_none() { return false; }
                for branch in &branches {
                    if let Some(block) = branch.block() {
                        if !Self::block_always_exits(&block) { return false; }
                    } else {
                        return false;
                    }
                }
                if let Some(eb) = &else_branch {
                    if let Some(block) = eb.block() {
                        Self::block_always_exits(&block)
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
}

