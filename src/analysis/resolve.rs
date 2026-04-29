use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::types::*;
use super::Analysis;
use super::build_ir::OverloadCheck;

// ── Type Resolution (Phase 2) ──────────────────────────────────────────────────

impl<'a> Analysis<'a> {
    pub fn resolve_types(&mut self) {
        // Pre-resolve annotated return symbols so they're available before
        // the main resolution loop tries to resolve callers
        for func_idx_raw in 0..self.ir.functions.len() {
            let func = &self.ir.functions[func_idx_raw];
            if func.return_annotations.is_empty() {
                continue;
            }
            let scope = func.scope;
            for (i, vt) in func.return_annotations.clone().iter().enumerate() {
                let ret_id = SymbolIdentifier::FunctionRet(FunctionIndex(func_idx_raw), i);
                if let Some(ret_sym_idx) = self.get_symbol(&ret_id, scope)
                    && let Some(ver) = self.ir.symbols[ret_sym_idx.val()].versions.first_mut()
                        && ver.resolved_type.is_none() {
                            ver.resolved_type = Some(vt.clone());
                        }
            }
        }

        let mut pending: Vec<(SymbolIndex, usize)> = Vec::new();
        for (si, sym) in self.ir.symbols.iter().enumerate() {
            for (vi, ver) in sym.versions.iter().enumerate() {
                if ver.type_source.is_some() && ver.resolved_type.is_none() {
                    pending.push((SymbolIndex(si), vi));
                }
            }
        }

        // Collect call expressions not already backing a symbol's type_source
        let symbol_exprs: std::collections::HashSet<ExprId> = self.ir.symbols.iter()
            .flat_map(|s| s.versions.iter())
            .filter_map(|v| v.type_source)
            .collect();
        let mut pending_calls: Vec<ExprId> = self.ir.exprs.iter().enumerate()
            .filter(|(_, e)| matches!(e, Expr::FunctionCall { .. }))
            .map(|(i, _)| ExprId(i))
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
                    let expr_id = self.ir.symbols[si.val()].versions[vi].type_source.unwrap();
                    let is_branch_merge = matches!(self.expr(expr_id), Expr::BranchMerge(_));
                    if is_branch_merge {
                        // BranchMerge may produce a partial union when some branches
                        // haven't resolved yet. Clear the cache so we re-evaluate with
                        // any newly resolved branches from this iteration.
                        self.resolved_expr_cache.remove(&expr_id);
                    }
                    if let Some(resolved) = self.resolve_expr(expr_id) {
                        let prev = self.ir.symbols[si.val()].versions[vi].resolved_type.replace(resolved.clone());
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

                // Process deferred class-equality narrowings: resolve the RHS of
                // `x == EXPR` and, if EXPR is a class-typed value, narrow x to that class
                // and propagate through multi-return siblings.
                self.resolve_deferred_class_eq_narrowings(&mut pending);

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
                        if sym_idx.is_external() { continue; }
                        let current_type = self.ir.symbols[sym_idx.val()].versions.first()
                            .and_then(|v| v.resolved_type.clone());
                        // Re-resolve if unresolved
                        if current_type.is_none() {
                            if let Some(vt) = self.resolve_annotation_type(ann) {
                                self.ir.symbols[sym_idx.val()].versions[0].resolved_type = Some(vt);
                                // Store type args for parameterized annotations
                                if let crate::annotations::AnnotationType::Parameterized(_, type_arg_anns) = ann {
                                    let type_args: Vec<ValueType> = type_arg_anns.iter()
                                        .filter_map(|ta| self.resolve_annotation_type(ta))
                                        .collect();
                                    if !type_args.is_empty() {
                                        self.ir.symbols[sym_idx.val()].versions[0].type_args = type_args;
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
                        if let Some(ValueType::Table(Some(old_idx))) = &current_type
                            && let Some(class_name) = self.table(*old_idx).class_name.clone()
                                && let Some(&new_idx) = self.ir.classes.get(&class_name)
                                    && new_idx != *old_idx {
                                        self.ir.symbols[sym_idx.val()].versions[0].resolved_type =
                                            Some(ValueType::Table(Some(new_idx)));
                                        new_resolution = true;
                                    }
                    }
                }
                // Before giving up, try backward param-type inference
                // (sets resolved_type on unannotated local params based on how
                // they're used in the function body). Allowed to re-run on
                // each stall — once a param is inferred it's removed from the
                // candidate set, which guarantees termination while letting
                // newly-inferred types propagate as hints for dependent params
                // (e.g. caller's arg → callee's backward-inferred param type).
                if self.backward_param_types
                    && self.infer_backward_param_types() {
                        new_resolution = true;
                    }
                // Refine synthesized return-only overload slots whose source
                // expressions have now resolved. Runs alongside backward
                // inference so the main fixpoint gets a chance to populate
                // types before we try to read them back.
                if self.refine_synthesized_return_overloads() {
                    new_resolution = true;
                }
                if !new_resolution {
                    break;
                }
                // Clear expression cache so dependent expressions (e.g. field access
                // on re-resolved params) get re-evaluated in the next fixpoint iteration.
                // Builder chain call results are preserved via `builder_call_memo` so
                // re-resolution doesn't duplicate the built tables.
                self.resolved_expr_cache.clear();
                // Repopulate pending_calls and symbol-backed FunctionCalls so call-site
                // diagnostics (type-mismatch, need-check-nil) re-emit against the refreshed
                // param types. Without this, calls drained on their first resolution
                // (when @built-name class args were still unresolved) would never have
                // their arg types checked again.
                let symbol_exprs: HashSet<ExprId> = self.ir.symbols.iter()
                    .flat_map(|s| s.versions.iter())
                    .filter_map(|v| v.type_source)
                    .collect();
                pending_calls = self.ir.exprs.iter().enumerate()
                    .filter(|(_, e)| matches!(e, Expr::FunctionCall { .. }))
                    .map(|(i, _)| ExprId(i))
                    .filter(|id| !symbol_exprs.contains(id))
                    .collect();
                for (si, sym) in self.ir.symbols.iter().enumerate() {
                    for (vi, ver) in sym.versions.iter().enumerate() {
                        if let Some(expr_id) = ver.type_source {
                            // Re-resolve call expressions (for call-site diagnostics) and
                            // OverloadNarrow expressions — plus StripNil/StripFalsy, which
                            // commonly wrap OverloadNarrow-backed SymbolRefs and would
                            // otherwise hold onto stale pre-refinement types.
                            if matches!(self.ir.exprs[expr_id.val()],
                                Expr::FunctionCall { .. }
                                | Expr::OverloadNarrow { .. }
                                | Expr::StripNil(_)
                                | Expr::StripFalsy(_)) {
                                pending.push((SymbolIndex(si), vi));
                            }
                        }
                    }
                }
            }
        }

        self.resolve_deep_field_injections();
        self.resolve_deferred_field_assignments();
    }

    fn is_structurally_duplicate_type(&mut self, types: &[ValueType], new: &ValueType) -> bool {
        types.iter().any(|existing| {
            if existing == new { return true; }
            match (existing, new) {
                (ValueType::Table(Some(idx_a)), ValueType::Table(Some(idx_b))) => {
                    let ta = self.ir.table(*idx_a);
                    let tb = self.ir.table(*idx_b);
                    if ta.class_name.is_some() || tb.class_name.is_some()
                        || ta.key_type != tb.key_type
                        || ta.value_type != tb.value_type
                        || ta.fields.len() != tb.fields.len()
                        || !ta.fields.keys().all(|k| tb.fields.contains_key(k))
                    {
                        return false;
                    }
                    let field_pairs: Vec<_> = ta.fields.iter()
                        .map(|(k, fa)| (fa.expr, tb.fields[k].expr))
                        .collect();
                    field_pairs.iter().all(|(ea, eb)| {
                        match (self.resolve_expr(*ea), self.resolve_expr(*eb)) {
                            (Some(a), Some(b)) => a == b,
                            (None, None) => true,
                            _ => false,
                        }
                    })
                }
                _ => false,
            }
        })
    }

    /// After the fixpoint loop, infer `key_type`/`value_type` for table constructors
    /// that have bracket-keyed fields (or array fields) but couldn't be fully resolved
    /// at Phase 1 (literals only).
    fn infer_bracket_field_types(&mut self) -> bool {
        let table_indices: Vec<TableIndex> = self.ir.bracket_key_fields.keys().copied().collect();
        let mut made_progress = false;
        for table_idx in table_indices {
            let already_resolved = self.ir.tables[table_idx.val()].key_type.is_some();

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
                        if !self.is_structurally_duplicate_type(&new_types, &vt) { new_types.push(vt); }
                    } else {
                        all_resolved = false;
                    }
                }
                if all_resolved && !new_types.is_empty() {
                    let new_vt = if new_types.len() == 1 { new_types.pop().unwrap() }
                                 else { ValueType::make_union(new_types) };
                    if self.ir.tables[table_idx.val()].value_type.as_ref() != Some(&new_vt) {
                        self.ir.tables[table_idx.val()].value_type = Some(new_vt);
                        made_progress = true;
                    }
                }
                continue;
            }

