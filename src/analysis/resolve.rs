use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::types::*;
use super::Analysis;

// ── Type Resolution (Phase 2) ──────────────────────────────────────────────────

impl<'a> Analysis<'a> {
    pub fn resolve_types(&mut self) {
        // Pre-resolve annotated return symbols so they're available before
        // the main resolution loop tries to resolve callers
        for func_idx in 0..self.ir.functions.len() {
            let func = &self.ir.functions[func_idx];
            if func.return_annotations.is_empty() {
                continue;
            }
            let scope = func.scope;
            for (i, vt) in func.return_annotations.clone().iter().enumerate() {
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, i);
                if let Some(ret_sym_idx) = self.get_symbol(&ret_id, scope) {
                    if let Some(ver) = self.ir.symbols[ret_sym_idx].versions.first_mut() {
                        if ver.resolved_type.is_none() {
                            ver.resolved_type = Some(vt.clone());
                        }
                    }
                }
            }
        }

        let mut pending: Vec<(SymbolIndex, usize)> = Vec::new();
        for (si, sym) in self.ir.symbols.iter().enumerate() {
            for (vi, ver) in sym.versions.iter().enumerate() {
                if ver.type_source.is_some() && ver.resolved_type.is_none() {
                    pending.push((si, vi));
                }
            }
        }

        // Collect call expressions not already backing a symbol's type_source
        let symbol_exprs: std::collections::HashSet<ExprId> = self.ir.symbols.iter()
            .flat_map(|s| s.versions.iter())
            .filter_map(|v| v.type_source)
            .collect();
        let mut pending_calls: Vec<ExprId> = self.deferred.call_exprs.iter()
            .copied()
            .filter(|id| !symbol_exprs.contains(id))
            .collect();

        // Collect table field expressions that need resolving. These aren't backed by
        // any symbol's type_source, so the fixpoint loop must resolve them explicitly
        // to handle @builds-field / @built-name / @return self / @return built chains.
        // Includes both:
        //   - Overlay fields (external table field assignments like `Element._STATE_SCHEMA = ...`)
        //   - Local table fields set inside constructors (like `self._state = ...`)
        let mut pending_field_exprs: Vec<ExprId> = self.ir.overlay_fields.values()
            .flat_map(|fields| fields.values())
            .flat_map(|fi| std::iter::once(fi.expr).chain(fi.extra_exprs.iter().copied()))
            .filter(|id| !symbol_exprs.contains(id))
            .collect();
        // Also collect field expressions from local tables (< EXT_BASE) with class names
        for table in self.ir.tables.iter() {
            if table.class_name.is_some() {
                for fi in table.fields.values() {
                    if !symbol_exprs.contains(&fi.expr) {
                        pending_field_exprs.push(fi.expr);
                    }
                    for &extra in &fi.extra_exprs {
                        if !symbol_exprs.contains(&extra) {
                            pending_field_exprs.push(extra);
                        }
                    }
                }
            }
        }

        // Unified fixpoint: resolve both symbol type sources and standalone call expressions.
        // Call expressions can propagate param types (e.g. fun() annotations on inline
        // callbacks) which unblock symbol resolution, and vice versa.
        const MAX_FIXPOINT_ITERATIONS: usize = 50;
        let mut iteration = 0;
        loop {
            iteration += 1;
            if iteration > MAX_FIXPOINT_ITERATIONS {
                self.safety_limit_hit = Some(format!(
                    "type resolution did not converge after {MAX_FIXPOINT_ITERATIONS} iterations \
                     (tables={}, exprs={})", self.ir.tables.len(), self.ir.exprs.len()
                ));
                break;
            }
            let prev_sym_len = pending.len();
            let prev_call_len = pending_calls.len();
            let prev_field_len = pending_field_exprs.len();

            // Inner loop: repeat the three retain passes until no more progress
            // is made within this outer iteration. This collapses dependency chains
            // (where symbol A depends on symbol B later in the list) from O(N) outer
            // iterations into a single outer iteration.
            loop {
                let inner_total = pending.len() + pending_calls.len() + pending_field_exprs.len();

                pending.retain(|&(si, vi)| {
                    let expr_id = self.ir.symbols[si].versions[vi].type_source.unwrap();
                    let is_branch_merge = matches!(self.expr(expr_id), Expr::BranchMerge(_));
                    if is_branch_merge {
                        // BranchMerge may produce a partial union when some branches
                        // haven't resolved yet. Clear the cache so we re-evaluate with
                        // any newly resolved branches from this iteration.
                        self.resolved_expr_cache.remove(&expr_id);
                    }
                    if let Some(resolved) = self.resolve_expr(expr_id) {
                        let prev = self.ir.symbols[si].versions[vi].resolved_type.replace(resolved.clone());
                        if is_branch_merge && prev.as_ref() != Some(&resolved) {
                            // BranchMerge result changed — keep in pending for another
                            // iteration so that newly resolved branches can contribute.
                            true
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                });

                pending_calls.retain(|&expr_id| {
                    // A call is "processed" once its function identity resolves,
                    // even if the call returns None (e.g. void-returning functions).
                    // Check function resolvability to avoid re-running side effects.
                    let func_resolvable = match self.expr(expr_id) {
                        Expr::FunctionCall { func, .. } => {
                            let func = *func;
                            self.resolve_expr(func).is_some()
                        }
                        _ => false,
                    };
                    if func_resolvable {
                        self.resolve_expr(expr_id);
                        false
                    } else {
                        true
                    }
                });

                // Resolve table field expressions (builder chains on class fields)
                pending_field_exprs.retain(|&expr_id| {
                    self.resolve_expr(expr_id).is_none()
                });

                // Infer key/value types for tables with bracket assignments.
                // Must run inside the fixpoint loop so that BracketIndex
                // expressions can resolve once value_type is set.
                if self.infer_bracket_field_types() {
                    // New table types were set — continue the inner loop so
                    // BracketIndex expressions get another chance to resolve.
                    continue;
                }

                // Process deferred sibling narrowings: resolve cross-file FieldAccess
                // callees and apply StripNil versions if the function has return-only overloads.
                self.resolve_deferred_sibling_narrowings(&mut pending);

                let new_total = pending.len() + pending_calls.len() + pending_field_exprs.len();
                if new_total == inner_total {
                    break;
                }
            }

            if pending.len() == prev_sym_len && pending_calls.len() == prev_call_len && pending_field_exprs.len() == prev_field_len {
                // Before giving up, try re-resolving param annotations that reference
                // @built-name classes discovered during this fixpoint loop.
                let mut new_resolution = false;
                for func_idx in 0..self.ir.functions.len() {
                    let param_annotations = self.ir.functions[func_idx].param_annotations.clone();
                    let func_args = self.ir.functions[func_idx].args.clone();
                    for (i, ann) in param_annotations.iter().enumerate() {
                        let Some(&sym_idx) = func_args.get(i) else { continue };
                        if sym_idx >= EXT_BASE { continue; }
                        let current_type = self.ir.symbols[sym_idx].versions.first()
                            .and_then(|v| v.resolved_type.clone());
                        // Re-resolve if unresolved
                        if current_type.is_none() {
                            if let Some(vt) = self.resolve_annotation_type(ann) {
                                self.ir.symbols[sym_idx].versions[0].resolved_type = Some(vt);
                                // Store type args for parameterized annotations
                                if let crate::annotations::AnnotationType::Parameterized(_, type_arg_anns) = ann {
                                    let type_args: Vec<ValueType> = type_arg_anns.iter()
                                        .filter_map(|ta| self.resolve_annotation_type(ta))
                                        .collect();
                                    if !type_args.is_empty() {
                                        self.ir.symbols[sym_idx].versions[0].type_args = type_args;
                                    }
                                }
                                new_resolution = true;
                            }
                            continue;
                        }
                        // Update param pointers to the latest @built-name class table
                        // index. When the table moves (e.g. from a pre-registered empty
                        // ext class to a populated ir class), this counts as new progress
                        // so field accesses get re-evaluated.
                        if let Some(ValueType::Table(Some(old_idx))) = &current_type {
                            if let Some(class_name) = self.table(*old_idx).class_name.clone() {
                                if let Some(&new_idx) = self.ir.classes.get(&class_name) {
                                    if new_idx != *old_idx {
                                        self.ir.symbols[sym_idx].versions[0].resolved_type =
                                            Some(ValueType::Table(Some(new_idx)));
                                        new_resolution = true;
                                    }
                                }
                            }
                        }
                    }
                }
                if !new_resolution {
                    break;
                }
                // Clear expression cache so dependent expressions (e.g. field access
                // on re-resolved params) get re-evaluated in the next fixpoint iteration.
                self.resolved_expr_cache.clear();
            }
        }

        self.resolve_deep_field_injections();
        self.resolve_deferred_field_assignments();
        self.check_undefined_field_diagnostics();
        self.check_return_type_diagnostics();
        self.check_field_type_diagnostics();
        self.check_assign_type_diagnostics();
        self.check_access_diagnostics();
        self.check_nil_diagnostics();
        self.check_undefined_global_diagnostics();
        self.check_create_global_diagnostics();
        self.check_unused_local_diagnostics();
        self.check_duplicate_set_field_diagnostics();
        self.check_missing_fields_diagnostics();
        self.check_grouped_return_diagnostics();
        self.check_missing_return_diagnostics();
        self.check_diagnostic_codes();
        self.check_malformed_annotations();

        // Remove inject-field false positives for fields that now exist after Phase 2
        // (e.g. builder-pattern fields from @builds-field / @built-name resolution)
        self.remove_inject_field_false_positives();

        // Remove undefined-doc-class diagnostics for classes registered during resolution
        // (e.g. @built-name classes discovered during the fixpoint loop)
        self.diagnostics.retain(|d| {
            if d.code == crate::diagnostics::undefined_doc_class::CODE {
                // Extract class name from message "undefined class 'Foo'"
                if let Some(name) = d.message.strip_prefix("undefined class '").and_then(|s| s.strip_suffix('\'')) {
                    if self.ir.classes.contains_key(name) || self.ir.ext.classes.contains_key(name) {
                        return false;
                    }
                }
            }
            true
        });

        // Deduplicate diagnostics (resolve loop may emit the same diagnostic multiple times)
        {
            let mut seen = std::collections::HashSet::new();
            self.diagnostics.retain(|d| seen.insert((d.code, d.start, d.end)));
        }

        // Emit a visible diagnostic if a safety limit was hit
        if let Some(ref msg) = self.safety_limit_hit {
            self.diagnostics.push(crate::diagnostics::WowDiagnostic {
                code: "safety-limit",
                message: format!("analysis incomplete: {msg}; some types and diagnostics may be missing"),
                severity: lsp_types::DiagnosticSeverity::ERROR,
                start: 0,
                end: 0,
            });
        }
    }

    /// After the fixpoint loop, infer `key_type`/`value_type` for table constructors
    /// that have bracket-keyed fields (or array fields) but couldn't be fully resolved
    /// at Phase 1 (literals only).
    fn infer_bracket_field_types(&mut self) -> bool {
        let table_indices: Vec<TableIndex> = self.ir.bracket_key_fields.keys().copied().collect();
        let mut made_progress = false;
        for table_idx in table_indices {
            let already_resolved = self.ir.tables[table_idx].key_type.is_some();

            // If key_type/value_type were already set (Phase 1 literals or earlier
            // fixpoint iteration), update value_type from bracket assignment types.
            // Bracket-indexed assignments overwrite elements, so the assigned type
            // replaces the original element type (e.g. `parts[i] = parseInt(parts[i])`
            // changes a string[] to number[]).
            if already_resolved {
                let bracket_fields = self.ir.bracket_key_fields[&table_idx].clone();
                let mut new_types: Vec<ValueType> = Vec::new();
                let mut all_resolved = true;
                for (_key_expr, val_expr) in &bracket_fields {
                    if let Some(vt) = self.resolve_expr_to_broad_type(*val_expr) {
                        if !new_types.contains(&vt) { new_types.push(vt); }
                    } else {
                        all_resolved = false;
                    }
                }
                if all_resolved && !new_types.is_empty() {
                    let new_vt = if new_types.len() == 1 { new_types.pop().unwrap() }
                                 else { ValueType::make_union(new_types) };
                    if self.ir.tables[table_idx].value_type.as_ref() != Some(&new_vt) {
                        self.ir.tables[table_idx].value_type = Some(new_vt);
                        made_progress = true;
                    }
                }
                continue;
            }

            let bracket_fields = self.ir.bracket_key_fields[&table_idx].clone();
            let array_fields = self.ir.tables[table_idx].array_fields.clone();

            let mut key_types: Vec<ValueType> = Vec::new();
            let mut val_types: Vec<ValueType> = Vec::new();
            let mut all_resolved = true;

            for (key_expr, val_expr) in &bracket_fields {
                if let Some(kt) = self.resolve_expr_to_broad_type(*key_expr) {
                    if !key_types.contains(&kt) { key_types.push(kt); }
                } else {
                    all_resolved = false;
                }
                if let Some(vt) = self.resolve_expr_to_broad_type(*val_expr) {
                    if !val_types.contains(&vt) { val_types.push(vt); }
                } else {
                    all_resolved = false;
                }
            }

            // Also consider array (positional) fields
            if !array_fields.is_empty() {
                if !key_types.contains(&ValueType::Number) {
                    key_types.push(ValueType::Number);
                }
                for af in &array_fields {
                    if let Some(vt) = self.resolve_expr_to_broad_type(*af) {
                        if !val_types.contains(&vt) { val_types.push(vt); }
                    } else {
                        all_resolved = false;
                    }
                }
            }

            if !all_resolved || key_types.is_empty() || val_types.is_empty() {
                continue;
            }

            let key = if key_types.len() == 1 { key_types.pop().unwrap() }
                      else { ValueType::make_union(key_types) };
            let val = if val_types.len() == 1 { val_types.pop().unwrap() }
                      else { ValueType::make_union(val_types) };
            self.ir.tables[table_idx].key_type = Some(key);
            self.ir.tables[table_idx].value_type = Some(val);
            made_progress = true;
        }
        made_progress
    }

    /// Resolve an expression to its broad type (stripping specific literal values).
    fn resolve_expr_to_broad_type(&mut self, expr_id: ExprId) -> Option<ValueType> {
        let resolved = self.resolve_expr(expr_id)?;
        Some(Self::broaden_type(resolved))
    }

    /// Strip specific literal values from a type, keeping only the broad category.
    fn broaden_type(vt: ValueType) -> ValueType {
        match vt {
            ValueType::String(_) => ValueType::String(None),
            ValueType::Boolean(_) => ValueType::Boolean(None),
            ValueType::Union(types) => {
                let broad: Vec<ValueType> = types.into_iter().map(Self::broaden_type).collect();
                ValueType::make_union(broad)
            }
            other => other,
        }
    }

    /// Process deferred sibling narrowings from build_ir. These are multi-return siblings
    /// where the callee was a FieldAccess that couldn't be resolved at build time (cross-file).
    /// Now during the fixpoint loop, try to resolve the func_expr and check for return-only
    /// overloads. If found, create StripNil versions for the siblings.
    fn resolve_deferred_sibling_narrowings(&mut self, pending: &mut Vec<(SymbolIndex, usize)>) {
        if self.deferred_sibling_narrowings.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.deferred_sibling_narrowings);
        let mut remaining = Vec::new();
        for (func_expr, siblings, scope_idx) in entries {
            // Try to resolve the func expression to get the function type
            let func_type = self.resolve_expr(func_expr);
            let has_return_overloads = match func_type {
                Some(ValueType::Function(Some(func_idx))) => {
                    self.ir.func(func_idx).overloads.iter().any(|o| o.is_return_only)
                }
                Some(_) => false, // Resolved but not a function — no overloads
                None => {
                    // Still can't resolve — keep for next iteration
                    remaining.push((func_expr, siblings, scope_idx));
                    continue;
                }
            };
            if has_return_overloads {
                for &(_, sibling_idx) in &siblings {
                    // Skip siblings that are already narrowed (including the guarded one)
                    if self.narrowed_symbols.get(&scope_idx).is_some_and(|s| s.contains(&sibling_idx)) {
                        continue;
                    }
                    self.narrowed_symbols.entry(scope_idx).or_default().insert(sibling_idx);
                    if let Some(new_ver) = self.ir.push_strip_nil_version(sibling_idx, scope_idx) {
                        // Add to pending so the fixpoint loop resolves the new version
                        pending.push((sibling_idx, new_ver));
                    }
                }
            }
        }
        self.deferred_sibling_narrowings = remaining;
    }

    /// After the fixpoint loop, resolve deep field injections (e.g. `self._plot.dot = expr`)
    /// by walking the intermediate chain to find the actual target table, then adding the
    /// field there so deferred undefined-field checks can find it.
    fn resolve_deep_field_injections(&mut self) {
        let injections = std::mem::take(&mut self.deferred.deep_field_injections);
        for inj in injections {
            let Some(mut current_table) = self.ir.find_table_for_symbol(&inj.root_name, inj.scope_idx)
                else { continue };

            // Walk intermediates to find the actual target table
            let mut resolved = true;
            for intermediate in &inj.intermediates {
                // Extract field data without holding a borrow on self
                let field_data = self.ir.get_field(current_table, intermediate)
                    .map(|fi| (fi.annotation.clone(), fi.expr, fi.extra_exprs.clone()));
                let Some((annotation, expr, extras)) = field_data else {
                    resolved = false;
                    break;
                };
                let table_type = annotation.or_else(|| {
                    // Mirror FieldAccess resolution: if there are reassignments and the
                    // initial value is nil, skip the nil placeholder.
                    let skip_primary = !extras.is_empty()
                        && matches!(self.resolve_expr(expr), Some(ValueType::Nil));
                    let all_exprs: Vec<ExprId> = if skip_primary {
                        extras.clone()
                    } else {
                        std::iter::once(expr).chain(extras.iter().copied()).collect()
                    };
                    let mut types: Vec<ValueType> = Vec::new();
                    for e in all_exprs {
                        if let Some(vt) = self.resolve_expr(e) {
                            if !types.contains(&vt) {
                                types.push(vt);
                            }
                        }
                    }
                    if types.is_empty() { None } else { Some(ValueType::make_union(types)) }
                });
                match table_type {
                    Some(ValueType::Table(Some(idx))) => current_table = idx,
                    Some(ValueType::Union(ref types)) => {
                        if let Some(idx) = types.iter().find_map(|t| match t {
                            ValueType::Table(Some(idx)) => Some(*idx),
                            _ => None,
                        }) {
                            current_table = idx;
                        } else { resolved = false; break; }
                    }
                    _ => { resolved = false; break; }
                }
            }
            if !resolved { continue; }

            // Add field to the correct target table
            if !self.ir.has_field(current_table, &inj.field_name) {
                let deep_vis = if inj.root_name == "self" {
                    crate::annotations::default_visibility_for_name(&inj.field_name)
                } else {
                    crate::annotations::Visibility::Public
                };
                let fi = FieldInfo {
                    expr: inj.expr_id,
                    extra_exprs: Vec::new(),
                    visibility: deep_vis,
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                };
                if current_table < EXT_BASE {
                    self.ir.tables[current_table].fields.insert(inj.field_name, fi);
                } else {
                    self.ir.insert_overlay_field(current_table, inj.field_name, fi);
                }
            }
        }
    }

    /// After the fixpoint loop, resolve field assignments on variables whose class table
    /// wasn't known during Phase 1 (e.g. type comes from a function return).
    fn resolve_deferred_field_assignments(&mut self) {
        let assignments = std::mem::take(&mut self.deferred.deferred_field_assignments);
        for assign in assignments {
            // Try to find the class table via the symbol's resolved type
            let sym_idx = match self.ir.get_symbol(
                &SymbolIdentifier::Name(assign.root_name.clone()),
                assign.scope_idx,
            ) {
                Some(idx) => idx,
                None => continue,
            };
            let ver_idx = self.ir.sym(sym_idx).versions.len() - 1;
            let table_idx = self.ir.sym(sym_idx).versions[ver_idx].type_source
                .and_then(|ts| self.ir.find_table_index(ts))
                .or_else(|| {
                    // Fall back to resolved_type for function-return-typed symbols
                    match &self.ir.sym(sym_idx).versions[ver_idx].resolved_type {
                        Some(ValueType::Table(Some(idx))) => Some(*idx),
                        Some(ValueType::Union(types)) => types.iter().find_map(|t| match t {
                            ValueType::Table(Some(idx)) => Some(*idx),
                            _ => None,
                        }),
                        _ => None,
                    }
                });
            let Some(table_idx) = table_idx else { continue };

            // Emit inject-field diagnostic if appropriate.
            // Use class_has_field() which walks built_table and parent chains,
            // and also re-lookup via ir.classes in case Phase 2 updated the table.
            let field_already_exists = self.class_has_field(table_idx, &assign.field_name);
            if !field_already_exists {
                let table = self.table(table_idx);
                let has_annotations = table.fields.values().any(|f| f.annotation.is_some());
                if table.class_name.is_some() && has_annotations {
                    let class_name = table.class_name.clone().unwrap_or_default();
                    // Also check via class name lookup — Phase 2 may have updated
                    // ir.classes to point to a different table with built fields.
                    let class_table_idx = self.ir.classes.get(&class_name).copied();
                    if class_table_idx.map_or(true, |ci| !self.class_has_field(ci, &assign.field_name)) {
                        crate::diagnostics::inject_field::check(
                            &mut self.diagnostics,
                            &assign.field_name, &class_name,
                            assign.ident_start as usize, assign.ident_end as usize,
                        );
                    }
                }
            }

            // Register the field on the table — ad-hoc injected fields default to Public;
            // self._foo inside a method keeps implicit protected from _ prefix.
            let vis = if assign.root_name == "self" {
                crate::annotations::default_visibility_for_name(&assign.field_name)
            } else {
                crate::annotations::Visibility::Public
            };
            if table_idx < EXT_BASE {
                if let Some(fi) = self.ir.tables[table_idx].fields.get_mut(&assign.field_name) {
                    fi.extra_exprs.push(assign.expr_id);
                } else {
                    self.ir.tables[table_idx].fields.insert(assign.field_name.clone(), FieldInfo {
                        expr: assign.expr_id,
                        extra_exprs: Vec::new(),
                        visibility: vis,
                        annotation: None,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        def_range: None,
                    });
                }
            } else {
                if let Some(overlay_fi) = self.ir.get_overlay_field_mut(table_idx, &assign.field_name) {
                    overlay_fi.extra_exprs.push(assign.expr_id);
                } else {
                    self.ir.insert_overlay_field(table_idx, assign.field_name.clone(), FieldInfo {
                        expr: assign.expr_id,
                        extra_exprs: Vec::new(),
                        visibility: vis,
                        annotation: None,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        def_range: None,
                    });
                }
            }
        }
    }

    /// Maximum recursion depth for expression resolution. Prevents stack overflow
    /// on deeply nested builder chains or pathological field access patterns.
    const MAX_RESOLVE_DEPTH: usize = 200;

    /// Minimum chain length (in expression nodes) to trigger iterative resolution.
    /// Each method call contributes 2 nodes (FunctionCall + FieldAccess), so this
    /// threshold of 40 catches chains of 20+ chained method calls.
    const ITERATIVE_CHAIN_THRESHOLD: usize = 40;

    /// Collect a method-call chain bottom-up starting from `expr_id`.
    /// Walks FunctionCall → FieldAccess → FunctionCall → ... links, stopping at
    /// any non-chain expression or already-cached node. Returns the chain in
    /// bottom-up order (root receiver's immediate dependents first).
    fn collect_call_chain(&self, expr_id: ExprId) -> Vec<ExprId> {
        let mut chain = Vec::new();
        let mut current = expr_id;
        loop {
            if self.resolved_expr_cache.contains_key(&current) {
                break;
            }
            match self.expr(current) {
                Expr::FunctionCall { func, .. } => {
                    let func = *func;
                    chain.push(current);
                    match self.expr(func) {
                        Expr::FieldAccess { table, .. } => {
                            let table = *table;
                            chain.push(func);
                            current = table;
                        }
                        _ => break,
                    }
                }
                Expr::FieldAccess { table, .. } => {
                    let table = *table;
                    chain.push(current);
                    current = table;
                }
                _ => break,
            }
        }
        chain.reverse();
        chain
    }

    /// Resolve a chain of expressions iteratively (bottom-up).
    /// Each link's dependencies are resolved before it, so recursive depth per
    /// link stays at O(1) via cache hits instead of O(chain_length).
    fn resolve_chain_iteratively(&mut self, chain: &[ExprId]) -> Option<ValueType> {
        let mut last_result = None;
        for &expr_id in chain {
            if let Some(cached) = self.resolved_expr_cache.get(&expr_id) {
                last_result = cached.clone();
                continue;
            }
            if !self.resolving_exprs.insert(expr_id) {
                return None;
            }
            self.resolve_depth += 1;
            let result = self.resolve_expr_inner(expr_id);
            self.resolve_depth -= 1;
            self.resolving_exprs.remove(&expr_id);
            // Only cache successful resolutions — None means "not yet resolvable,
            // retry next fixpoint iteration", matching resolve_expr() semantics.
            if result.is_some() {
                self.resolved_expr_cache.insert(expr_id, result.clone());
            }
            last_result = result;
            if last_result.is_none() {
                break;
            }
        }
        last_result
    }

    pub(super) fn resolve_expr(&mut self, expr_id: ExprId) -> Option<ValueType> {
        // Return cached result if available (avoids re-creating tables/exprs
        // for builder chains on each fixpoint iteration)
        if let Some(cached) = self.resolved_expr_cache.get(&expr_id) {
            return cached.clone();
        }
        // For deep method-call chains (builder patterns), resolve iteratively
        // bottom-up instead of recursively to avoid hitting the depth limit.
        // Only check at shallow depth to avoid overhead during normal resolution.
        if self.resolve_depth < 3 && matches!(self.expr(expr_id), Expr::FunctionCall { .. }) {
            let chain = self.collect_call_chain(expr_id);
            if chain.len() >= Self::ITERATIVE_CHAIN_THRESHOLD {
                return self.resolve_chain_iteratively(&chain);
            }
        }
        // Depth limit: prevent stack overflow on deeply nested chains
        if self.resolve_depth >= Self::MAX_RESOLVE_DEPTH {
            if self.safety_limit_hit.is_none() {
                self.safety_limit_hit = Some(format!(
                    "expression resolution exceeded depth limit ({})", Self::MAX_RESOLVE_DEPTH
                ));
            }
            return None;
        }
        // Cycle detection: if we're already resolving this expr, break the cycle
        if !self.resolving_exprs.insert(expr_id) {
            return None;
        }
        self.resolve_depth += 1;
        let result = self.resolve_expr_inner(expr_id);
        self.resolve_depth -= 1;
        self.resolving_exprs.remove(&expr_id);
        // Cache successful resolutions (None = not yet resolvable, retry next iteration)
        if result.is_some() {
            self.resolved_expr_cache.insert(expr_id, result.clone());
        }
        result
    }