            let bracket_fields = self.ir.bracket_key_fields[&table_idx].clone();
            let array_fields = self.ir.tables[table_idx.val()].array_fields.clone();

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
                    if !self.is_structurally_duplicate_type(&val_types, &vt) { val_types.push(vt); }
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
                        if !self.is_structurally_duplicate_type(&val_types, &vt) { val_types.push(vt); }
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
            self.ir.tables[table_idx.val()].key_type = Some(key);
            self.ir.tables[table_idx.val()].value_type = Some(val);
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
    /// overloads. If found, create OverloadNarrow versions for the siblings.
    fn resolve_deferred_sibling_narrowings(&mut self, pending: &mut Vec<(SymbolIndex, usize)>) {
        if self.deferred_sibling_narrowings.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.deferred_sibling_narrowings);
        let mut remaining = Vec::new();
        for entry in entries {
            // Try to resolve the func expression to get the function type
            let func_type = self.resolve_expr(entry.func_expr);
            let has_return_overloads = match func_type {
                Some(ValueType::Function(Some(func_idx))) => {
                    self.ir.func(func_idx).overloads.iter().any(|o| o.is_return_only)
                }
                Some(_) => false, // Resolved but not a function — no overloads
                None => {
                    // Still can't resolve — keep for next iteration
                    remaining.push(entry);
                    continue;
                }
            };
            if has_return_overloads {
                for &(ret_index, sibling_idx) in &entry.siblings {
                    // Skip the directly-guarded sibling (already narrowed via guard in any tracking set)
                    if self.narrow_kind_for(sibling_idx, entry.scope).is_some() {
                        continue;
                    }
                    // Do NOT add to narrowed_symbols — OverloadNarrow computes the correct type
                    if let Some(new_ver) = self.ir.push_overload_narrow_version(
                        sibling_idx, entry.scope, entry.func_expr, ret_index, entry.narrowed.clone(),
                    ) {
                        // Refs to `sibling_idx` in the scope subtree were lowered
                        // before this deferred narrowing ran and still point at the
                        // pre-narrow version. Redirect them to the OverloadNarrow
                        // version so downstream diagnostics see the narrowed type.
                        self.rewrite_sym_refs_in_subtree(sibling_idx, entry.scope, new_ver);
                        pending.push((sibling_idx, new_ver));
                    }
                }
            }
        }
        self.deferred_sibling_narrowings = remaining;
    }

    /// Refine synthesized return-only overload slots whose source expressions
    /// were non-literal at build time (emitted as `ValueType::Any` placeholders).
    /// Resolves whatever candidates it can on each call and folds their types
    /// into the entry's `resolved` accumulator. The overload slot is rewritten
    /// as the union of resolved types plus a residual `Any` for any candidates
    /// still unresolved — so a permanently-unresolvable candidate doesn't block
    /// its siblings from contributing, while still communicating the imprecise
    /// position via the lingering `Any`. Returns true if any slot was updated.
    pub(super) fn refine_synthesized_return_overloads(&mut self) -> bool {
        if self.synth_return_overload_refinements.is_empty() { return false; }
        let entries = std::mem::take(&mut self.synth_return_overload_refinements);
        let mut remaining = Vec::new();
        let mut progress = false;
        for mut entry in entries {
            // Try to resolve each still-pending candidate. On success, fold its
            // type into `resolved` and drop the candidate; on failure, retain.
            entry.candidates.retain(|&eid| {
                match self.resolve_expr(eid) {
                    // Treat empty unions (from strip_nil on a nil-only type) as
                    // unresolved — the underlying expression's type likely hasn't
                    // settled yet, and Union([]) would display as "".
                    Some(ValueType::Union(ref members)) if members.is_empty() => true,
                    Some(vt) => {
                        if !entry.resolved.contains(&vt) { entry.resolved.push(vt); }
                        false
                    }
                    None => true,
                }
            });
            // Compute the new slot: resolved union, plus Any if anything is
            // still unresolved (preserving the placeholder until we know more).
            let mut members = entry.resolved.clone();
            if !entry.candidates.is_empty() && !members.contains(&ValueType::Any) {
                members.push(ValueType::Any);
            }
            let new_type = if members.is_empty() {
                ValueType::Any
            } else {
                ValueType::make_union(members)
            };
            let slot = &mut self.ir.functions[entry.function_idx.val()]
                .overloads[entry.overload_idx]
                .returns[entry.ret_pos];
            if *slot != new_type {
                *slot = new_type;
                progress = true;
            }
            // Keep the entry around while any candidate is still pending so a
            // later iteration can fold its type in as well.
            if !entry.candidates.is_empty() {
                remaining.push(entry);
            }
        }
        self.synth_return_overload_refinements = remaining;
        progress
    }

    /// Process deferred class-equality narrowings from `x == EXPR`.
    /// Once `EXPR` resolves to a class-typed value, narrow `x` to that class (both for
    /// display via `type_filtered_symbols` and for sibling narrowing via
    /// `class_narrowed_symbols`), and push `OverloadNarrow` versions for any multi-return
    /// siblings of `x` so that return-only overloads can be filtered by the class match.
    fn resolve_deferred_class_eq_narrowings(&mut self, pending: &mut Vec<(SymbolIndex, usize)>) {
        if self.deferred_class_eq_narrowings.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.deferred_class_eq_narrowings);
        let mut remaining = Vec::new();
        for (sym_idx, expr_id, scope_idx) in entries {
            let Some(resolved) = self.resolve_expr(expr_id) else {
                remaining.push((sym_idx, expr_id, scope_idx));
                continue;
            };
            // Only narrow if the resolved type is (or contains) a class table.
            let Some((class_idx, class_name)) = self.first_class_table(&resolved) else { continue };
            // Avoid re-applying the same narrowing repeatedly across fixpoint iterations.
            if self.class_narrowed_symbols.get(&scope_idx)
                .and_then(|m| m.get(&sym_idx))
                .is_some_and(|n| n == &class_name)
            {
                continue;
            }
            self.class_narrowed_symbols.entry(scope_idx).or_default()
                .insert(sym_idx, class_name.clone());
            // Symbol-level display narrowing: filter the resolved type to the class.
            let class_vt = ValueType::Table(Some(class_idx));
            self.type_filtered_symbols.entry(scope_idx).or_default()
                .insert(sym_idx, class_vt.clone());
            // Push a TypeFilter version so references within this scope pick up the
            // narrowed type through `version_for_scope`.
            self.push_type_filter_version(sym_idx, class_vt, scope_idx, false);
            let trigger_ver = self.ir.symbols[sym_idx.val()].versions.len() - 1;
            // Feed the new version into the fixpoint queue so it gets resolved.
            pending.push((sym_idx, trigger_ver));

            // Update the directly-narrowed symbol's SymbolRef expressions in the subtree
            // (the TypeFilter version was just pushed — direct the refs there).
            self.rewrite_sym_refs_in_subtree(sym_idx, scope_idx, trigger_ver);

            // Propagate to multi-return siblings.
            let Some(siblings) = self.multi_return_siblings.get(&sym_idx).cloned() else { continue };
            // Only wire overload narrowing for functions with return-only overloads.
            let overload_check = self.check_return_only_overloads_from_siblings(&siblings);
            let func_expr = match overload_check {
                OverloadCheck::HasOverloads(fe) => Some(fe),
                OverloadCheck::Deferred(fe) => Some(fe),
                OverloadCheck::NoOverloads => None,
            };
            if let Some(func_expr) = func_expr {
                let narrowed_info = self.collect_narrowed_sibling_info(&siblings, scope_idx);
                if !narrowed_info.is_empty() {
                    for &(ret_index, sibling_idx) in &siblings {
                        if sibling_idx == sym_idx { continue; }
                        if self.narrow_kind_for(sibling_idx, scope_idx).is_some() { continue; }
                        if let Some(new_ver) = self.ir.push_overload_narrow_version(
                            sibling_idx, scope_idx, func_expr, ret_index, narrowed_info.clone(),
                        ) {
                            self.rewrite_sym_refs_in_subtree(sibling_idx, scope_idx, new_ver);
                            pending.push((sibling_idx, new_ver));
                        }
                    }
                }
            }
        }
        self.deferred_class_eq_narrowings = remaining;
    }

    /// Retroactively redirect SymbolRef expressions for `sym_idx` that reside in `root_scope`
    /// or any of its descendant scopes to version `new_ver`. Also updates `symbol_version_at`,
    /// invalidates the resolved-expression cache for each rewritten site, and prunes stale
    /// diagnostics that were emitted based on the pre-narrowing type.
    ///
    /// Only rewrites sites whose current version is STRICTLY LESS than `new_ver` so that
    /// re-invoking this helper is idempotent and so that assignment-created reassignment
    /// versions (which are newer) aren't clobbered by a narrowing update.
    pub(crate) fn rewrite_sym_refs_in_subtree(&mut self, sym_idx: SymbolIndex, root_scope: ScopeIndex, new_ver: usize) {
        let Some(sites) = self.sym_ref_sites.get(&sym_idx).cloned() else { return };
        for (expr_id, offset) in sites {
            let Some(site_scope) = self.ir.scope_at_offset(offset) else { continue };
            if !self.is_scope_in_subtree(site_scope, root_scope) { continue; }
            let old_ver = if let Expr::SymbolRef(s, v) = self.ir.expr(expr_id) {
                if *s != sym_idx { continue; }
                *v
            } else {
                continue;
            };
            if old_ver >= new_ver { continue; }
            self.ir.exprs[expr_id.val()] = Expr::SymbolRef(sym_idx, new_ver);
            self.symbol_version_at.insert(offset, new_ver);
            self.resolved_expr_cache.remove(&expr_id);
        }
    }

    /// Check if `candidate` is the same as `root` or a descendant scope of `root`.
    fn is_scope_in_subtree(&self, candidate: ScopeIndex, root: ScopeIndex) -> bool {
        if candidate == root { return true; }
        let mut current = self.ir.scopes.get(candidate.val()).and_then(|s| s.parent);
        while let Some(s) = current {
            if s == root { return true; }
            if s.is_external() { break; }
            current = self.ir.scopes[s.val()].parent;
        }
        false
    }

    /// Extract the first class-typed table (with a `class_name`) from a resolved value,
    /// scanning unions. Returns (table_idx, class_name) or None.
    fn first_class_table(&self, vt: &ValueType) -> Option<(TableIndex, String)> {
        match vt {
            ValueType::Table(Some(idx)) => {
                self.ir.table(*idx).class_name.clone().map(|n| (*idx, n))
            }
            ValueType::Union(ts) => ts.iter().find_map(|t| self.first_class_table(t)),
            _ => None,
        }
    }

    /// Resolve an OverloadNarrow expression: filter return-only overloads by narrowed
    /// siblings and compute the union of types at `ret_index` across compatible overloads.
    fn resolve_overload_narrow(&mut self, inner: ExprId, func_expr: ExprId, ret_index: usize, narrowed: &[(usize, NarrowKind)]) -> Option<ValueType> {
        // Try to resolve the function to get its overloads
        let func_type = self.resolve_expr(func_expr);
        let func_idx = match &func_type {
            Some(ValueType::Function(Some(idx))) => Some(*idx),
            _ => None,
        };
        if let Some(func_idx) = func_idx {
            let overloads: Vec<_> = self.func(func_idx).overloads.iter()
                .filter(|o| o.is_return_only)
                .cloned()
                .collect();
            if !overloads.is_empty() {
                // Filter overloads compatible with all narrowed siblings.
                // `return_type_at` honors `has_vararg_tail` — positions past
                // a case's declared arity fall through to the vararg element
                // type (else implicit nil, matching Lua runtime semantics).
                let compatible: Vec<_> = overloads.iter().filter(|o| {
                    narrowed.iter().all(|(sibling_ret_idx, kind)| {
                        let ovl_type = o.return_type_at(*sibling_ret_idx);
                        self.overload_type_compatible_with(&ovl_type, kind)
                    })
                }).collect();
                if !compatible.is_empty() {
                    let mut types = Vec::new();
                    for o in &compatible {
                        let t = o.return_type_at(ret_index);
                        if !types.contains(&t) {
                            types.push(t);
                        }
                    }
                    return Some(ValueType::make_union(types));
                }
            }
        }
        // Fallback: strip nil / falsy based on the narrowed kinds that carry a direction.
        // ClassEq and StripTruthy don't have a clean value-level fallback; leave the type alone.
        let any_strip_falsy = narrowed.iter().any(|(_, k)| matches!(k, NarrowKind::StripFalsy));
        let any_strip_nil = narrowed.iter().any(|(_, k)| matches!(k, NarrowKind::StripNil));
        self.resolve_expr(inner).map(|vt| {
            if any_strip_falsy { vt.strip_falsy() }
            else if any_strip_nil { vt.strip_nil() }
            else { vt }
        })
    }

    /// Check if an overload type at a return position is compatible with the given narrow kind.
    fn overload_type_compatible_with(&self, t: &ValueType, kind: &NarrowKind) -> bool {
        match kind {
            NarrowKind::StripNil => Self::overload_type_survives_strip_nil(t),
            NarrowKind::StripFalsy => Self::overload_type_survives_strip_falsy(t),
            NarrowKind::StripTruthy => Self::overload_type_survives_strip_truthy(t),
            NarrowKind::ClassEq(class_name) => self.overload_type_matches_class(t, class_name),
        }
    }

    /// Check if an overload type at a position would survive strip_nil (has non-nil values).
    fn overload_type_survives_strip_nil(t: &ValueType) -> bool {
        match t {
            ValueType::Nil => false,
            ValueType::Union(ts) => ts.iter().any(|member| !matches!(member, ValueType::Nil)),
            _ => true,
        }
    }

    /// Check if an overload type at a position would survive strip_falsy (has truthy values).
    fn overload_type_survives_strip_falsy(t: &ValueType) -> bool {
        // strip_falsy doesn't narrow Boolean(None) to Boolean(Some(true)),
        // but for overload types, literal booleans are typical.
        // Also check for pure-nil and pure-false explicitly.
        match t {
            ValueType::Nil | ValueType::Boolean(Some(false)) => false,
            ValueType::Union(ts) => ts.iter().any(|member| {
                !matches!(member, ValueType::Nil | ValueType::Boolean(Some(false)))
            }),
            _ => true,
        }
    }

    /// Check if an overload type at a position would survive strip_truthy (has nil or false).
    /// Used when a sibling is narrowed to falsy (e.g. `if not x then` or `if x then` else-branch).
    fn overload_type_survives_strip_truthy(t: &ValueType) -> bool {
        fn member_is_falsy(m: &ValueType) -> bool {
            matches!(m, ValueType::Nil | ValueType::Boolean(Some(false)))
                // Boolean(None) covers both true and false, so it survives.
                || matches!(m, ValueType::Boolean(None))
        }
        match t {
            ValueType::Union(ts) => ts.iter().any(member_is_falsy),
            other => member_is_falsy(other),
        }
    }

    /// Check if an overload type at a position is compatible with the named class.
    /// Survives when the overload type contains (or intersects) the class — direct match,
    /// inheritance in either direction, or presence in a union.
    fn overload_type_matches_class(&self, t: &ValueType, class_name: &str) -> bool {
        match t {
            ValueType::Table(Some(idx)) => self.table_matches_class(*idx, class_name),
            ValueType::Union(ts) => ts.iter().any(|m| self.overload_type_matches_class(m, class_name)),
            ValueType::Any => true,
            _ => false,
        }
    }

    fn table_matches_class(&self, idx: TableIndex, class_name: &str) -> bool {
        let info = self.ir.table(idx);
        if info.class_name.as_deref() == Some(class_name) {
            return true;
        }
        if let Some(&target_idx) = self.ir.classes.get(class_name)
            .or_else(|| self.ir.ext.classes.get(class_name))
        {
            // Bidirectional subclass check: the overload may be annotated with a
            // base class while the narrowing came from a subclass instance, or
            // vice-versa. Either direction means some value could be both types,
            // so the overload position is compatible with the narrowing.
            if self.ir.is_subclass_of(idx, target_idx) || self.ir.is_subclass_of(target_idx, idx) {
                return true;
            }
        }
        false
    }

    /// After the fixpoint loop, resolve deep field injections (e.g. `self._plot.dot = expr`)
    /// by walking the intermediate chain to find the actual target table, then adding the
    /// field there so deferred undefined-field checks can find it.
    fn resolve_deep_field_injections(&mut self) {
        let injections = std::mem::take(&mut self.deep_field_injections);
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
                        if let Some(vt) = self.resolve_expr(e)
                            && !types.contains(&vt) {
                                types.push(vt);
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
                    crate::annotations::default_visibility_for_name(&inj.field_name, self.implicit_protected_prefix)
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
                if !current_table.is_external() {
                    self.ir.tables[current_table.val()].fields.insert(inj.field_name, fi);
                } else {
                    self.ir.insert_overlay_field(current_table, inj.field_name, fi);
                }
            }
        }
    }

    /// After the fixpoint loop, resolve field assignments on variables whose class table
    /// wasn't known during Phase 1 (e.g. type comes from a function return).
    fn resolve_deferred_field_assignments(&mut self) {
        let assignments = std::mem::take(&mut self.deferred_field_assignments);
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

            let field_existed = self.class_has_field(table_idx, &assign.field_name);
            self.ir.field_assignments.push(FieldAssignment {
                table_idx, root_name: assign.root_name.clone(), field_name: assign.field_name.clone(),
                actual_expr: assign.expr_id,
                scope_idx: assign.scope_idx, block_stmt_index: assign.block_stmt_index,
                ident_start: assign.ident_start, ident_end: assign.ident_end,
                expr_start: assign.expr_start, expr_end: assign.expr_end,
                field_existed_at_build: field_existed,
                had_annotation_at_build: false,
                lateinit: false,
                in_constructor: false,
                in_function: true,
                is_method_def: assign.is_method_def,
            });

            // Register the field on the table — ad-hoc injected fields default to Public;
            // self._foo inside a method keeps implicit protected from _ prefix.
            let vis = if assign.root_name == "self" {
                crate::annotations::default_visibility_for_name(&assign.field_name, self.implicit_protected_prefix)
            } else {
                crate::annotations::Visibility::Public
            };
            if !table_idx.is_external() {
                if let Some(fi) = self.ir.tables[table_idx.val()].fields.get_mut(&assign.field_name) {
                    fi.extra_exprs.push(assign.expr_id);
                } else {
                    self.ir.tables[table_idx.val()].fields.insert(assign.field_name.clone(), FieldInfo {
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
                    if overlay_fi.annotation.is_none() {
                        if let Some(ref ann) = assign.inline_annotation {
                            overlay_fi.annotation = Some(ann.clone());
                        }
                        if assign.inline_annotation_text.is_some() {
                            overlay_fi.annotation_text = assign.inline_annotation_text.clone();
                        }
                        if overlay_fi.annotation_type_raw.is_none() {
                            overlay_fi.annotation_type_raw = assign.inline_type_raw.clone();
                        }
                    }
                    if assign.inline_is_lateinit { overlay_fi.lateinit = true; }
                } else {
                    // Inherit annotations: inline `---@type` > external `@field` > none
                    let source_fi = if assign.inline_annotation.is_some() {
                        Some((&assign.inline_annotation, &assign.inline_annotation_text,
                              &assign.inline_type_raw, assign.inline_is_lateinit))
                    } else {
                        self.ir.table(table_idx).fields.get(&assign.field_name)
                            .map(|f| (&f.annotation, &f.annotation_text, &f.annotation_type_raw, f.lateinit))
                    };
                    let (ann, ann_text, ann_raw, lateinit) = match source_fi {
                        Some((a, t, r, l)) => (a.clone(), t.clone(), r.clone(), l),
                        None => (None, None, None, false),
                    };
                    self.ir.insert_overlay_field(table_idx, assign.field_name.clone(), FieldInfo {
                        expr: assign.expr_id,
                        extra_exprs: Vec::new(),
                        visibility: vis,
                        annotation: ann,
                        annotation_text: ann_text,
                        annotation_type_raw: ann_raw,
                        lateinit,
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
        // Cache successful resolutions (None = not yet resolvable, retry next iteration).
        // Skip caching SymbolRef — it reads version.resolved_type directly, so the
        // cache would go stale when the fixpoint loop updates the version. The read
        // is a cheap vec index; caching it risks masking updates from BranchMerge
        // and other volatile expressions within the same inner-loop pass.
        if result.is_some() && !matches!(self.expr(expr_id), Expr::SymbolRef(..)) {
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
                return match self.resolve_expr(inner).map(|vt| vt.strip_nil()) {
                    Some(ValueType::Union(ref members)) if members.is_empty() => None,
                    other => other,
                };
            }
            Expr::StripFalsy(inner) => {
                let inner = *inner;
                return match self.resolve_expr(inner).map(|vt| vt.strip_falsy()) {
                    Some(ValueType::Union(ref members)) if members.is_empty() => None,
                    other => other,
                };
            }
            Expr::OverloadNarrow { inner, func_expr, ret_index, narrowed } => {
                let inner = *inner;
                let func_expr = *func_expr;
                let ret_index = *ret_index;
                let narrowed = narrowed.clone();
                return self.resolve_overload_narrow(inner, func_expr, ret_index, &narrowed);
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
                return if types.is_empty() {
                    None
                } else {
                    Some(self.ir.dedupe_union_tables(ValueType::make_union(types)))
                };
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

            Expr::FunctionCall { func, args, arg_ranges, ret_index, discarded: _, is_method_call, .. } => {
                self.resolve_function_call(expr_id, func, args, arg_ranges, ret_index, super::resolve_call::CallSiteInfo {
                    is_method_call: *is_method_call,
                })
            }

            Expr::FieldAccess { table, field, field_range: _ } => {
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
                                if let Some(vt) = self.resolve_expr(expr_id)
                                    && !field_types.contains(&vt) {
                                        field_types.push(vt);
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

                // _G global-environment redirect: look up field as a scope0 symbol
                for &idx in &table_indices {
                    if self.ir.is_global_env(idx) {
                        let sym_id = SymbolIdentifier::Name(field.clone());
                        let sym_idx = self.ir.scopes[0].symbols.get(&sym_id).copied()
                            .or_else(|| self.ir.ext.scope0_symbols.get(&sym_id).copied());
                        if let Some(si) = sym_idx {
                            let sym = self.sym(si);
                            if let Some(vt) = sym.versions.last().and_then(|v| v.resolved_type.clone()) {
                                return Some(vt);
                            }
                        }
                        // Don't emit undefined-field for _G tables
                        return None;
                    }
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
                                    if let Some(vt) = self.resolve_expr(expr)
                                        && !parent_field_types.contains(&vt) {
                                            parent_field_types.push(vt);
                                        }
                                }
                            }
                        }
                    }
                    if !parent_field_types.is_empty() {
                        return Some(ValueType::make_union(parent_field_types));
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
                                let table_idx = TableIndex(self.ir.tables.len());
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
                let table_expr = *table;
                let table_type = self.resolve_expr(table_expr)?;
                // Bracket index on any yields any
                if matches!(table_type, ValueType::Any) { return Some(ValueType::Any); }
                match &table_type {
                    ValueType::Table(Some(idx)) => {
                        let vt = self.table(*idx).value_type.clone();
                        if let Some(ref val) = vt
                            && val.contains_type_variable() {
                                let type_args = self.get_expr_type_args(table_expr);
                                if !type_args.is_empty() {
                                    let params = self.table(*idx).class_type_params.clone();
                                    let subs: HashMap<String, ValueType> = params.into_iter()
                                        .zip(type_args)
                                        .collect();
                                    return Some(self.substitute_generics_deep(val, &subs));
                                }
                            }
                        vt
                    }
                    ValueType::Union(types) => {
                        let mut value_types: Vec<ValueType> = Vec::new();
                        for t in types {
                            if let ValueType::Table(Some(idx)) = t
                                && let Some(vt) = &self.table(*idx).value_type
                                    && !value_types.contains(vt) {
                                        value_types.push(vt.clone());
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
                    match iter_type {
                    ValueType::Function(Some(func_idx)) => {
                        // Get return type at var_index from the iterator function
                        let effective_var_idx = self.func(func_idx).effective_return_index(var_idx);
                        let ret_vt = self.func(func_idx).return_annotations.get(effective_var_idx).cloned();
                        if let Some(ref vt) = ret_vt
                            && !vt.contains_type_variable() {
                                return ret_vt;
                            }
                        // Try return symbol
                        let func_scope = self.func(func_idx).scope;
                        let ret_id = SymbolIdentifier::FunctionRet(func_idx, var_idx);
                        if let Some(ret_sym_idx) = self.get_symbol(&ret_id, func_scope) {
                            let ret_type = self.sym(ret_sym_idx).versions.first()
                                .and_then(|v| v.resolved_type.clone());
                            if let Some(ref vt) = ret_type
                                && !vt.contains_type_variable() {
                                    return ret_type;
                                }
                        }
                    }
                    ValueType::Table(Some(table_idx)) => {
                        if let Some(call_func_idx) = self.table(table_idx).call_func {
                            let class_type_params = self.table(table_idx).class_type_params.clone();
                            let type_args = self.get_expr_type_args(iter_call);
                            // Check for returns<F> projection with type_args substitution
                            if let Some(crate::types::ProjectionKind::Return(ref name)) =
                                self.func(call_func_idx).return_projections.get(&0).cloned()
                            {
                                let bound = class_type_params.iter().enumerate()
                                    .find(|(_, p)| *p == name)
                                    .and_then(|(pos, _)| type_args.get(pos).cloned());
                                if let Some(ValueType::Function(Some(f_idx))) = bound {
                                    let f_returns = self.func(f_idx).return_annotations.clone();
                                    let f_has_vararg = self.func(f_idx).has_vararg_return;
                                    let vt = f_returns.get(var_idx).cloned()
                                        .or_else(|| {
                                            if f_has_vararg && !f_returns.is_empty() {
                                                f_returns.last().cloned()
                                            } else if f_returns.is_empty() {
                                                let f_scope = self.func(f_idx).scope;
                                                let ret_id = SymbolIdentifier::FunctionRet(f_idx, var_idx);
                                                self.get_symbol(&ret_id, f_scope)
                                                    .and_then(|si| self.sym(si).versions.first()
                                                        .and_then(|v| v.resolved_type.clone()))
                                            } else { None }
                                        });
                                    return vt.or(Some(ValueType::Nil));
                                }
                            }
                            // Fallback: direct return annotations from call_func
                            let effective_var_idx = self.func(call_func_idx).effective_return_index(var_idx);
                            let ret_vt = self.func(call_func_idx).return_annotations.get(effective_var_idx).cloned();
                            if let Some(ref vt) = ret_vt
                                && !vt.contains_type_variable() {
                                    return ret_vt;
                                }
                        }
                    }
                    _ => {}
                    }
                }

                // Fallback: infer from the argument table's key_type/value_type.
                // This handles generic iterators (pairs/ipairs) where K/V aren't fully inferred.
                let iter_expr = self.expr(iter_call).clone();
                if let Expr::FunctionCall { args, .. } = &iter_expr
                    && let Some(&first_arg) = args.first()
                        && let Some(arg_type) = self.resolve_expr(first_arg)
                            && let ValueType::Table(Some(table_idx)) = arg_type {
                                match var_idx {
                                    0 => return self.table(table_idx).key_type.clone(),
                                    1 => return self.table(table_idx).value_type.clone(),
                                    _ => {}
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
            return resolve_binary_op_standalone(op, lhs_type, rhs_type)
                .map(|vt| self.ir.dedupe_union_tables(vt));
        }

        // Table operand present: try standard first (needs clone to preserve for metamethod fallback)
        if let Some(result) = resolve_binary_op_standalone(op, lhs_type, rhs_type) {
            return Some(self.ir.dedupe_union_tables(result));
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
                if let Some(ref ann) = fi.annotation
                    && let ValueType::Function(Some(func_idx)) = ann {
                        return self.func(*func_idx).return_annotations.first().cloned();
                    }
                if let Expr::FunctionDef(func_idx) = self.expr(fi.expr) {
                    return self.func(*func_idx).return_annotations.first().cloned();
                }
            }
        }
        None
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