    fn resolve_expr_inner(&mut self, expr_id: ExprId) -> Option<ValueType> {
        // Fast path: leaf variants that don't need &mut self (avoids cloning heap data)
        match self.expr(expr_id) {
            Expr::Literal(vt) => return Some(vt.clone()),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                return self.sym(*sym_idx).versions[*ver_idx].resolved_type.clone();
            }
            Expr::FunctionDef(func_idx) => return Some(ValueType::Function(Some(*func_idx))),
            Expr::TableConstructor(table_idx) => return Some(ValueType::Table(Some(*table_idx))),
            Expr::StripNil(inner) => {
                let inner = *inner;
                return self.resolve_expr(inner).map(|vt| vt.strip_nil());
            }
            Expr::StripFalsy(inner) => {
                let inner = *inner;
                return self.resolve_expr(inner).map(|vt| vt.strip_falsy());
            }
            Expr::CastAdd(inner, cast_type) => {
                let inner = *inner;
                let cast_type = cast_type.clone();
                return self.resolve_expr(inner).map(|vt| ValueType::union(vt, cast_type));
            }
            Expr::CastRemove(inner, cast_type) => {
                let inner = *inner;
                let cast_type = cast_type.clone();
                return self.resolve_expr(inner).map(|vt| vt.strip_type(&cast_type));
            }
            Expr::TypeFilter(inner, guard_type) => {
                let inner = *inner;
                let guard_type = guard_type.clone();
                let resolved = self.resolve_expr(inner);
                return resolved.map(|vt| vt.filter_type_with(&guard_type, &|idx| self.table(idx).is_enum));
            }
            Expr::BranchMerge(exprs) => {
                let exprs = exprs.clone();
                let mut types: Vec<ValueType> = Vec::new();
                for eid in exprs {
                    if let Some(vt) = self.resolve_expr(eid) {
                        types.push(vt);
                    }
                }
                return if types.is_empty() { None } else { Some(ValueType::make_union(types)) };
            }
            Expr::Unknown => return None,
            _ => {}
        }
        // Remaining variants need &mut self — clone to release the borrow
        let expr = self.expr(expr_id).clone();
        match &expr {
            Expr::BinaryOp { op, lhs, rhs } => {
                let op = *op;
                let lhs_type = self.resolve_expr(*lhs);
                let rhs_type = self.resolve_expr(*rhs);
                match (lhs_type, rhs_type) {
                    (Some(l), Some(r)) => self.resolve_binary_op(op, l, r),
                    // Arithmetic with at least one Number operand yields Number (e.g. x = x + 1)
                    (Some(ValueType::Number), None) | (None, Some(ValueType::Number))
                        if op.is_arithmetic() => Some(ValueType::Number),
                    // Concatenation with at least one string-like operand yields String
                    (Some(ref t), None) | (None, Some(ref t))
                        if op == Operator::Concatenate && t.can_concat_to_string() => Some(ValueType::String(None)),
                    // Comparisons always yield boolean
                    _ if op.is_comparison() => Some(ValueType::Boolean(None)),
                    _ => None,
                }
            }

            Expr::UnaryOp { op, operand } => {
                let operand_type = self.resolve_expr(*operand)?;
                match op {
                    Operator::Not => Some(ValueType::Boolean(None)),
                    Operator::Subtract => {
                        match &operand_type {
                            ValueType::Number => Some(ValueType::Number),
                            _ => self.resolve_unary_metamethod(*op, &operand_type),
                        }
                    }
                    Operator::ArrayLength => {
                        match &operand_type {
                            ValueType::Table(Some(_)) => {
                                // Check __len metamethod first, fall back to number
                                self.resolve_unary_metamethod(*op, &operand_type)
                                    .or(Some(ValueType::Number))
                            }
                            _ => Some(ValueType::Number),
                        }
                    }
                    _ => None,
                }
            }

            Expr::Grouped(inner) => self.resolve_expr(*inner),

            Expr::FunctionCall { func, args, arg_ranges, ret_index, call_range, discarded, is_method_call } => {
                let call_range = *call_range;
                let discarded = *discarded;
                let is_method_call = *is_method_call;
                let func_expr_id = *func;
                let arg_ranges = arg_ranges.clone();
                // Resolve the function expression to get its type
                let func_type = self.resolve_expr(func_expr_id)?;
                let mut constructor_table_idx: Option<TableIndex> = None;
                let mut callee_is_nullable = false;
                let func_idx = match func_type {
                    ValueType::Function(Some(idx)) => idx,
                    ValueType::Table(Some(table_idx)) => {
                        if let Some(fi) = self.table(table_idx).call_func {
                            fi
                        } else if let Some(fi) = self.resolve_constructor_func(table_idx) {
                            // @constructor: use the named method for arg checking
                            constructor_table_idx = Some(table_idx);
                            fi
                        } else {
                            return None;
                        }
                    }
                    ValueType::Union(ref types) => {
                        // Extract function from a nullable union (e.g. nil | function)
                        let func_from_union = types.iter().find_map(|t| match t {
                            ValueType::Function(Some(idx)) => Some(*idx),
                            _ => None,
                        });
                        let has_nil = types.iter().any(|t| *t == ValueType::Nil);
                        let has_any_func = func_from_union.is_some() || types.iter().any(|t| matches!(t, ValueType::Function(None)));
                        match func_from_union {
                            Some(idx) => {
                                if has_nil {
                                    callee_is_nullable = true;
                                }
                                idx
                            }
                            None => {
                                // Function(None) in union — can't resolve the call, but emit nil diagnostic
                                if has_nil && has_any_func {
                                    // Emit diagnostic now since we'll return None below
                                    let mut suppressed = self.and_guarded_call_exprs.contains(&func_expr_id);
                                    if !suppressed {
                                        if let Some(scope_idx) = self.scope_at_offset(call_range.0) {
                                            if let Some(sym_idx) = self.ir.find_root_symbol(func_expr_id) {
                                                if self.is_symbol_narrowed(sym_idx, scope_idx) {
                                                    suppressed = true;
                                                } else if let Some((_, chain)) = self.ir.extract_field_chain(func_expr_id) {
                                                    if self.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                                                        suppressed = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if !suppressed {
                                        let type_str = self.format_value_type_depth(&func_type, 0);
                                        crate::diagnostics::need_check_nil::check_call(
                                            &mut self.diagnostics,
                                            &type_str,
                                            call_range.0 as usize, call_range.1 as usize,
                                        );
                                    }
                                }
                                return None;
                            }
                        }
                    }
                    _ => return None,
                };

                // Emit need-check-nil for calling a possibly-nil value
                if callee_is_nullable {
                    let mut suppressed = self.and_guarded_call_exprs.contains(&func_expr_id);
                    if !suppressed {
                        if let Some(scope_idx) = self.scope_at_offset(call_range.0) {
                            if let Some(sym_idx) = self.ir.find_root_symbol(func_expr_id) {
                                if self.is_symbol_narrowed(sym_idx, scope_idx) {
                                    suppressed = true;
                                } else if let Some((_, chain)) = self.ir.extract_field_chain(func_expr_id) {
                                    if self.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                                        suppressed = true;
                                    }
                                }
                            }
                        }
                    }
                    if !suppressed {
                        let type_str = self.format_value_type_depth(&func_type, 0);
                        crate::diagnostics::need_check_nil::check_call(
                            &mut self.diagnostics,
                            &type_str,
                            call_range.0 as usize, call_range.1 as usize,
                        );
                    }
                }

                // setmetatable / getmetatable: metatable type inference
                if *ret_index == 0 {
                    if let Some(smt_idx) = self.ir.ext.setmetatable_func_idx {
                        if func_idx == smt_idx {
                            return self.resolve_setmetatable(args);
                        }
                    }
                    if let Some(gmt_idx) = self.ir.ext.getmetatable_func_idx {
                        if func_idx == gmt_idx {
                            return self.resolve_getmetatable(args);
                        }
                    }
                }

                // Extract scalar fields without cloning the full Function struct
                let deprecated = self.func(func_idx).deprecated;
                let nodiscard = self.func(func_idx).nodiscard;
                let is_vararg = self.func(func_idx).is_vararg;
                let func_scope = self.func(func_idx).scope;
                let has_generics = !self.func(func_idx).generics.is_empty();
                let has_overloads = !self.func(func_idx).overloads.is_empty();
                let returns_self = self.func(func_idx).returns_self;
                // Clone only the Vecs we need unconditionally
                let func_args = self.func(func_idx).args.clone();
                // Defer conditional clones
                let overloads = if has_overloads { self.func(func_idx).overloads.clone() } else { Vec::new() };
                let generics = if has_generics { self.func(func_idx).generics.clone() } else { Vec::new() };
                let defclass = if has_generics { self.func(func_idx).defclass.clone() } else { None };
                let return_annotations = if has_generics { self.func(func_idx).return_annotations.clone() } else { Vec::new() };
                let param_annotations = self.func(func_idx).param_annotations.clone();

                // Emit @deprecated diagnostic
                if deprecated {
                    let name = self.function_name(func_idx).unwrap_or_else(|| "?".to_string());
                    crate::diagnostics::deprecated::check(
                        &mut self.diagnostics,
                        &name, call_range.0 as usize, call_range.1 as usize,
                    );
                }

                // Emit @nodiscard diagnostic
                if nodiscard && discarded {
                    let name = self.function_name(func_idx).unwrap_or_else(|| "?".to_string());
                    crate::diagnostics::discard_returns::check(
                        &mut self.diagnostics,
                        &name, call_range.0 as usize, call_range.1 as usize,
                    );
                }

                // For colon method calls, self is implicit — func_args includes it but args doesn't.
                // This covers three cases:
                // 1. Colon-defined methods: first param is auto-injected "self"
                // 2. Dot-defined methods called with colon (e.g. `obj:method()` calling
                //    `function T.__static.method(cls)`) — first param isn't "self" but
                //    the receiver is still implicitly passed
                // 3. Stored function fields called with colon (e.g. `self:_callback(row)`
                //    where _callback is `fun(query: AuctionQuery, row?: AuctionRow)`) —
                //    the receiver is implicitly passed as the first argument
                let has_self = func_args.first().is_some_and(|&sym| {
                    matches!(&self.sym(sym).id, SymbolIdentifier::Name(n) if n == "self")
                });
                let self_offset = if (constructor_table_idx.is_some() && has_self)
                    || (is_method_call && has_self)
                    || (is_method_call && !has_self && !func_args.is_empty()) { 1 } else { 0 };

                let param_optional = self.func(func_idx).param_optional.clone();

                // Emit redundant-parameter / missing-parameter diagnostics
                {
                    let actual_count = args.len();
                    let expected_count = func_args.len() - self_offset;

                    // If the last argument is varargs or a function call, it can expand
                    // to multiple values at runtime, so skip arg-count diagnostics.
                    let last_is_multi = args.last().is_some_and(|&last_id| {
                        matches!(self.ir.expr(last_id), Expr::VarArgs(..) | Expr::FunctionCall { .. })
                    });

                    // Redundant: more args than params, and function is not vararg
                    if actual_count > expected_count && !is_vararg && !last_is_multi {
                        // Check overloads: if any overload accepts this many args, skip
                        let overload_accepts = overloads.iter().any(|o| {
                            let o_self = if o.params.first().is_some_and(|p| p.name == "self") { 1 } else { 0 };
                            o.params.len() - o_self >= actual_count
                        });
                        if !overload_accepts {
                            // Highlight the first redundant argument
                            if let Some(&(start, end)) = arg_ranges.get(expected_count) {
                                crate::diagnostics::redundant_param::check(
                                    &mut self.diagnostics, expected_count, actual_count,
                                    start as usize, end as usize,
                                );
                            }
                        }
                    }

                    // Missing: fewer args than required params
                    if actual_count < expected_count && !last_is_multi {
                        // Count required params (non-optional, excluding trailing optional/unannotated)
                        let required_count = {
                            let mut count = expected_count;
                            // Walk backwards from the end, skipping optional and unannotated params
                            for i in (self_offset..func_args.len()).rev() {
                                let is_optional = param_optional.get(i).copied().unwrap_or(false);
                                let is_unannotated = param_annotations.get(i)
                                    .map_or(true, |a| matches!(a, crate::annotations::AnnotationType::Simple(s) if s.is_empty()));
                                if is_optional || is_unannotated {
                                    count -= 1;
                                } else {
                                    break;
                                }
                            }
                            count
                        };
                        if actual_count < required_count {
                            // Check overloads: if any overload is satisfied, skip
                            let overload_satisfied = overloads.iter().any(|o| {
                                actual_count >= o.params.len()
                            });
                            if !overload_satisfied {
                                // Find the name of the first missing required param (offset by self)
                                if let Some(&missing_sym) = func_args.get(actual_count + self_offset) {
                                    let param_name = match &self.sym(missing_sym).id {
                                        SymbolIdentifier::Name(n) => n.clone(),
                                        _ => "?".to_string(),
                                    };
                                    crate::diagnostics::missing_param::check(
                                        &mut self.diagnostics, &param_name,
                                        call_range.0 as usize, call_range.1 as usize,
                                    );
                                }
                            }
                        }
                    }
                }

                // Propagate callee's fun() param annotation types into inline function params
                for (i, arg_expr_id) in args.iter().enumerate() {
                    // Check if this argument is an inline function definition
                    let inline_func_idx = match self.ir.expr(*arg_expr_id) {
                        Expr::FunctionDef(idx) => *idx,
                        _ => continue,
                    };
                    if inline_func_idx >= EXT_BASE { continue; }
                    // Get the callee's param annotation for this position
                    let sig = match param_annotations.get(i + self_offset) {
                        Some(crate::annotations::AnnotationType::Simple(s)) if s.starts_with("fun(") => {
                            match crate::annotations::parse_overload(s) {
                                Some(sig) => sig,
                                None => continue,
                            }
                        }
                        Some(crate::annotations::AnnotationType::Fun(params, returns, is_vararg)) => {
                            crate::annotations::OverloadSig {
                                params: params.clone(),
                                returns: returns.clone(),
                                is_vararg: *is_vararg,
                                is_return_only: false,
                            }
                        }
                        _ => continue,
                    };
                    let inline_args = self.ir.functions[inline_func_idx].args.clone();
                    for (j, param_info) in sig.params.iter().enumerate() {
                        let Some(&inline_sym_idx) = inline_args.get(j) else { continue };
                        if inline_sym_idx >= EXT_BASE { continue; }
                        if self.ir.symbols[inline_sym_idx].versions.first()
                            .map_or(false, |v| v.resolved_type.is_some()) { continue; }
                        if let Some(vt) = self.resolve_annotation_type(&param_info.typ) {
                            let vt = if param_info.optional {
                                ValueType::union(vt, ValueType::Nil)
                            } else {
                                vt
                            };
                            self.ir.symbols[inline_sym_idx].versions[0].resolved_type = Some(vt);
                        }
                    }
                    // Propagate return types from fun() signature into inline function
                    if self.ir.functions[inline_func_idx].return_annotations.is_empty() {
                        if sig.returns.is_empty() {
                            // fun() with no return type — mark as explicitly void
                            self.ir.functions[inline_func_idx].explicit_void_return = true;
                        } else {
                            let mut return_vts = Vec::new();
                            for ret_annotation in &sig.returns {
                                if let Some(vt) = self.resolve_annotation_type(ret_annotation) {
                                    return_vts.push(vt);
                                }
                            }
                            if !return_vts.is_empty() {
                                self.ir.functions[inline_func_idx].return_annotations = return_vts;
                            }
                        }
                    }
                }

                // Build generic substitution map from call-site arg types
                let mut generic_subs: HashMap<String, ValueType> = HashMap::new();
                // Track which argument index inferred each generic (for diagnostics)
                let mut generic_arg_indices: HashMap<String, usize> = HashMap::new();
                // Track generics inferred from structural patterns (T[], table<K,V>)
                // — safe to use for type-mismatch substitution (vs. promotional patterns
                // like backtick/defclass where the arg type intentionally differs)
                let mut structural_generic_names: HashSet<String> = HashSet::new();
                if !generics.is_empty() {
                    let generic_names: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
                    for (i, arg_expr_id) in args.iter().enumerate() {
                        if let Some(arg_type) = self.resolve_expr(*arg_expr_id) {
                            // Check if this param's type is a TypeVariable
                            let param_type = if let Some(&param_sym_idx) = func_args.get(i + self_offset) {
                                self.sym(param_sym_idx).versions.last()
                                    .and_then(|ver| ver.resolved_type.clone())
                            } else {
                                None
                            };
                            if let Some(ValueType::TypeVariable(ref name)) = param_type {
                                if !generic_subs.contains_key(name) {
                                    // Skip empty-union args (result of and-narrowing nil types:
                                    // `x and func(x)` where x is nil → StripFalsy(nil) = Union([]))
                                    // so a later argument can provide a meaningful type for inference.
                                    if matches!(&arg_type, ValueType::Union(t) if t.is_empty()) {
                                        // fall through to structural inference below
                                    } else {
                                    // For backtick params (`T` or unions containing `T`), resolve the string literal to a class type
                                    let inferred = if param_annotations.get(i + self_offset).map_or(false, crate::annotations::annotation_contains_backtick) {
                                        if let Some(class_name) = self.ir.string_literals.get(arg_expr_id) {
                                            self.ir.classes.get(class_name).copied()
                                                .or_else(|| self.ir.ext.classes.get(class_name).copied())
                                                .map(|idx| ValueType::Table(Some(idx)))
                                                .unwrap_or_else(|| arg_type.clone())
                                        } else {
                                            arg_type.clone()
                                        }
                                    } else {
                                        arg_type.clone()
                                    };
                                    generic_subs.insert(name.clone(), inferred);
                                    generic_arg_indices.insert(name.clone(), i);
                                    }
                                }
                            } else if let Some(ValueType::Union(ref types)) = param_type {
                                // Optional params have type Union(TypeVariable("P"), Nil) —
                                // extract the TypeVariable to infer the generic, stripping nil.
                                // If the arg is literally nil, skip insertion so the constraint
                                // fallback applies (avoids false generic-constraint-mismatch).
                                if let Some(name) = types.iter().find_map(|t| match t {
                                    ValueType::TypeVariable(n) => Some(n),
                                    _ => None,
                                }) {
                                    if !generic_subs.contains_key(name) {
                                        let stripped = arg_type.strip_nil();
                                        let is_nil_like = matches!(&stripped, ValueType::Nil) || matches!(&stripped, ValueType::Union(t) if t.is_empty());
                                        if !is_nil_like {
                                            // Check if any member of the param annotation is a Backtick type —
                                            // if so, try to resolve a string literal argument as a class name.
                                            let inferred = if let Some(annotation) = param_annotations.get(i + self_offset) {
                                                if crate::annotations::annotation_contains_backtick(annotation) {
                                                    if let Some(class_name) = self.ir.string_literals.get(arg_expr_id) {
                                                        self.ir.classes.get(class_name).copied()
                                                            .or_else(|| self.ir.ext.classes.get(class_name).copied())
                                                            .map(|idx| ValueType::Table(Some(idx)))
                                                            .unwrap_or(stripped)
                                                    } else {
                                                        stripped
                                                    }
                                                } else {
                                                    stripped
                                                }
                                            } else {
                                                stripped
                                            };
                                            generic_subs.insert(name.clone(), inferred);
                                            generic_arg_indices.insert(name.clone(), i);
                                        }
                                    }
                                }
                            }
                            // Infer generics from structured param annotations (T[], table<K,V>)
                            let prev_len = generic_subs.len();
                            if let Some(annotation) = param_annotations.get(i + self_offset) {
                                self.infer_generics_from_annotation(annotation, &generic_names, &generics, &defclass, *arg_expr_id, &mut generic_subs);
                            }
                            // Record arg index for any newly inferred generics
                            if generic_subs.len() > prev_len {
                                for name in generic_subs.keys() {
                                    if !generic_arg_indices.contains_key(name) {
                                        structural_generic_names.insert(name.clone());
                                    }
                                    generic_arg_indices.entry(name.clone()).or_insert(i);
                                }
                            }
                        }
                    }
                    // Infer generics from receiver type_args for method calls.
                    // When a method has @param self ClassName<T> and the receiver was typed
                    // with @type ClassName<number>, infer T = number from the receiver's type_args.
                    if is_method_call {
                        if let Some(crate::annotations::AnnotationType::Parameterized(_, type_arg_anns)) = param_annotations.get(0) {
                            if let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(*func).clone() {
                                let receiver_type_args = self.get_expr_type_args(receiver_expr);
                                if receiver_type_args.len() == type_arg_anns.len() {
                                    for (pos, type_arg_ann) in type_arg_anns.iter().enumerate() {
                                        if let crate::annotations::AnnotationType::Simple(name) = type_arg_ann {
                                            if generic_names.contains(name) && !generic_subs.contains_key(name) {
                                                if let Some(concrete) = receiver_type_args.get(pos) {
                                                    generic_subs.insert(name.clone(), concrete.clone());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Validate generic constraints before fallback
                    for (name, constraint) in &generics {
                        if let (Some(constraint_type), Some(actual_type)) = (constraint, generic_subs.get(name)) {
                            // Skip validation when inferred type is itself a TypeVariable
                            // (e.g. passing a generic param to another generic function)
                            if matches!(actual_type, ValueType::TypeVariable(_)) { continue; }
                            // Skip validation for the @defclass generic — the argument is a
                            // plain table being promoted into the class type.
                            if defclass.as_deref() == Some(name.as_str()) { continue; }
                            // Strip nil before checking constraint — the nil case is already
                            // caught by need-check-nil, so we avoid duplicate warnings.
                            // Pure nil (strip_nil → empty union) still fails the constraint.
                            let actual_stripped = actual_type.strip_nil();
                            let is_pure_nil = matches!(&actual_stripped, ValueType::Union(t) if t.is_empty());
                            if is_pure_nil || (!actual_stripped.is_assignable_to(constraint_type) && !self.is_table_subtype(&actual_stripped, constraint_type)) {
                                if let Some(&arg_idx) = generic_arg_indices.get(name) {
                                    if let Some(&(start, end)) = arg_ranges.get(arg_idx) {
                                        let constraint_str = self.format_value_type_depth(constraint_type, 1);
                                        let actual_str = self.format_value_type_depth(actual_type, 1);
                                        crate::diagnostics::generic_constraint_mismatch::check(
                                            &mut self.diagnostics,
                                            name, &constraint_str, &actual_str,
                                            start as usize, end as usize,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    // Fallback: for any generic not inferred, use its constraint type
                    for (name, constraint) in &generics {
                        if !generic_subs.contains_key(name) {
                            if let Some(ct) = constraint {
                                generic_subs.insert(name.clone(), ct.clone());
                            }
                        }
                    }
                }

                // Find the matching overload (if any) — used for both diagnostics and return type.
                // Skip return-only overloads (`@overload return: ...`) which only affect narrowing.
                // Overload params may include an explicit `self` first param; subtract it
                // when comparing against call-site arg count.
                // Uses range-based matching (accounting for optional params) and type-based
                // discrimination to prefer overloads whose parameter types are compatible.
                let (matching_overload, overload_self_offset) = if !overloads.is_empty() {
                    let n_args = args.len();
                    let ovl_self_off = |o: &&ResolvedOverload| -> usize {
                        if o.params.first().is_some_and(|p| p.name == "self") { 1 } else { 0 }
                    };
                    // Range-match: min_required <= n_args <= max_params (accounting for optional)
                    let range_matched: Vec<&ResolvedOverload> = overloads.iter()
                        .filter(|o| !o.is_return_only)
                        .filter(|o| {
                            let off = ovl_self_off(o);
                            let non_self_params = &o.params[off..];
                            let required = non_self_params.iter().filter(|p| !p.optional).count();
                            let total = non_self_params.len();
                            n_args >= required && n_args <= total
                        })
                        .collect();
                    // When multiple overloads match by range, discriminate by
                    // string literal parameter values at the call site.
                    let string_filtered: Vec<&ResolvedOverload> = if range_matched.len() > 1 {
                        let filtered: Vec<&ResolvedOverload> = range_matched.iter().copied().filter(|o| {
                            let off = ovl_self_off(o);
                            o.params.iter().skip(off).take(n_args).enumerate().all(|(i, p)| {
                                match &p.typ {
                                    Some(ValueType::String(Some(expected))) => {
                                        args.get(i)
                                            .and_then(|id| self.ir.string_literals.get(id))
                                            .is_some_and(|actual| actual == expected)
                                    }
                                    Some(ValueType::Union(types)) => {
                                        let lits: Vec<&str> = types.iter().filter_map(|t| {
                                            if let ValueType::String(Some(s)) = t { Some(s.as_str()) } else { None }
                                        }).collect();
                                        if lits.is_empty() { return true; }
                                        args.get(i)
                                            .and_then(|id| self.ir.string_literals.get(id))
                                            .is_some_and(|actual| lits.contains(&actual.as_str()))
                                    }
                                    _ => true,
                                }
                            })
                        }).collect();
                        if filtered.is_empty() { range_matched } else { filtered }
                    } else {
                        range_matched
                    };
                    // When multiple overloads remain, prefer one with compatible arg types.
                    let found = if string_filtered.len() > 1 {
                        // Resolve arg types for type-based discrimination
                        let arg_types: Vec<Option<ValueType>> = args.iter()
                            .map(|id| self.resolve_expr(*id))
                            .collect();
                        // Score: count type mismatches per overload
                        let scored: Vec<(&ResolvedOverload, usize)> = string_filtered.iter().map(|o| {
                            let off = ovl_self_off(o);
                            let mismatches = arg_types.iter().enumerate().filter(|(i, arg_t)| {
                                if let Some(arg_t) = arg_t {
                                    if let Some(param) = o.params.get(i + off) {
                                        if let Some(param_t) = &param.typ {
                                            // Skip mismatch check for params with unresolved type variables
                                            if self.type_involves_type_variable(param_t) {
                                                return false;
                                            }
                                            // Optional params accept nil
                                            if param.optional && matches!(arg_t, ValueType::Nil) {
                                                return false;
                                            }
                                            !arg_t.is_assignable_to(param_t)
                                                && !self.is_table_subtype(arg_t, param_t)
                                        } else {
                                            false // no param type → no mismatch
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    false // unresolved arg → no mismatch
                                }
                            }).count();
                            (*o, mismatches)
                        }).collect();
                        let best = scored.iter().min_by_key(|(_, m)| *m);
                        // Only use the overload if it has zero mismatches;
                        // otherwise fall through to the primary function signature.
                        best.and_then(|(o, m)| if *m == 0 { Some(*o) } else { None })
                    } else if let Some(&only) = string_filtered.first() {
                        // Single candidate: verify type compatibility before committing
                        let off = ovl_self_off(&only);
                        let has_mismatch = args.iter().enumerate().any(|(i, arg_id)| {
                            if let Some(arg_t) = self.resolve_expr(*arg_id) {
                                if let Some(param) = only.params.get(i + off) {
                                    if let Some(param_t) = &param.typ {
                                        // Skip mismatch check for params with unresolved type variables
                                        // (e.g. T[] in generic functions) — can't compare until inferred
                                        if self.type_involves_type_variable(param_t) {
                                            return false;
                                        }
                                        // Optional params accept nil
                                        if param.optional && matches!(arg_t, ValueType::Nil) {
                                            return false;
                                        }
                                        !arg_t.is_assignable_to(param_t)
                                            && !self.is_table_subtype(&arg_t, param_t)
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        });
                        if has_mismatch { None } else { Some(only) }
                    } else {
                        None
                    };
                    if let Some(o) = found {
                        let off = if o.params.first().is_some_and(|p| p.name == "self") { 1 } else { 0 };
                        (Some(o), off)
                    } else {
                        (None, 0)
                    }
                } else {
                    (None, 0)
                };

                // When a generic overload is matched, re-infer generics from the
                // overload's param types. The initial inference used the primary
                // function's param layout which may map args to different positions
                // (e.g. 2-arg overload vs 3-arg primary for tinsert).
                if has_generics {
                    if let Some(overload) = matching_overload {
                        let generic_names: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
                        for (i, arg_expr_id) in args.iter().enumerate() {
                            let Some(arg_type) = self.resolve_expr(*arg_expr_id) else { continue };
                            let param_type = overload.params.get(i + overload_self_offset)
                                .and_then(|p| p.typ.as_ref());
                            let Some(param_type) = param_type else { continue };
                            // Direct TypeVariable: T → infer T = arg_type
                            if let ValueType::TypeVariable(name) = param_type {
                                if generic_names.contains(name) && !generic_subs.contains_key(name) {
                                    generic_subs.insert(name.clone(), arg_type.clone());
                                    generic_arg_indices.insert(name.clone(), i);
                                    structural_generic_names.insert(name.clone());
                                }
                            }
                            // Table with TypeVariable value_type: T[] → infer T from array elements
                            if let ValueType::Table(Some(idx)) = param_type {
                                let vt_name = self.table(*idx).value_type.clone();
                                if let Some(ValueType::TypeVariable(name)) = &vt_name {
                                    if generic_names.contains(name) && !generic_subs.contains_key(name) {
                                        if let Some(elem_type) = self.infer_array_element_type(*arg_expr_id) {
                                            generic_subs.insert(name.clone(), elem_type);
                                            generic_arg_indices.entry(name.clone()).or_insert(i);
                                            structural_generic_names.insert(name.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Emit type mismatch diagnostics
                for (i, arg_expr_id) in args.iter().enumerate() {
                    let Some(mut arg_type) = self.resolve_expr(*arg_expr_id) else { continue };
                    // Strip nil from argument type if the root symbol is narrowed at this call site
                    if let Some(&(start, _)) = arg_ranges.get(i) {
                        if let Some(sym_idx) = self.ir.find_root_symbol(*arg_expr_id) {
                            if let Some(scope_idx) = self.scope_at_offset(start) {
                                // Skip narrowing if the symbol was reassigned after the
                                // narrowed version (the reassignment's type takes precedence).
                                if !self.is_narrowing_overridden(sym_idx, scope_idx) {
                                    if let Some(narrowed_vt) = self.get_type_narrowing(sym_idx, scope_idx) {
                                        // Only replace if the resolved type isn't already more
                                        // specific (e.g. from an inner `and` type-filter version).
                                        if !arg_type.is_assignable_to(narrowed_vt) {
                                            arg_type = narrowed_vt.clone();
                                        }
                                    } else if let Some(guard_vt) = self.get_type_filtering(sym_idx, scope_idx) {
                                        arg_type = arg_type.filter_type_with(guard_vt, &|idx| self.table(idx).is_enum);
                                    }
                                    if let Some(stripped_vt) = self.get_type_stripping(sym_idx, scope_idx) {
                                        arg_type = arg_type.strip_type_with(stripped_vt, &|idx| self.table(idx).is_enum);
                                    }
                                }
                                if self.is_symbol_falsy_narrowed(sym_idx, scope_idx) {
                                    arg_type = arg_type.strip_falsy();
                                } else if self.is_symbol_narrowed(sym_idx, scope_idx) {
                                    arg_type = arg_type.strip_nil();
                                }
                                // Also check field-level narrowing (e.g. assert(self.field)
                                // or assert(self.a.b)). When a field chain is narrowed and
                                // its type is plain Nil, skip the mismatch check entirely.
                                if let Some((_, chain)) = self.ir.extract_field_chain(*arg_expr_id) {
                                    if self.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                                        arg_type = arg_type.strip_nil();
                                        if matches!(arg_type, ValueType::Nil) {
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Get expected parameter type (first version = the @param annotation, not a later @cast)
                    let expected_type = if let Some(overload) = matching_overload {
                        let param = overload.params.get(i + overload_self_offset);
                        // Skip type-mismatch for nil args to optional overload params
                        if param.is_some_and(|p| p.optional) && matches!(arg_type, ValueType::Nil) {
                            continue;
                        }
                        param.and_then(|p| p.typ.clone())
                    } else if let Some(&param_sym_idx) = func_args.get(i + self_offset) {
                        self.sym(param_sym_idx).versions.first()
                            .and_then(|ver| ver.resolved_type.clone())
                    } else {
                        None
                    };
                    let Some(expected_type) = expected_type else { continue };
                    // Apply generic substitutions from structural inference (T[], table<K,V>)
                    // to enable type checking (e.g. tinsert(string[], number) → mismatch).
                    // Only use structurally-inferred generics to avoid false positives
                    // from promotional patterns (backtick, defclass).
                    let expected_type = if !structural_generic_names.is_empty() {
                        let structural_subs: HashMap<String, ValueType> = generic_subs.iter()
                            .filter(|(k, _)| structural_generic_names.contains(k.as_str()))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if !structural_subs.is_empty() {
                            self.substitute_generics_deep(&expected_type, &structural_subs)
                        } else {
                            expected_type
                        }
                    } else {
                        expected_type
                    };
                    // Skip type-mismatch for generic type variables
                    if matches!(expected_type, ValueType::TypeVariable(_)) { continue; }
                    // Check assignability (structural + table subclass + function param count)
                    let structurally_matched = !arg_type.is_assignable_to(&expected_type)
                        && self.is_table_subtype(&arg_type, &expected_type);
                    if structurally_matched {
                        // Structural match succeeded — check for excess fields
                        if let Some(&(start, end)) = arg_ranges.get(i) {
                            self.check_excess_structural_fields(
                                &arg_type, &expected_type,
                                start as usize, end as usize,
                            );
                        }
                    }
                    if (!arg_type.is_assignable_to(&expected_type) && !structurally_matched)
                        || !self.is_function_compatible(&arg_type, &expected_type) {
                        // Check if this is a nil-union where the non-nil part is compatible.
                        // If so, emit need-check-nil instead of type-mismatch.
                        // Only applies to Union types containing nil (not bare Nil).
                        let is_nil_union_compatible = matches!(&arg_type, ValueType::Union(types) if types.iter().any(|t| matches!(t, ValueType::Nil))) && {
                            let stripped = arg_type.strip_nil();
                            stripped.is_assignable_to(&expected_type)
                                && self.is_function_compatible(&stripped, &expected_type)
                        };
                        let param_name: String = if let Some(overload) = matching_overload {
                            overload.params.get(i + overload_self_offset).map(|p| p.name.clone()).unwrap_or_else(|| "?".to_string())
                        } else if let Some(&param_sym_idx) = func_args.get(i + self_offset) {
                            if let SymbolIdentifier::Name(n) = &self.sym(param_sym_idx).id { n.clone() } else { "?".to_string() }
                        } else {
                            "?".to_string()
                        };
                        let expected_str = self.format_value_type_depth(&expected_type, 1);
                        let actual_str = self.format_value_type_depth(&arg_type, 1);
                        if let Some(&(start, end)) = arg_ranges.get(i) {
                            if is_nil_union_compatible {
                                crate::diagnostics::need_check_nil::check_param(
                                    &mut self.diagnostics, &param_name,
                                    &expected_str, &actual_str,
                                    start as usize, end as usize,
                                );
                            } else {
                                crate::diagnostics::type_mismatch::check(
                                    &mut self.diagnostics, &param_name,
                                    &expected_str, &actual_str,
                                    start as usize, end as usize,
                                );
                            }
                        }
                    }
                }

                // @constructor: return the class table type
                if let Some(ctor_table_idx) = constructor_table_idx {
                    return if *ret_index == 0 {
                        Some(ValueType::Table(Some(ctor_table_idx)))
                    } else {
                        None
                    };
                }

                // @return self: resolve receiver type for method calls
                if returns_self && *ret_index == 0 {
                    let builds_field_info = self.func(func_idx).builds_field.clone();
                    let built_name_param = self.func(func_idx).built_name;
                    let built_extends = self.func(func_idx).built_extends;
                    let receiver_type = if let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(*func).clone() {
                        self.resolve_expr(receiver_expr)
                    } else {
                        None
                    };
                    if let Some(rt) = receiver_type {
                        // If this method has @builds-field, create a new table with the added field
                        if let (Some((param_idx, field_vt, field_lateinit)), ValueType::Table(Some(recv_idx))) = (builds_field_info, &rt) {
                            let field_name = args.get(param_idx - 1) // 1-based to 0-based
                                .and_then(|&arg_expr| self.ir.string_literals.get(&arg_expr))
                                .cloned();
                            if let Some(name) = field_name {
                                let resolved_field_vt = if !generic_subs.is_empty() {
                                    self.substitute_generics_deep(&field_vt, &generic_subs)
                                } else {
                                    field_vt
                                };
                                let new_idx = self.clone_table_with_built_field(*recv_idx, &name, resolved_field_vt, field_lateinit);
                                return Some(ValueType::Table(Some(new_idx)));
                            }
                        }
                        // @built-name: set the built_table's class_name from a string literal argument
                        if let (Some(param_idx), ValueType::Table(Some(recv_idx))) = (built_name_param, &rt) {
                            let class_name = args.get(param_idx - 1)
                                .and_then(|&arg_expr| self.ir.string_literals.get(&arg_expr))
                                .cloned();
                            if let Some(name) = class_name {
                                let new_idx = self.clone_table_with_built_name(*recv_idx, &name, built_extends);
                                return Some(ValueType::Table(Some(new_idx)));
                            }
                        }
                        return Some(rt);
                    }
                }

                // @return built: return the accumulated built_table
                if self.func(func_idx).returns_built && *ret_index == 0 {
                    let returns_built_parent = self.func(func_idx).returns_built_parent.clone();
                    let receiver_type = if let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(*func).clone() {
                        self.resolve_expr(receiver_expr)
                    } else {
                        None
                    };
                    if let Some(ValueType::Table(Some(recv_idx))) = receiver_type {
                        if let Some(built_idx) = self.table(recv_idx).built_table {
                            // Optionally add parent class to the built table
                            if let Some(parent_name) = returns_built_parent {
                                if let Some(&parent_idx) = self.ir.classes.get(&parent_name)
                                    .or_else(|| self.ir.ext.classes.get(&parent_name))
                                {
                                    if !self.table(built_idx).parent_classes.contains(&parent_idx) {
                                        self.ir_mut_table(built_idx).parent_classes.push(parent_idx);
                                    }
                                }
                            }
                            return Some(ValueType::Table(Some(built_idx)));
                        }
                        // No built fields accumulated — return empty table
                        return Some(ValueType::Table(None));
                    }
                }

                // Pick the matching overload signature for return types
                let ret_index = *ret_index;
                // Check if any return-only overload implies nil at this return position.
                // If so, the primary return type should be unioned with nil (the function
                // can return nothing/nil via the return-only overload path).
                let return_overloads_may_nil = self.func(func_idx).return_overload_may_nil(ret_index);
                let return_type = matching_overload
                    .and_then(|o| o.returns.get(ret_index))
                    .map(|vt| {
                        if generic_subs.is_empty() {
                            vt.clone()
                        } else {
                            self.substitute_generics_deep(vt, &generic_subs)
                        }
                    });
                if let Some(rt) = return_type {
                    return Some(rt);
                }

                // Generic substitution for non-overload return types
                if !generic_subs.is_empty() {
                    if let Some(ret_vt) = return_annotations.get(ret_index) {
                        let substituted = self.substitute_generics_deep(ret_vt, &generic_subs);
                        if !matches!(substituted, ValueType::TypeVariable(_)) {
                            if return_overloads_may_nil && !substituted.contains_nil() && !matches!(substituted, ValueType::Any) {
                                return Some(ValueType::make_union(vec![substituted, ValueType::Nil]));
                            }
                            return Some(substituted);
                        }
                    }
                }

                // Non-overload: look up the return symbol.
                // For vararg returns (...T as last @return), clamp to the last slot.
                let effective_ret_index = self.func(func_idx).effective_return_index(ret_index);
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, effective_ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_scope)?;
                let ret_type = self.sym(ret_sym_idx).versions.first()?.resolved_type.clone();
                // If this function has generics and the return type is still a
                // TypeVariable, don't return it — keep unresolved so a later
                // fixpoint pass can substitute the concrete type.
                if !generics.is_empty() {
                    if let Some(ref vt) = ret_type {
                        if vt.contains_type_variable() {
                            return None;
                        }
                    }
                }
                // @built-name: if this function has @built-name, set the built_table's class_name
                // on the returned table from the specified string literal argument
                if let Some(built_name_idx) = self.func(func_idx).built_name {
                    if ret_index == 0 {
                        if let Some(ValueType::Table(Some(table_idx))) = &ret_type {
                            let class_name = args.get(built_name_idx - 1)
                                .and_then(|&arg_expr| self.ir.string_literals.get(&arg_expr))
                                .cloned();
                            if let Some(name) = class_name {
                                let extends = self.func(func_idx).built_extends;
                                let new_idx = self.clone_table_with_built_name(*table_idx, &name, extends);
                                return Some(ValueType::Table(Some(new_idx)));
                            }
                        }
                    }
                }
                // Propagate @built-name through wrapper functions: if this function returns
                // a class table whose __init method has @built-name, apply it using this
                // call's arguments.
                if self.func(func_idx).built_name.is_none() && ret_index == 0 {
                    if let Some(ValueType::Table(Some(table_idx))) = &ret_type {
                        let init_built_name = self.table(*table_idx).fields.get("__init")
                            .map(|f| f.expr)
                            .and_then(|expr_id| {
                                if let Expr::FunctionDef(fi) = self.expr(expr_id) {
                                    Some(*fi)
                                } else {
                                    None
                                }
                            })
                            .and_then(|fi| self.func(fi).built_name);
                        if let Some(param_idx) = init_built_name {
                            let class_name = args.get(param_idx - 1)
                                .and_then(|&arg_expr| self.ir.string_literals.get(&arg_expr))
                                .cloned();
                            if let Some(name) = class_name {
                                let new_idx = self.clone_table_with_built_name(*table_idx, &name, false);
                                return Some(ValueType::Table(Some(new_idx)));
                            }
                        }
                    }
                }
                // If return-only overloads imply nil at this position, union with nil
                if return_overloads_may_nil {
                    match ret_type {
                        Some(vt) if !vt.contains_nil() && !matches!(vt, ValueType::Any) => {
                            Some(ValueType::make_union(vec![vt, ValueType::Nil]))
                        }
                        other => other,
                    }
                } else {
                    ret_type
                }
            }

            Expr::FieldAccess { table, field, field_range } => {
                let field_range = *field_range;
                let table_type = self.resolve_expr(*table)?;
                // Field access on any yields any
                if matches!(table_type, ValueType::Any) { return Some(ValueType::Any); }
                let table_indices: Vec<TableIndex> = match &table_type {
                    ValueType::Table(Some(idx)) => vec![*idx],
                    ValueType::Intersection(types) => types.iter().filter_map(|t| match t {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }).collect(),
                    ValueType::Union(types) => types.iter().flat_map(|t| match t {
                        ValueType::Table(Some(idx)) => vec![*idx],
                        ValueType::Intersection(itypes) => itypes.iter().filter_map(|it| match it {
                            ValueType::Table(Some(idx)) => Some(*idx),
                            _ => None,
                        }).collect(),
                        _ => vec![],
                    }).collect(),
                    _ => return None,
                };
                if table_indices.is_empty() { return None; }

                // Try each table in the union for the field, collecting types
                // Prefer @type annotation when available, else use expr + extra_exprs
                let mut field_types: Vec<ValueType> = Vec::new();
                let mut field_exists = false;
                for &idx in &table_indices {
                    if let Some(fi) = self.ir.get_field(idx, field) {
                        field_exists = true;
                        if let Some(ref ann_vt) = fi.annotation {
                            if !field_types.contains(ann_vt) {
                                field_types.push(ann_vt.clone());
                            }
                        } else {
                            let primary = fi.expr;
                            let extras: Vec<ExprId> = fi.extra_exprs.clone();
                            // If there are reassignments and the initial value is nil,
                            // skip the nil — it's just a placeholder initializer.
                            let skip_primary = !extras.is_empty()
                                && matches!(self.resolve_expr(primary), Some(ValueType::Nil));
                            let all_exprs: Vec<ExprId> = if skip_primary {
                                extras
                            } else {
                                std::iter::once(primary).chain(extras).collect()
                            };
                            for expr_id in all_exprs {
                                if let Some(vt) = self.resolve_expr(expr_id) {
                                    if !field_types.contains(&vt) {
                                        field_types.push(vt);
                                    }
                                }
                            }
                        }
                    }
                }
                if !field_types.is_empty() {
                    return Some(ValueType::make_union(field_types));
                }
                // Field exists but type couldn't be resolved — don't emit undefined-field
                if field_exists {
                    return None;
                }

                // Field not found — check parent classes, then undefined-field diagnostic
                let first_idx = table_indices[0];
                if self.table(first_idx).class_name.is_some() {
                    // Resolve field from parent classes
                    let mut parent_field_types: Vec<ValueType> = Vec::new();
                    for &idx in &table_indices {
                        let parents = self.table(idx).parent_classes.clone();
                        for &parent_idx in &parents {
                            if let Some(fi) = self.ir.get_field(parent_idx, field) {
                                if let Some(ref ann_vt) = fi.annotation {
                                    if !parent_field_types.contains(ann_vt) {
                                        parent_field_types.push(ann_vt.clone());
                                    }
                                } else {
                                    let expr = fi.expr;
                                    if let Some(vt) = self.resolve_expr(expr) {
                                        if !parent_field_types.contains(&vt) {
                                            parent_field_types.push(vt);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !parent_field_types.is_empty() {
                        return Some(ValueType::make_union(parent_field_types));
                    }
                    let found = table_indices.iter().any(|&idx| {
                        self.table(idx).parent_classes.iter().any(|&pi| self.ir.has_field(pi, field))
                    });
                    if !found {
                        if let Some((start, end)) = field_range {
                            self.deferred.undefined_field_checks.push(UndefinedFieldCheck {
                                table_expr: *table,
                                field: field.clone(),
                                start,
                                end,
                            });
                        }
                    }
                }
                None
            }
            Expr::VarArgs(ret_index, file_level) => {
                if *file_level {
                    // WoW passes (addonName: string, addonTable: table) at file scope
                    match ret_index {
                        0 => Some(ValueType::String(None)),
                        1 => {
                            if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                                Some(ValueType::Table(Some(addon_idx)))
                            } else {
                                let table_idx = self.ir.tables.len();
                                self.ir.tables.push(TableInfo::default());
                                Some(ValueType::Table(Some(table_idx)))
                            }
                        }
                        _ => Some(ValueType::Nil),
                    }
                } else {
                    // Inside a function: varargs are untyped (any)
                    None
                }
            }
            Expr::BracketIndex { table, key: _ } => {
                let table_type = self.resolve_expr(*table)?;
                // Bracket index on any yields any
                if matches!(table_type, ValueType::Any) { return Some(ValueType::Any); }
                match &table_type {
                    ValueType::Table(Some(idx)) => {
                        self.table(*idx).value_type.clone()
                    }
                    ValueType::Union(types) => {
                        let mut value_types: Vec<ValueType> = Vec::new();
                        for t in types {
                            if let ValueType::Table(Some(idx)) = t {
                                if let Some(vt) = &self.table(*idx).value_type {
                                    if !value_types.contains(vt) {
                                        value_types.push(vt.clone());
                                    }
                                }
                            }
                        }
                        if value_types.is_empty() { None }
                        else { Some(ValueType::make_union(value_types)) }
                    }
                    _ => None,
                }
            }
            Expr::ForInVar { iterator_call, var_index } => {
                let iter_call = *iterator_call;
                let var_idx = *var_index;

                // Primary: resolve the iterator call and extract the iterator function's returns.
                // For pairs(tbl), the call resolves to the first return which is the iterator function.
                if let Some(iter_type) = self.resolve_expr(iter_call) {
                    if let ValueType::Function(Some(func_idx)) = iter_type {
                        // Get return type at var_index from the iterator function
                        let effective_var_idx = self.func(func_idx).effective_return_index(var_idx);
                        let ret_vt = self.func(func_idx).return_annotations.get(effective_var_idx).cloned();
                        if let Some(ref vt) = ret_vt {
                            if !vt.contains_type_variable() {
                                return ret_vt;
                            }
                        }
                        // Try return symbol
                        let func_scope = self.func(func_idx).scope;
                        let ret_id = SymbolIdentifier::FunctionRet(func_idx, var_idx);
                        if let Some(ret_sym_idx) = self.get_symbol(&ret_id, func_scope) {
                            let ret_type = self.sym(ret_sym_idx).versions.first()
                                .and_then(|v| v.resolved_type.clone());
                            if let Some(ref vt) = ret_type {
                                if !vt.contains_type_variable() {
                                    return ret_type;
                                }
                            }
                        }
                    }
                }

                // Fallback: infer from the argument table's key_type/value_type.
                // This handles generic iterators (pairs/ipairs) where K/V aren't fully inferred.
                let iter_expr = self.expr(iter_call).clone();
                if let Expr::FunctionCall { args, .. } = &iter_expr {
                    if let Some(&first_arg) = args.first() {
                        if let Some(arg_type) = self.resolve_expr(first_arg) {
                            if let ValueType::Table(Some(table_idx)) = arg_type {
                                match var_idx {
                                    0 => return self.table(table_idx).key_type.clone(),
                                    1 => return self.table(table_idx).value_type.clone(),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                None
            }

            _ => None,
        }
    }

    pub(super) fn resolve_binary_op(&self, op: Operator, lhs_type: ValueType, rhs_type: ValueType) -> Option<ValueType> {
        // Check if either operand is a table — only then do we need the metamethod path
        let lhs_table = match &lhs_type { ValueType::Table(Some(idx)) => Some(*idx), _ => None };
        let rhs_table = match &rhs_type { ValueType::Table(Some(idx)) => Some(*idx), _ => None };
        let has_table_operand = lhs_table.is_some() || rhs_table.is_some();

        // Try standard resolution (takes ownership — no clone needed on the hot path)
        if !has_table_operand {
            return resolve_binary_op_standalone(op, lhs_type, rhs_type);
        }

        // Table operand present: try standard first (needs clone to preserve for metamethod fallback)
        if let Some(result) = resolve_binary_op_standalone(op, lhs_type, rhs_type) {
            return Some(result);
        }

        // Fall back to metamethod check
        let metamethod = match op {
            Operator::Add => "__add",
            Operator::Subtract => "__sub",
            Operator::Multiply => "__mul",
            Operator::Divide => "__div",
            Operator::Modulo => "__mod",
            Operator::Hat => "__pow",
            Operator::Concatenate => "__concat",
            _ => return None,
        };
        // Check lhs metatable first, then rhs (Lua semantics)
        let table_idx = lhs_table.or(rhs_table)?;
        self.resolve_metamethod_return(table_idx, metamethod)
    }

    /// Resolve a unary metamethod (__unm or __len) on a table operand.
    fn resolve_unary_metamethod(&self, op: Operator, operand_type: &ValueType) -> Option<ValueType> {
        let metamethod = match op {
            Operator::Subtract => "__unm",
            Operator::ArrayLength => "__len",
            _ => return None,
        };
        let table_idx = match operand_type {
            ValueType::Table(Some(idx)) => *idx,
            _ => return None,
        };
        self.resolve_metamethod_return(table_idx, metamethod)
    }

    /// Look up a metamethod on a table's metatable (or the table itself for @class
    /// tables that define metamethods directly) and resolve its return type.
    fn resolve_metamethod_return(&self, table_idx: TableIndex, metamethod: &str) -> Option<ValueType> {
        // Check: 1) the metatable set via setmetatable, 2) the table itself (for @class)
        let candidates = [
            self.table(table_idx).metatable,
            Some(table_idx),
        ];
        for candidate in candidates.into_iter().flatten() {
            if let Some(fi) = self.ir.get_field_direct(candidate, metamethod) {
                if let Some(ref ann) = fi.annotation {
                    if let ValueType::Function(Some(func_idx)) = ann {
                        return self.func(*func_idx).return_annotations.first().cloned();
                    }
                }
                if let Expr::FunctionDef(func_idx) = self.expr(fi.expr) {
                    return self.func(*func_idx).return_annotations.first().cloned();
                }
            }
        }
        None
    }

    /// Get the type_args for an expression, used to infer generics from parameterized receivers.
    /// Returns type args from SymbolVersion for direct variable references, or resolves them
    /// from FieldInfo.annotation_type_raw for field access chains.
    fn get_expr_type_args(&mut self, expr_id: ExprId) -> Vec<ValueType> {
        // Clone expression data to avoid borrow conflicts with resolve_expr
        let expr = self.expr(expr_id).clone();
        match expr {
            // Direct variable reference: check the symbol version's type_args
            Expr::SymbolRef(sym_idx, ver) => {
                let sym = self.sym(sym_idx);
                if let Some(version) = sym.versions.get(ver) {
                    if !version.type_args.is_empty() {
                        return version.type_args.clone();
                    }
                }
                Vec::new()
            }
            // Field access: check the field's annotation_type_raw for parameterized types
            Expr::FieldAccess { table, field, .. } => {
                if let Some(ValueType::Table(Some(table_idx))) = self.resolve_expr(table) {
                    if let Some(fi) = self.table(table_idx).fields.get(&field) {
                        if let Some(crate::annotations::AnnotationType::Parameterized(_, ref type_arg_anns)) = fi.annotation_type_raw {
                            return type_arg_anns.iter()
                                .filter_map(|ta| crate::annotations::annotation_type_to_value_type(ta))
                                .collect();
                        }
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Deep generic substitution: recurses into Function and Table types,
    /// creating new IR entries with substituted type variables.
    pub(super) fn substitute_generics_deep(&mut self, vt: &ValueType, subs: &HashMap<String, ValueType>) -> ValueType {
        match vt {
            ValueType::TypeVariable(name) => {
                subs.get(name).cloned().unwrap_or_else(|| vt.clone())
            }
            ValueType::Union(types) => {
                let subst: Vec<_> = types.iter()
                    .map(|t| self.substitute_generics_deep(t, subs))
                    // Drop unresolved type variables — these are generics that couldn't
                    // be inferred from the call site (e.g. Tp when no template arg given).
                    .filter(|t| !matches!(t, ValueType::TypeVariable(_)))
                    .collect();
                ValueType::make_union(subst)
            }
            ValueType::Intersection(types) => {
                let subst: Vec<_> = types.iter()
                    .map(|t| self.substitute_generics_deep(t, subs))
                    .filter(|t| !matches!(t, ValueType::TypeVariable(_)))
                    .collect();
                match subst.len() {
                    0 => ValueType::Table(None),
                    1 => subst.into_iter().next().unwrap(),
                    _ => ValueType::Intersection(subst),
                }
            }
            ValueType::Function(Some(func_idx)) => {
                let func = self.func(*func_idx);
                // Check if any param or return types contain type variables
                let has_tv = func.args.iter().any(|&sym_idx| {
                    self.sym(sym_idx).versions.iter()
                        .any(|v| v.resolved_type.as_ref().map_or(false, |t| t.contains_type_variable()))
                }) || func.return_annotations.iter().any(|vt| vt.contains_type_variable());
                if !has_tv {
                    return vt.clone();
                }
                // Clone the function with substituted types
                let dummy_node = func.def_node;
                let is_vararg = func.is_vararg;
                let has_vararg_return_clone = func.has_vararg_return;
                let param_optional = func.param_optional.clone();
                let param_annotations = func.param_annotations.clone();
                let return_annotations = func.return_annotations.clone();
                let explicit_void_return = func.explicit_void_return;
                let arg_infos: Vec<(SymbolIdentifier, Option<ValueType>)> = func.args.iter().map(|&sym_idx| {
                    let sym = self.sym(sym_idx);
                    let resolved = sym.versions.first().and_then(|v| v.resolved_type.clone());
                    (sym.id.clone(), resolved)
                }).collect();

                let func_scope = self.ir.insert_scope(None);
                let mut new_args = Vec::new();
                for (id, resolved) in &arg_infos {
                    let substituted = resolved.as_ref().map(|t| self.substitute_generics_deep(t, subs));
                    let sym_idx = self.ir.symbols.len();
                    let order = self.ir.next_order();
                    self.ir.symbols.push(Symbol {
                        id: id.clone(),
                        scope_idx: func_scope,
                        versions: vec![SymbolVersion {
                            def_node: dummy_node,
                            type_source: None,
                            resolved_type: substituted,
                            type_args: Vec::new(),
                            created_in_scope: func_scope,
                            creation_order: order,
                        }],
                    });
                    new_args.push(sym_idx);
                }

                let new_func_idx = self.ir.functions.len();
                let subst_return_annotations: Vec<ValueType> = return_annotations.iter()
                    .map(|t| self.substitute_generics_deep(t, subs))
                    .collect();
                let mut new_rets = Vec::new();
                for (i, ret_vt) in subst_return_annotations.iter().enumerate() {
                    let sym_idx = self.ir.symbols.len();
                    let order = self.ir.next_order();
                    self.ir.symbols.push(Symbol {
                        id: SymbolIdentifier::FunctionRet(new_func_idx, i),
                        scope_idx: func_scope,
                        versions: vec![SymbolVersion {
                            def_node: dummy_node,
                            type_source: None,
                            resolved_type: Some(ret_vt.clone()),
                            type_args: Vec::new(),
                            created_in_scope: func_scope,
                            creation_order: order,
                        }],
                    });
                    new_rets.push(sym_idx);
                }

                self.ir.functions.push(Function {
                    def_node: dummy_node,
                    scope: func_scope,
                    args: new_args,
                    rets: new_rets,
                    return_annotations: subst_return_annotations,
                    overloads: Vec::new(),
                    doc: None,
                    deprecated: false,
                    nodiscard: false,
                    generics: Vec::new(),
                    generic_constraints_raw: Vec::new(),
                    param_annotations,
                    param_descriptions: Vec::new(),
                    defclass: None,
                    defclass_parent: None,
                    is_vararg,
                    vararg_annotation: None,
                    vararg_description: None,
                    param_optional,
                    returns_self: false,
                    explicit_void_return,
                    constructor: false,
                    builds_field: None,
                    built_name: None,
                    built_extends: false,
                    returns_built: false,
                    returns_built_parent: None,
                    type_narrows: None,
                    type_narrows_class: None,
                    has_vararg_return: has_vararg_return_clone,
                });
                ValueType::Function(Some(new_func_idx))
            }
            ValueType::Table(Some(table_idx)) => {
                let table = self.table(*table_idx);
                let has_tv = table.value_type.as_ref().map_or(false, |t| t.contains_type_variable())
                    || table.key_type.as_ref().map_or(false, |t| t.contains_type_variable())
                    || table.fields.values().any(|fi| fi.annotation.as_ref().map_or(false, |t| t.contains_type_variable()));
                if !has_tv {
                    return vt.clone();
                }
                // Clone all table data before mutating self
                let old_key = table.key_type.clone();
                let old_val = table.value_type.clone();
                let class_name = table.class_name.clone();
                let class_type_params = table.class_type_params.clone();
                let parent_classes = table.parent_classes.clone();
                let array_fields = table.array_fields.clone();
                let accessors = table.accessors.clone();
                let call_func = table.call_func;
                let metatable_index = table.metatable_index;
                let old_fields: Vec<(String, crate::types::FieldInfo)> = table.fields.iter().map(|(name, fi)| {
                    (name.clone(), crate::types::FieldInfo {
                        expr: fi.expr,
                        extra_exprs: fi.extra_exprs.clone(),
                        visibility: fi.visibility,
                        annotation: fi.annotation.clone(),
                        annotation_text: fi.annotation_text.clone(),
                        annotation_type_raw: fi.annotation_type_raw.clone(),
                        lateinit: fi.lateinit,
                        def_range: fi.def_range,
                    })
                }).collect();

                let new_key = old_key.as_ref().map(|t| self.substitute_generics_deep(t, subs));
                let new_val = old_val.as_ref().map(|t| self.substitute_generics_deep(t, subs));
                let fields: HashMap<String, crate::types::FieldInfo> = old_fields.into_iter().map(|(name, fi)| {
                    let new_ann = fi.annotation.as_ref().map(|t| self.substitute_generics_deep(t, subs));
                    (name, crate::types::FieldInfo {
                        expr: fi.expr,
                        extra_exprs: fi.extra_exprs,
                        visibility: fi.visibility,
                        annotation: new_ann,
                        annotation_text: fi.annotation_text,
                        annotation_type_raw: fi.annotation_type_raw,
                        lateinit: fi.lateinit,
                        def_range: fi.def_range,
                    })
                }).collect();
                let new_table_idx = self.ir.tables.len();
                self.ir.tables.push(TableInfo {
                    fields, class_name, class_type_params, parent_classes,
                    array_fields, key_type: new_key, value_type: new_val,
                    accessors, call_func, metatable_index, ..Default::default()
                });
                ValueType::Table(Some(new_table_idx))
            }
            other => other.clone(),
        }
    }

    /// Mutable access to a local table (must be < EXT_BASE).
    fn ir_mut_table(&mut self, idx: TableIndex) -> &mut TableInfo {
        &mut self.ir.tables[idx]
    }

    /// Resolve a `setmetatable(tbl, mt)` call. Mutates the table in-place (matching
    /// Lua semantics) by setting `metatable_index`, `metatable`, and `call_func`.
    fn resolve_setmetatable(&mut self, args: &[ExprId]) -> Option<ValueType> {
        let tbl_expr = args.first()?;
        let tbl_type = self.resolve_expr(*tbl_expr);

        // If the first argument isn't a resolved table, return None so fixpoint retries
        let tbl_idx = match tbl_type {
            Some(ValueType::Table(Some(idx))) => idx,
            _ => return None,
        };

        // Can only mutate local tables (not external)
        if tbl_idx >= EXT_BASE {
            return Some(ValueType::Table(Some(tbl_idx)));
        }

        // No metatable arg → return the table as-is
        let mt_expr = match args.get(1) {
            Some(e) => *e,
            None => return Some(ValueType::Table(Some(tbl_idx))),
        };

        let mt_type = self.resolve_expr(mt_expr);
        let mt_idx = match mt_type {
            Some(ValueType::Table(Some(idx))) => idx,
            _ => {
                // Metatable not resolved yet — return the table without changes;
                // fixpoint will retry and may resolve it later
                return Some(ValueType::Table(Some(tbl_idx)));
            }
        };

        // Store the raw metatable (for getmetatable())
        self.ir.tables[tbl_idx].metatable = Some(mt_idx);

        // Resolve __index on the metatable once; use the result for both
        // metatable_index and class_name propagation fallbacks below.
        let index_resolved = self.resolve_metatable_index_expr(mt_idx);

        // Case 1: __index resolved to a table directly (table ref or function with @return)
        if let Some(index_idx) = index_resolved.as_ref().and_then(|vt| self.extract_table_from_type(vt)) {
            self.ir.tables[tbl_idx].metatable_index = Some(index_idx);
            // Propagate class_name from the __index target to the result table.
            // This makes `setmetatable({}, { __index = MyClass })` type as `MyClass`
            // instead of anonymous `table`, enabling correct return-type matching.
            if self.ir.tables[tbl_idx].class_name.is_none() {
                if let Some(name) = self.table(index_idx).class_name.clone() {
                    self.ir.tables[tbl_idx].class_name = Some(name);
                }
            }
        }

        // Case 2: propagate class_name from the metatable itself.
        // Handles `---@class Foo \n local MT = { __index = function(...) ... end }`
        // where the class annotation is on the metatable, not an __index target.
        if self.ir.tables[tbl_idx].class_name.is_none() {
            if let Some(name) = self.table(mt_idx).class_name.clone() {
                self.ir.tables[tbl_idx].class_name = Some(name);
            }
        }

        // Case 3: when __index is a function without @return annotations,
        // scan its return expressions for bracket/field accesses on class-typed
        // tables. This handles the common pattern:
        //   __index = function(self, key) if METHODS[key] then return METHODS[key] end ... end
        // where METHODS has a @class annotation.
        if self.ir.tables[tbl_idx].class_name.is_none() {
            if let Some(class_idx) = self.find_class_in_index_function(&index_resolved) {
                let name = self.table(class_idx).class_name.clone();
                self.ir.tables[tbl_idx].class_name = name;
                // Set metatable_index to the delegate methods table so field lookups
                // find class methods. This is an approximation — the real __index is a
                // function, but pointing metatable_index at the table it delegates to
                // gives correct field resolution behavior.
                if self.ir.tables[tbl_idx].metatable_index.is_none() {
                    self.ir.tables[tbl_idx].metatable_index = Some(class_idx);
                }
            }
        }

        // Resolve __call on the metatable and set call_func on the table
        if self.ir.tables[tbl_idx].call_func.is_none() {
            if let Some(func_idx) = self.resolve_metatable_call_func(mt_idx) {
                self.ir.tables[tbl_idx].call_func = Some(func_idx);
            }
        }

        Some(ValueType::Table(Some(tbl_idx)))
    }

    /// Resolve `getmetatable(obj)`: return the raw metatable stored on the table.
    fn resolve_getmetatable(&mut self, args: &[ExprId]) -> Option<ValueType> {
        let tbl_expr = args.first()?;
        let tbl_type = self.resolve_expr(*tbl_expr)?;
        let tbl_idx = match tbl_type {
            ValueType::Table(Some(idx)) => idx,
            _ => return None,
        };
        match self.table(tbl_idx).metatable {
            Some(mt_idx) => Some(ValueType::Table(Some(mt_idx))),
            None => Some(ValueType::Table(None)), // no metatable → generic table
        }
    }

    /// Resolve the `__index` field on a metatable to its ValueType.
    /// Uses `get_field` (not `get_field_direct`) because chained metatables may have
    /// their `__index` field deferred — walking the metatable_index chain finds the
    /// inherited `__index` when the direct field hasn't been resolved yet.
    fn resolve_metatable_index_expr(&mut self, mt_idx: TableIndex) -> Option<ValueType> {
        let fi = self.ir.get_field(mt_idx, "__index")?;
        let expr = fi.expr;
        self.resolve_expr(expr)
    }

    /// Resolve the `__call` field on a metatable to a FunctionIndex.
    fn resolve_metatable_call_func(&mut self, mt_idx: TableIndex) -> Option<FunctionIndex> {
        let fi = self.ir.get_field(mt_idx, "__call")?;
        let expr = fi.expr;
        let resolved = self.resolve_expr(expr)?;
        match resolved {
            ValueType::Function(Some(idx)) => Some(idx),
            ValueType::Union(ref types) => {
                types.iter().find_map(|t| match t {
                    ValueType::Function(Some(idx)) => Some(*idx),
                    _ => None,
                })
            }
            _ => None,
        }
    }

    /// When `__index` is a function without `@return` annotations, scan the function's
    /// return expressions for bracket/field accesses on class-typed tables. Returns the
    /// first class table found. This handles patterns like:
    ///   `__index = function(self, key) if METHODS[key] then return METHODS[key] end ... end`
    ///
    /// Takes the already-resolved `__index` ValueType to avoid re-resolving the field.
    fn find_class_in_index_function(&mut self, index_resolved: &Option<ValueType>) -> Option<TableIndex> {
        let func_idx = match index_resolved {
            Some(ValueType::Function(Some(idx))) => *idx,
            _ => return None,
        };
        // Only use this fallback when the function has no return annotations
        // (functions with @return are already handled by extract_table_from_type)
        if !self.func(func_idx).return_annotations.is_empty() {
            return None;
        }
        let rets: Vec<SymbolIndex> = self.func(func_idx).rets.clone();
        for ret_sym_idx in rets {
            let type_source = self.ir.symbols.get(ret_sym_idx)
                .and_then(|s| s.versions.last())
                .and_then(|v| v.type_source);
            let expr_id = match type_source {
                Some(id) => id,
                None => continue,
            };
            let expr = match self.ir.exprs.get(expr_id) {
                Some(e) => e.clone(),
                None => continue,
            };
            let base_expr = match &expr {
                Expr::BracketIndex { table, .. } => Some(*table),
                Expr::FieldAccess { table, .. } => Some(*table),
                _ => None,
            };
            if let Some(base) = base_expr {
                if let Some(base_type) = self.resolve_expr(base) {
                    if let ValueType::Table(Some(idx)) = base_type {
                        if self.table(idx).class_name.is_some() {
                            return Some(idx);
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract a TableIndex from a ValueType, handling Table, Union, and Function
    /// (for function-valued __index, extracts the first return type if it's a table).
    fn extract_table_from_type(&self, vt: &ValueType) -> Option<TableIndex> {
        match vt {
            ValueType::Table(Some(idx)) => Some(*idx),
            ValueType::Union(types) => {
                types.iter().find_map(|t| match t {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                })
            }
            ValueType::Function(Some(func_idx)) => {
                // __index as function: check if return type is a table
                let ret = self.func(*func_idx).return_annotations.first()?;
                match ret {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Look up the @constructor method on a class table and return its FunctionIndex.
    /// Checks the table's own `constructors` set, then walks parent classes.
    fn resolve_constructor_func(&self, table_idx: TableIndex) -> Option<FunctionIndex> {
        // Find a constructor name: check own table, then walk parents
        let ctor_name = self.table(table_idx).constructors.iter().next().cloned()
            .or_else(|| {
                self.table(table_idx).parent_classes.clone().iter()
                    .find_map(|&p| self.table(p).constructors.iter().next().cloned())
            })?;
        // Resolve the constructor method to a function (get_field walks parents)
        let field = self.get_field(table_idx, &ctor_name)
            .or_else(|| self.table(table_idx).parent_classes.clone().iter()
                .find_map(|&p| self.get_field(p, &ctor_name)))?;
        if let Expr::FunctionDef(fi) = self.expr(field.expr) {
            Some(*fi)
        } else {
            None
        }
    }

    /// Safety limit: maximum number of tables that can be created during the fixpoint loop.
    /// Builder chains create 2 tables per step; this caps total IR table count to prevent OOM.
    const MAX_IR_TABLES: usize = 50_000;

    /// Clone a table, create/extend its built_table with a new field, and return the new table index.
    fn clone_table_with_built_field(&mut self, source_idx: TableIndex, field_name: &str, field_type: ValueType, lateinit: bool) -> TableIndex {
        if self.ir.tables.len() >= Self::MAX_IR_TABLES {
            if self.safety_limit_hit.is_none() {
                self.safety_limit_hit = Some(format!(
                    "builder chain exceeded table limit ({})", Self::MAX_IR_TABLES
                ));
            }
            return source_idx; // bail out: return source unchanged
        }
        let source = self.table(source_idx);
        let schema_fields = source.fields.clone();
        let class_name = source.class_name.clone();
        let class_type_params = source.class_type_params.clone();
        let parent_classes = source.parent_classes.clone();
        let accessors = source.accessors.clone();
        let call_func = source.call_func;
        let existing_built = source.built_table;
        let metatable_index = source.metatable_index;

        // Clone or create the built table's fields
        let mut built_fields = if let Some(bt_idx) = existing_built {
            self.table(bt_idx).fields.clone()
        } else {
            HashMap::new()
        };
        let (built_class_name, built_parent_classes) = if let Some(bt_idx) = existing_built {
            (self.table(bt_idx).class_name.clone(), self.table(bt_idx).parent_classes.clone())
        } else {
            (class_name.clone(), Vec::new())
        };

        // Add the new field, but don't overwrite @class overlay fields
        // (overlay fields have annotation_type_raw set from @field parsing)
        let has_overlay = built_fields.get(field_name)
            .map_or(false, |f| f.annotation_type_raw.is_some());
        if !has_overlay {
            let dummy_expr = self.ir.push_expr(Expr::Literal(field_type.clone()));
            built_fields.insert(field_name.to_string(), crate::types::FieldInfo {
                expr: dummy_expr,
                extra_exprs: Vec::new(),
                visibility: crate::annotations::default_visibility_for_name(field_name),
                annotation: Some(field_type),
                annotation_text: None,
                annotation_type_raw: None,
                lateinit,
                def_range: None,
            });
        }

        // Create new built table
        let new_built_idx = self.ir.tables.len();
        self.ir.tables.push(TableInfo {
            fields: built_fields, class_name: built_class_name.clone(),
            parent_classes: built_parent_classes, ..Default::default()
        });

        // Keep ir.classes pointing to the latest built table with this name
        if let Some(ref name) = built_class_name {
            if self.ir.classes.get(name).is_some() {
                self.ir.classes.insert(name.clone(), new_built_idx);
            }
        }

        // Create new schema table pointing to new built table
        let new_schema_idx = self.ir.tables.len();
        self.ir.tables.push(TableInfo {
            fields: schema_fields, class_name, class_type_params,
            parent_classes, accessors, call_func,
            built_table: Some(new_built_idx), metatable_index, ..Default::default()
        });

        new_schema_idx
    }

    /// Clone a table and set (or update) its built_table's class_name from `@built-name`.
    /// If no built_table exists yet, creates an empty one. Registers the name in `ir.classes`.
    fn clone_table_with_built_name(&mut self, source_idx: TableIndex, class_name: &str, extends: bool) -> TableIndex {
        if self.ir.tables.len() >= Self::MAX_IR_TABLES {
            if self.safety_limit_hit.is_none() {
                self.safety_limit_hit = Some(format!(
                    "builder chain exceeded table limit ({})", Self::MAX_IR_TABLES
                ));
            }
            return source_idx; // bail out: return source unchanged
        }
        let source = self.table(source_idx);
        let schema_fields = source.fields.clone();
        let schema_class_name = source.class_name.clone();
        let class_type_params = source.class_type_params.clone();
        let parent_classes = source.parent_classes.clone();
        let accessors = source.accessors.clone();
        let call_func = source.call_func;
        let existing_built = source.built_table;
        let metatable_index = source.metatable_index;

        // When extending, set the existing built type as the parent of the new one.
        // Also collect all ancestor parent_classes so single-level parent resolution
        // can find fields from any ancestor (since FieldAccess only walks one level).
        let (mut built_fields, built_parents) = if extends {
            let mut parents = Vec::new();
            if let Some(bt_idx) = existing_built {
                parents.push(bt_idx);
                // Flatten: collect all ancestors from the parent chain so the
                // single-level FieldAccess parent resolution can find them
                let mut frontier = self.table(bt_idx).parent_classes.clone();
                let mut visited = std::collections::HashSet::new();
                while let Some(p) = frontier.pop() {
                    if visited.insert(p) {
                        parents.push(p);
                        frontier.extend_from_slice(&self.table(p).parent_classes);
                    }
                }
            }
            (HashMap::new(), parents)
        } else {
            let fields = if let Some(bt_idx) = existing_built {
                self.table(bt_idx).fields.clone()
            } else {
                HashMap::new()
            };
            (fields, Vec::new())
        };

        // Preserve parent_classes from the previously-registered class entry (if any).
        // PreResolvedGlobals pass 3c sets up @built-extends parent relationships on ext tables,
        // but per-file resolution may re-create the built table without the receiver's built_table
        // being set (e.g. expression statements on inherited schema fields). In that case,
        // the ext entry's parent_classes should be carried forward.
        let mut final_parents = built_parents;
        if final_parents.is_empty() {
            if let Some(&old_idx) = self.ir.classes.get(class_name) {
                let old_parents = &self.table(old_idx).parent_classes;
                if !old_parents.is_empty() {
                    final_parents = old_parents.clone();
                }
            }
        }

        // Before creating the built table, check if there's a local @class overlay
        // with the same name (from Phase 0 prescan). Merge its @field annotations
        // into the built table so overlay types take precedence over builder types.
        // Note: on the FIRST call for a given class_name, ir.classes points to the
        // prescan @class table (index < EXT_BASE). On subsequent calls (from chained
        // clone_table_with_built_field), ir.classes points to a previous built table
        // which already carries the overlay fields forward via built_fields cloning —
        // so re-merging from the previous built table is harmless.
        let mut overlay_correlated = Vec::new();
        if let Some(&overlay_idx) = self.ir.classes.get(class_name) {
            if overlay_idx < EXT_BASE {
                let overlay_fields: Vec<(String, FieldInfo)> = self.ir.tables[overlay_idx].fields.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (fname, fi) in overlay_fields {
                    built_fields.insert(fname, fi);
                }
                overlay_correlated = self.ir.tables[overlay_idx].correlated_groups.clone();
            }
        }

        // Create new built table with the specified class_name
        let new_built_idx = self.ir.tables.len();
        self.ir.tables.push(TableInfo {
            fields: built_fields, class_name: Some(class_name.to_string()),
            parent_classes: final_parents, correlated_groups: overlay_correlated,
            ..Default::default()
        });

        // Register the class name so @param/@type annotations can reference it
        self.ir.classes.insert(class_name.to_string(), new_built_idx);

        // Create new schema table pointing to new built table
        let new_schema_idx = self.ir.tables.len();
        self.ir.tables.push(TableInfo {
            fields: schema_fields, class_name: schema_class_name,
            class_type_params, parent_classes, accessors, call_func,
            built_table: Some(new_built_idx), metatable_index, ..Default::default()
        });

        new_schema_idx
    }
}

/// Pure function for binary op type resolution (no `self` needed).
/// Called from both `Analysis::resolve_binary_op` and `AnalysisResult::resolve_expr_type_inner`.
pub(super) fn resolve_binary_op_standalone(op: Operator, lhs_type: ValueType, rhs_type: ValueType) -> Option<ValueType> {
    match op {
        Operator::Or => {
            match (&lhs_type, &rhs_type) {
                (ValueType::Any, _) => Some(ValueType::Any),
                (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => Some(rhs_type),
                (ValueType::Boolean(Some(true)), _) => Some(lhs_type),
                (ValueType::Boolean(None), ValueType::Boolean(_)) => Some(lhs_type),
                (ValueType::Boolean(None), _) => {
                    Some(ValueType::union(ValueType::Boolean(Some(true)), rhs_type.clone()))
                },
                (ValueType::Union(types), _) => {
                    let has_falsy = types.iter().any(|t| matches!(t, ValueType::Nil | ValueType::Boolean(Some(false))));
                    if has_falsy {
                        let mut remaining: Vec<ValueType> = types.iter()
                            .filter(|t| !matches!(t, ValueType::Nil | ValueType::Boolean(Some(false))))
                            .cloned().collect();
                        remaining.push(rhs_type.clone());
                        Some(ValueType::make_union(remaining))
                    } else {
                        Some(lhs_type)
                    }
                },
                (ValueType::Number | ValueType::String(_) | ValueType::Function(_) | ValueType::Table(_) | ValueType::Intersection(_) | ValueType::TypeVariable(_) | ValueType::Userdata | ValueType::Thread, _) => Some(lhs_type),
            }
        },
        Operator::And => {
            match (&lhs_type, &rhs_type) {
                (ValueType::Any, _) => Some(ValueType::make_union(vec![rhs_type.clone(), ValueType::Nil])),
                (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => Some(lhs_type),
                (ValueType::Union(types), _) => {
                    let falsy: Vec<ValueType> = types.iter()
                        .filter(|t| matches!(t, ValueType::Nil | ValueType::Boolean(Some(false))))
                        .cloned().collect();
                    if falsy.is_empty() {
                        Some(rhs_type)
                    } else {
                        let mut result = falsy;
                        result.push(rhs_type.clone());
                        Some(ValueType::make_union(result))
                    }
                },
                (ValueType::Boolean(Some(true)) | ValueType::Number | ValueType::String(_) | ValueType::Function(_) | ValueType::Table(_) | ValueType::Intersection(_) | ValueType::TypeVariable(_) | ValueType::Userdata | ValueType::Thread, _) => Some(rhs_type),
                (ValueType::Boolean(None), ValueType::Boolean(Some(true))) => Some(lhs_type),
                (_, ValueType::Boolean(Some(false)) | ValueType::Nil) => Some(rhs_type),
                (ValueType::Boolean(None), _) => {
                    Some(ValueType::union(ValueType::Boolean(Some(false)), rhs_type.clone()))
                },
            }
        },
        Operator::LessThan | Operator::GreaterThan | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals => Some(ValueType::Boolean(None)),
        Operator::NotEquals | Operator::Equals => Some(ValueType::Boolean(None)),
        Operator::Concatenate => {
            if lhs_type.can_concat_to_string() && rhs_type.can_concat_to_string() {
                Some(ValueType::String(None))
            } else {
                None
            }
        },
        Operator::Add | Operator::Subtract | Operator::Divide | Operator::Multiply | Operator::Modulo | Operator::Hat => {
            match (&lhs_type, &rhs_type) {
                (ValueType::Number, ValueType::Number) => Some(ValueType::Number),
                (ValueType::Any, _) | (_, ValueType::Any) => Some(ValueType::Number),
                (ValueType::Table(_), _) | (_, ValueType::Table(_)) => None,
                _ => None,
            }
        },
        _ => None,
    }
}

