use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::types::*;
use super::Analysis;
use super::build_ir::OverloadCheck;

// ── Type Resolution (Phase 2) ──────────────────────────────────────────────────

/// Check if a function's return annotation at `ret_idx` was declared with `!`
/// (non-nil assertion, e.g. `V!`). Used by for-in resolution to strip nil from
/// iteration variables when the iterator stub explicitly marks returns as non-nil.
fn is_forin_non_nil_return(func: &Function, ret_idx: usize) -> bool {
    func.return_annotations_raw
        .get(ret_idx)
        .is_some_and(|raw| matches!(raw, crate::annotations::AnnotationType::NonNil(_)))
}

impl<'a> Analysis<'a> {
    pub fn resolve_types(&mut self) {
        // Pre-size the expression cache and cycle-detection bitmap as dense Vecs.
        // Only local expressions (< EXT_BASE) are indexed; external ones resolve via fast paths.
        // Cache slots are initialized to None (= "not yet resolved"). Only Some(_) results
        // are ever stored, so None always means "no cached result" — never "resolved to nothing".
        self.resolved_expr_cache.resize(self.ir.exprs.len(), None);
        self.resolving_exprs.resize(self.ir.exprs.len(), false);

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
            if self.resolve_work_count >= Self::MAX_RESOLVE_WORK {
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

                // Check work limit before each inner iteration — the retain
                // passes call resolve_expr which increments the counter.
                if self.resolve_work_count >= Self::MAX_RESOLVE_WORK {
                    break;
                }

                pending.retain(|&(si, vi)| {
                    let expr_id = self.ir.symbols[si.val()].versions[vi].type_source.unwrap();
                    let expr = self.expr(expr_id);
                    let is_branch_merge = matches!(expr, Expr::BranchMerge(_));
                    // BinaryOp Or/And may resolve via a partial fallback when one
                    // operand hasn't resolved yet. Like BranchMerge, keep them in
                    // pending so they can improve once the operand resolves.
                    let is_volatile_binop = matches!(expr,
                        Expr::BinaryOp { op, .. } if *op == Operator::Or || *op == Operator::And);
                    // SymbolRef versions read their target's resolved_type
                    // live (no cache, no recursion). The target may still be
                    // partially resolved when the ref first resolves, so keep
                    // SymbolRefs volatile to track the target's improving type.
                    // This is critical for alias versions from push_alias_version
                    // but applies to any cross-symbol `local y = x` too.
                    let is_alias_ref = matches!(expr, Expr::SymbolRef(..));
                    let is_volatile = is_branch_merge || is_volatile_binop || is_alias_ref;
                    if is_volatile
                        && let Some(slot) = self.resolved_expr_cache.get_mut(expr_id.val()) {
                        *slot = None;
                    }
                    if let Some(resolved) = self.resolve_expr(expr_id) {
                        let prev = self.ir.symbols[si.val()].versions[vi].resolved_type.replace(resolved.clone());
                        // Propagate event type display alias through SymbolRef assignments
                        // so `local e = event` also shows the event type name.
                        if let Expr::SymbolRef(src_sym, src_ver) = self.ir.exprs[expr_id.val()]
                            && !src_sym.is_external()
                            && let Some(alias) = self.ir.event_type_display.get(&(src_sym, src_ver)).cloned()
                        {
                            self.ir.event_type_display.insert((si, vi), alias);
                        }
                        if is_volatile && prev.as_ref() != Some(&resolved) {
                            // Result changed — keep in pending for another
                            // iteration so that newly resolved operands can contribute.
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

                // Process deferred event-param narrowings: once event_params has been
                // propagated from overload contextual typing, resolve event payloads.
                if self.resolve_deferred_event_narrowings() {
                    continue;
                }

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
                // Propagate param types from class field annotations to
                // inline functions in table constructors (e.g. `---@type X`
                // above `local x = { handler = function(self, arg) end }`).
                if self.infer_table_constructor_field_params() {
                    new_resolution = true;
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
                // Expand single-return tail calls: create FunctionRet symbols at
                // higher slots once the callee's return arity is known.
                let tail_expanded = self.expand_resolved_tail_call_returns();
                if !tail_expanded.is_empty() {
                    for &(si, vi) in &tail_expanded {
                        pending.push((si, vi));
                    }
                    new_resolution = true;
                }
                // Detect destructure+re-return pass-throughs (e.g.
                // `local a, b = Callee(...); return a, b`) and propagate
                // callee's return-only overloads to the wrapper.
                if self.propagate_passthrough_return_overloads() {
                    new_resolution = true;
                }
                if !new_resolution {
                    break;
                }
                // Clear expression cache so dependent expressions (e.g. field access
                // on re-resolved params) get re-evaluated in the next fixpoint iteration.
                // Builder chain call results are preserved via `builder_call_memo` so
                // re-resolution doesn't duplicate the built tables.
                self.resolved_expr_cache.fill(None);
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
                                | Expr::StripFalsy(_)
                                | Expr::BinaryOp { .. }
                                | Expr::BranchMerge(_)) {
                                pending.push((SymbolIndex(si), vi));
                            }
                        }
                    }
                }
            }
        }

        self.dedup_synthesized_return_overloads();
        // Order matters: deferred field assignments must run first so that
        // runtime fields (e.g. `self.display = CreateFrame(...)`) are visible
        // when deep field injections walk intermediate chains
        // (e.g. `self.display.wrapped = ...`).
        self.resolve_deferred_field_assignments();
        self.resolve_deep_field_injections();
        self.finalize_enum_kinds();
    }

    /// After the fixpoint loop, determine each local `@enum` table's value kind
    /// from its resolved field types. If all fields resolve to `number` → `Number`;
    /// all `string` → `String`; otherwise leave as `Number` (the default from prescan)
    /// and let the `mixed-enum-values` diagnostic report the issue.
    fn finalize_enum_kinds(&mut self) {
        for table_idx in 0..self.ir.tables.len() {
            if !self.ir.tables[table_idx].enum_kind.is_enum() { continue; }
            // Skip external tables (stubs) — their enum kind is authoritative
            if table_idx >= EXT_BASE { continue; }
            // Key enums are always String — skip value-based reclassification
            if self.ir.tables[table_idx].is_key_enum { continue; }
            let fields: Vec<ExprId> = self.ir.tables[table_idx].fields.values()
                .map(|f| f.expr)
                .collect();
            if fields.is_empty() { continue; }

            let resolved: Vec<Option<ValueType>> = fields.iter()
                .map(|&expr_id| self.resolve_expr(expr_id))
                .collect();
            let classification = EnumFieldClassification::from_types(
                resolved.iter().map(|v| v.as_ref())
            );
            self.ir.tables[table_idx].enum_kind = classification.to_enum_kind();
        }
    }

    pub(super) fn is_structurally_duplicate_type(&mut self, types: &[ValueType], new: &ValueType) -> bool {
        types.iter().any(|existing| {
            if existing == new { return true; }
            self.types_structurally_match(existing, new, 0)
        })
    }

    /// Recursively compare two `ValueType`s for structural equivalence,
    /// treating `Table(Some(idx_a))` and `Table(Some(idx_b))` as equal when
    /// their metadata (class_name, key/value types, field count/names) matches
    /// and their resolved field types match recursively.
    /// `depth` guards against unbounded recursion (e.g. circular field refs).
    fn types_structurally_match(&mut self, a: &ValueType, b: &ValueType, depth: usize) -> bool {
        match (a, b) {
            (ValueType::Table(Some(idx_a)), ValueType::Table(Some(idx_b))) => {
                if idx_a == idx_b { return true; }
                if depth > 8 { return false; }
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
                if ta.fields.is_empty() { return true; }
                let field_pairs: Vec<_> = ta.fields.iter()
                    .map(|(k, fa)| (fa.expr, tb.fields[k].expr))
                    .collect();
                // Resolve all field expressions first (needs &mut self),
                // then compare — recursing for nested table types.
                let resolved: Vec<_> = field_pairs.into_iter()
                    .map(|(ea, eb)| (self.resolve_expr(ea), self.resolve_expr(eb)))
                    .collect();
                resolved.into_iter().all(|(ra, rb)| match (ra, rb) {
                    (Some(a), Some(b)) => a == b || self.types_structurally_match(&a, &b, depth + 1),
                    (None, None) => true,
                    _ => false,
                })
            }
            _ => false,
        }
    }

    /// After the fixpoint loop, infer `key_type`/`value_type` for table constructors
    /// that have bracket-keyed fields (or array fields) but couldn't be fully resolved
    /// at Phase 1 (literals only).
    fn infer_bracket_field_types(&mut self) -> bool {
        let table_indices: Vec<TableIndex> = self.ir.bracket_key_fields.keys().copied().collect();
        let mut made_progress = false;
        for table_idx in table_indices {
            let already_resolved = self.ir.tables[table_idx.val()].key_type.is_some();

            // If value_type was set from an annotation (`@type T[]`, `table<K,V>`),
            // bracket assignments must not override it — the annotation is authoritative.
            if already_resolved && self.ir.tables[table_idx.val()].value_type_annotated {
                continue;
            }

            // If key_type/value_type were already set (Phase 1 literals or earlier
            // fixpoint iteration), update value_type from bracket assignment types.
            // Bracket-indexed assignments overwrite elements, so the assigned type
            // replaces the original element type (e.g. `parts[i] = parseInt(parts[i])`
            // changes a string[] to number[]).
            // Nil assignments are excluded — writing nil clears a slot, it does not
            // change the element type of the list.
            if already_resolved {
                let bracket_fields = self.ir.bracket_key_fields[&table_idx].clone();
                let mut new_types: Vec<ValueType> = Vec::new();
                let mut all_resolved = true;
                for (_key_expr, val_expr) in &bracket_fields {
                    if let Some(vt) = self.resolve_expr_to_broad_type(*val_expr) {
                        if vt != ValueType::Nil && !self.is_structurally_duplicate_type(&new_types, &vt) { new_types.push(vt); }
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
            let constructor_bracket_count = self.ir.tables[table_idx.val()].constructor_bracket_count;
            // When the table has array fields (e.g. `{strsplit(",", s)}`), only process
            // constructor bracket fields on the first pass. Post-construction bracket
            // assignments (entries beyond constructor_bracket_count) are deferred to the
            // `already_resolved` branch on the next fixpoint iteration. This prevents
            // self-referential widening: `tbl[i] = converted_val` would otherwise union
            // with the original array field types on the same pass, causing reads like
            // `local val = tbl[i]` to see both original and converted types.
            // When there are no array fields (e.g. `local t = {}; t[1] = val`), process
            // all bracket fields immediately since they are the only type source.
            //
            // ORDERING INVARIANT: This relies on BracketIndex expressions resolving in
            // the fixpoint iteration where value_type is first set (from array fields).
            // Once resolved, non-volatile expressions are removed from `pending` and
            // won't be re-resolved when the `already_resolved` branch later replaces
            // value_type with the post-construction assignment types.
            let defer_post_construction = !array_fields.is_empty()
                && bracket_fields.len() > constructor_bracket_count;
            let effective_bracket_fields = if defer_post_construction {
                &bracket_fields[..constructor_bracket_count]
            } else {
                &bracket_fields[..]
            };

            let mut key_types: Vec<ValueType> = Vec::new();
            let mut val_types: Vec<ValueType> = Vec::new();
            let mut all_resolved = true;

            for (key_expr, val_expr) in effective_bracket_fields {
                if let Some(kt) = self.resolve_expr_to_broad_type(*key_expr) {
                    if !key_types.contains(&kt) { key_types.push(kt); }
                } else {
                    all_resolved = false;
                }
                if let Some(vt) = self.resolve_expr_to_broad_type(*val_expr) {
                    // Nil assignments clear a slot — don't include nil in the inferred element type
                    if vt != ValueType::Nil && !self.is_structurally_duplicate_type(&val_types, &vt) { val_types.push(vt); }
                } else {
                    all_resolved = false;
                }
            }

            // Also consider array (positional) fields.
            // Unlike bracket assignments, nil in a constructor (`{nil, 1}`) is kept
            // because it's an explicit positional element, not a slot-clearing write.
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

            // When the table constructor has positional elements AND later bracket
            // assignments mutate elements, save the original array-only element type
            // for display purposes (hover/inlay hints). The resolved value_type
            // (union of both) is used for type checking; initial_value_type preserves
            // what the constructor produced so `{strsplit(","  , s)}` shows `string[]`.
            let has_post_construction = bracket_fields.len() > constructor_bracket_count;
            if !array_fields.is_empty() && has_post_construction {
                let mut initial_types: Vec<ValueType> = Vec::new();
                for af in &array_fields {
                    if let Some(vt) = self.resolve_expr_to_broad_type(*af)
                        && !self.is_structurally_duplicate_type(&initial_types, &vt)
                    {
                        initial_types.push(vt);
                    }
                }
                if !initial_types.is_empty() {
                    let initial_vt = if initial_types.len() == 1 { initial_types.pop().unwrap() }
                                     else { ValueType::make_union(initial_types) };
                    self.ir.tables[table_idx.val()].initial_value_type = Some(initial_vt);
                }
            }

            let key = if key_types.len() == 1 { key_types.pop().unwrap() }
                      else { ValueType::make_union(key_types) };
            // Strip nil from inferred key type — nil keys can't exist in a Lua table.
            let key = key.strip_nil();
            if matches!(&key, ValueType::Union(types) if types.is_empty()) { continue; }
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
        let mut entries = std::mem::take(&mut self.deferred_sibling_narrowings);
        // Process entries carrying more narrowing context last. `rewrite_sym_refs_in_subtree`
        // only advances a ref to a *newer* version (higher index), so the last-applied
        // narrowing for a given sibling+scope wins. When several early-exit guards in the
        // same scope produce overlapping deferred entries (e.g. `[(0,StripTruthy)]` then
        // `[(0,StripTruthy),(1,StripNil)]`), the most complete one must win so the
        // OverloadNarrow filters against every active guard.
        entries.sort_by_key(|e| e.narrowed.len());
        let mut remaining = Vec::new();
        let mut rewrote_any = false;
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
                    // Skip the guard symbol(s) — they already have their own
                    // StripNil/StripFalsy version from the build_ir phase.
                    // Creating an OverloadNarrow for a guard and rewriting its
                    // refs would apply the narrowed type beyond the `and` RHS
                    // scope (e.g. to the LHS of the `and` expression), causing
                    // false-positive `redundant-or` diagnostics.
                    //
                    // This is the deferred-path equivalent of `narrow_siblings`'
                    // `sibling_idx == sym_idx` check — since the triggering
                    // symbol index is not stored in the deferred entry, we match
                    // by return position instead. This is slightly broader: when
                    // multiple positions are narrowed (e.g. `a and b and <expr>`),
                    // ALL siblings at narrowed positions are skipped, whereas the
                    // non-deferred path only skips the single `sym_idx` per call.
                    // The `sort_by_key(|e| e.narrowed.len())` ordering mitigates
                    // this: smaller (less-narrowed) entries run first, giving
                    // siblings their OverloadNarrow before a broader entry skips
                    // them.
                    if entry.narrowed.iter().any(|(pos, _)| *pos == ret_index) {
                        continue;
                    }
                    // Skip siblings reassigned since the multi-return assignment.
                    // `sibling_was_reassigned` sees through OverloadNarrow versions
                    // so a partial deferred entry (processed earlier by the
                    // narrowed.len() sort) doesn't block a fuller one from
                    // re-narrowing the same sibling with the complete guard set.
                    if self.sibling_was_reassigned(sibling_idx, entry.scope, ret_index) {
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
                        if self.rewrite_sym_refs_in_subtree(sibling_idx, entry.scope, new_ver) {
                            rewrote_any = true;
                        }
                        pending.push((sibling_idx, new_ver));
                    }
                }
            } else if let Some(ValueType::Function(Some(func_idx))) = &func_type {
                // Function resolved but has no overloads yet. If it also has no
                // return annotations, it might gain overloads from tail-call or
                // pass-through propagation during stall recovery. Retain the entry.
                if self.ir.func(*func_idx).return_annotations.is_empty() {
                    remaining.push(entry);
                    continue;
                }
            }
        }
        // A rewritten ref may be wrapped by parent expressions (e.g. a `StripNil`
        // applied to the argument by an earlier nil-guard) whose cached type is
        // now stale. Clearing the entire cache is broader than strictly necessary
        // (only transitively-dependent parents need invalidation), but deferred
        // sibling narrowings are rare and building a parent-expression dependency
        // graph isn't worth the complexity. A targeted approach would track
        // rewritten ExprIds and walk their parent chain, but there is no
        // parent-pointer map in the IR.
        if rewrote_any {
            self.resolved_expr_cache.fill(None);
        }
        self.deferred_sibling_narrowings = remaining;
    }

    /// Expand single-return tail-call functions during Phase 2. When a function's
    /// only return is a tail call (FunctionRet at slot 0 backed by a FunctionCall
    /// expression) and the callee is now known to have more return slots, create
    /// additional FunctionRet symbols at higher slots so hover and call-site
    /// resolution see the full multi-return signature.
    ///
    /// Returns the newly created (SymbolIndex, version_index) pairs for the caller
    /// to add to the pending-resolution list.
    fn expand_resolved_tail_call_returns(&mut self) -> Vec<(SymbolIndex, usize)> {
        let mut new_pending: Vec<(SymbolIndex, usize)> = Vec::new();

        // Collect candidate functions: local functions with no return annotations,
        // no synthesized return-only overloads, and rets whose max slot is 0.
        // The `max_slot > 0` check makes already-expanded functions O(rets.len())
        // to skip — acceptable since this runs only on fixpoint stall (typically
        // 0–2 times per file).
        let mut candidates: Vec<FunctionIndex> = Vec::new();
        for (fi, func) in self.ir.functions.iter().enumerate() {
            if !func.return_annotations.is_empty() { continue; }
            if func.overloads.iter().any(|o| o.is_return_only) { continue; }
            if func.rets.is_empty() { continue; }

            // Check max slot across all rets — once expanded, max_slot > 0
            // and the function is skipped on all subsequent iterations.
            let max_slot = func.rets.iter()
                .filter_map(|&sym_idx| {
                    if sym_idx.is_external() { return None; }
                    match &self.ir.symbols[sym_idx.val()].id {
                        SymbolIdentifier::FunctionRet(_, slot) => Some(*slot),
                        _ => None,
                    }
                })
                .max()
                .unwrap_or(0);
            if max_slot > 0 { continue; }
            candidates.push(FunctionIndex(fi));
        }

        for func_id in candidates {
            let rets = self.ir.functions[func_id.val()].rets.clone();

            // Group by DefNode
            let mut groups: HashMap<(u32, u32), Vec<(usize, SymbolIndex)>> = HashMap::new();
            for &sym_idx in &rets {
                if sym_idx.is_external() { continue; }
                let sym = &self.ir.symbols[sym_idx.val()];
                let SymbolIdentifier::FunctionRet(_, slot) = sym.id else { continue };
                let Some(ver) = sym.versions.first() else { continue };
                let key = (ver.def_node.start, ver.def_node.end);
                groups.entry(key).or_default().push((slot, sym_idx));
            }

            // For each pure tail call at slot 0, check callee's arity
            for group in groups.values() {
                if group.len() != 1 { continue; }
                let &(slot, sym_idx) = &group[0];
                if slot != 0 { continue; }

                let sym = &self.ir.symbols[sym_idx.val()];
                let scope_idx = sym.scope_idx;
                let Some(ver) = sym.versions.first() else { continue };
                let def_node = ver.def_node;
                let Some(type_source) = ver.type_source else { continue };

                // Must be a FunctionCall at ret_index 0
                let expr = self.ir.expr(type_source).clone();
                let (callee_func_expr, args, arg_ranges, call_range, is_method_call) = match &expr {
                    Expr::FunctionCall { func, args, arg_ranges, ret_index: 0, call_range, is_method_call, .. } =>
                        (*func, args.clone(), arg_ranges.clone(), *call_range, *is_method_call),
                    _ => continue,
                };

                // Resolve the callee's function type
                let Some(callee_type) = self.resolve_expr(callee_func_expr) else { continue };
                let callee_type = callee_type.into_strip_opaque();
                let callee_func_idx = match callee_type {
                    ValueType::Function(Some(idx)) => idx,
                    ValueType::Union(ref types) | ValueType::Intersection(ref types) => {
                        match types.iter().find_map(|t| match t {
                            ValueType::Function(Some(idx)) => Some(*idx),
                            _ => None,
                        }) {
                            Some(idx) => idx,
                            None => continue,
                        }
                    }
                    _ => continue,
                };

                // Determine callee's return arity.
                // Mutual recursion is safe: if A tail-calls B and B tail-calls A,
                // both have max_slot == 0 and callee_arity == 1, so neither expands.
                // Only a callee with already-established arity > 1 (from annotations
                // or multi-expression returns) triggers expansion.
                let callee_func = self.func(callee_func_idx);
                let callee_arity = if !callee_func.return_annotations.is_empty() {
                    callee_func.return_annotations.len()
                } else {
                    // Infer from callee's rets
                    callee_func.rets.iter()
                        .filter_map(|&s| match &self.sym(s).id {
                            SymbolIdentifier::FunctionRet(_, idx) => Some(*idx + 1),
                            _ => None,
                        })
                        .max()
                        .unwrap_or(0)
                };

                if callee_arity <= 1 { continue; }

                // Expand: create FunctionRet symbols for slots 1..callee_arity
                for new_slot in 1..callee_arity {
                    let new_expr = Expr::FunctionCall {
                        func: callee_func_expr,
                        args: args.clone(),
                        arg_ranges: arg_ranges.clone(),
                        ret_index: new_slot,
                        call_range,
                        discarded: false,
                        is_method_call,
                    };
                    let new_expr_id = self.ir.push_expr(new_expr);
                    let symbol_idx = self.ir.insert_symbol(
                        SymbolIdentifier::FunctionRet(func_id, new_slot),
                        scope_idx,
                        def_node,
                    );
                    self.ir.set_type_source(symbol_idx, new_expr_id);
                    let func_def = self.ir.functions.get_mut(func_id.val()).unwrap();
                    if !func_def.rets.contains(&symbol_idx) {
                        func_def.rets.push(symbol_idx);
                    }
                    new_pending.push((symbol_idx, 0));
                }

                // Propagate callee's return-only overloads to the wrapper so
                // callers of the wrapper benefit from sibling narrowing.
                let callee_overloads: Vec<ResolvedOverload> = self.func(callee_func_idx)
                    .overloads.iter()
                    .filter(|o| o.is_return_only)
                    .cloned()
                    .collect();
                if !callee_overloads.is_empty() {
                    let func_def = self.ir.functions.get_mut(func_id.val()).unwrap();
                    for ovl in callee_overloads {
                        func_def.push_unique_overload(ovl);
                    }
                    self.ir.synthesized_overload_funcs.insert(func_id);
                }
            }
        }
        new_pending
    }

    /// Detect functions whose single return statement re-returns values from a
    /// destructured multi-return call (with possible `@as` casts), and propagate
    /// the callee's return-only overloads to the wrapper using a nil-pattern
    /// split of the wrapper's resolved return types. Returns true if any
    /// overloads were propagated.
    ///
    /// Pattern: `local a, b = Callee(...); return a --[[@as T?]], b`
    /// Detection: trace each FunctionRet's type_source (or original_type_source
    /// for @as positions) back through SymbolRef → FunctionCall to identify the
    /// underlying callee. All positions must trace to the same call site.
    fn propagate_passthrough_return_overloads(&mut self) -> bool {
        let mut propagated = false;

        // Build the candidate set on first call; reuse on subsequent iterations.
        let candidates = self.passthrough_candidates.get_or_insert_with(|| {
            let mut cands = Vec::new();
            for (fi, func) in self.ir.functions.iter().enumerate() {
                if !func.return_annotations.is_empty() { continue; }
                if func.rets.is_empty() { continue; }
                cands.push(FunctionIndex(fi));
            }
            cands
        }).clone();

        let mut remaining: Vec<FunctionIndex> = Vec::new();

        for func_id in candidates {
            // Skip if this candidate already gained overloads in a prior iteration
            if self.ir.functions[func_id.val()].overloads.iter().any(|o| o.is_return_only) {
                continue;
            }
            let rets = self.ir.functions[func_id.val()].rets.clone();

            // Group by DefNode to find return statements
            let mut groups: HashMap<(u32, u32), Vec<(usize, SymbolIndex)>> = HashMap::new();
            for &sym_idx in &rets {
                if sym_idx.is_external() { continue; }
                let sym = &self.ir.symbols[sym_idx.val()];
                let SymbolIdentifier::FunctionRet(_, slot) = sym.id else { continue };
                let Some(ver) = sym.versions.first() else { continue };
                let key = (ver.def_node.start, ver.def_node.end);
                groups.entry(key).or_default().push((slot, sym_idx));
            }

            // Must be a single return statement with arity >= 2
            if groups.len() != 1 { continue; }
            let mut group = groups.into_values().next().unwrap();
            group.sort_by_key(|(slot, _)| *slot);
            if group.len() < 2 { continue; }

            // For each position, trace back to the underlying callee FunctionCall.
            // Use type_source first, then original_type_source (for @as positions).
            // Track which positions have @as (traced via original_type_source).
            let mut callee_call_range: Option<(u32, u32)> = None;
            let mut callee_func_idx: Option<FunctionIndex> = None;
            let mut all_traced = true;
            let mut has_as_cast: Vec<bool> = Vec::new();

            for &(_, sym_idx) in &group {
                let (ts, ots) = {
                    let sym = &self.ir.symbols[sym_idx.val()];
                    let Some(ver) = sym.versions.first() else { all_traced = false; break; };
                    (ver.type_source, ver.original_type_source)
                };

                // Try type_source first (direct SymbolRef), then original_type_source (@as)
                let (traced, is_as) = match self.trace_ret_to_callee_call(ts) {
                    Some(result) => (Some(result), false),
                    None => match self.trace_ret_to_callee_call(ots) {
                        Some(result) => (Some(result), true),
                        None => (None, false),
                    }
                };

                match traced {
                    Some((cr, fi)) => {
                        has_as_cast.push(is_as);
                        if let Some(existing_cr) = callee_call_range {
                            if existing_cr != cr {
                                all_traced = false;
                                break;
                            }
                        } else {
                            callee_call_range = Some(cr);
                            callee_func_idx = Some(fi);
                        }
                    }
                    None => {
                        all_traced = false;
                        break;
                    }
                }
            }

            if !all_traced { remaining.push(func_id); continue; }
            let Some(callee_func_idx) = callee_func_idx else { remaining.push(func_id); continue; };

            // Check if callee has return-only overloads
            let callee_overloads: Vec<ResolvedOverload> = self.func(callee_func_idx)
                .overloads.iter()
                .filter(|o| o.is_return_only)
                .cloned()
                .collect();
            if callee_overloads.is_empty() { remaining.push(func_id); continue; }

            let any_as_cast = has_as_cast.iter().any(|&x| x);

            // For positions with @as casts, resolve the wrapper's return type
            // so we can apply nil-pattern splitting on those positions.
            let mut resolved_types: Vec<Option<ValueType>> = Vec::new();
            if any_as_cast {
                for &(_, sym_idx) in &group {
                    let ver = &self.ir.symbols[sym_idx.val()].versions[0];
                    let rt = ver.type_source.and_then(|ts| self.resolve_expr(ts));
                    resolved_types.push(rt);
                }
            }

            // Generate propagated overloads:
            // - For direct positions (no @as): use callee's overload type directly
            // - For @as positions: nil-pattern split (nil → nil, non-nil → resolved.strip_nil())
            let mut new_overloads: Vec<ResolvedOverload> = Vec::new();
            for callee_ovl in &callee_overloads {
                let mut returns: Vec<ValueType> = Vec::new();
                let mut valid = true;

                for (i, &is_as) in has_as_cast.iter().enumerate() {
                    let callee_type = callee_ovl.return_type_at(i);
                    if !is_as {
                        // Direct pass-through: use callee's type exactly
                        returns.push(callee_type);
                    } else {
                        // @as position: nil-pattern split
                        if matches!(callee_type, ValueType::Nil) {
                            returns.push(ValueType::Nil);
                        } else if let Some(rt) = resolved_types.get(i).and_then(|r| r.as_ref()) {
                            returns.push(rt.strip_nil());
                        } else {
                            valid = false;
                            break;
                        }
                    }
                }
                if !valid { break; }
                new_overloads.push(ResolvedOverload {
                    params: Vec::new(),
                    returns,
                    is_return_only: true,
                    description: callee_ovl.description.clone(),
                    has_vararg_tail: callee_ovl.has_vararg_tail,
                    is_vararg: false,
                    returns_self_type_args: None,
                });
            }

            if new_overloads.len() == callee_overloads.len() {
                let func_def = self.ir.functions.get_mut(func_id.val()).unwrap();
                for ovl in new_overloads {
                    func_def.push_unique_overload(ovl);
                }
                self.ir.synthesized_overload_funcs.insert(func_id);
                propagated = true;
            } else {
                remaining.push(func_id);
            }
        }
        self.passthrough_candidates = Some(remaining);
        propagated
    }

    /// Trace a FunctionRet's type_source through SymbolRef → symbol's type_source
    /// → FunctionCall, returning the call_range and callee function index.
    fn trace_ret_to_callee_call(&mut self, type_source: Option<ExprId>) -> Option<((u32, u32), FunctionIndex)> {
        let ts = type_source?;
        let sym_ref = match self.ir.expr(ts) {
            Expr::SymbolRef(sym, ver) => (*sym, *ver),
            _ => return None,
        };
        // External symbols don't exist in per-file ir.symbols — bail out.
        if sym_ref.0.val() >= EXT_BASE {
            return None;
        }
        let sym = &self.ir.symbols[sym_ref.0.val()];
        let ver = sym.versions.get(sym_ref.1)?;
        let inner_ts = ver.type_source?;
        match self.ir.expr(inner_ts) {
            Expr::FunctionCall { func, call_range, .. } => {
                let func_expr = *func;
                let cr = *call_range;
                // Resolve the callee function
                let callee_type = self.resolve_expr(func_expr)?;
                let callee_type = callee_type.into_strip_opaque();
                let func_idx = match callee_type {
                    ValueType::Function(Some(idx)) => idx,
                    ValueType::Union(ref types) | ValueType::Intersection(ref types) => {
                        types.iter().find_map(|t| match t {
                            ValueType::Function(Some(idx)) => Some(*idx),
                            _ => None,
                        })?
                    }
                    _ => return None,
                };
                Some(((cr.0, cr.1), func_idx))
            }
            _ => None,
        }
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
            // Resolve all candidates. Candidates whose underlying expression
            // is a BranchMerge may return progressively wider types across
            // fixpoint iterations (as more branches resolve), so we always
            // re-resolve and keep candidates alive until fully settled.
            let mut any_unresolved = false;
            let mut resolved_this_round: Vec<ValueType> = Vec::new();
            for &eid in &entry.candidates {
                match self.resolve_expr(eid) {
                    Some(ValueType::Union(ref members)) if members.is_empty() => {
                        any_unresolved = true;
                    }
                    // Treat `Any` as unresolved — it typically means the
                    // underlying expression (OverloadNarrow, BranchMerge, etc.)
                    // hasn't settled yet and will refine to a concrete type
                    // on a later fixpoint iteration.
                    Some(ValueType::Any) => {
                        any_unresolved = true;
                    }
                    // Union containing Any: strip the Any member(s) and use
                    // only the concrete parts.  The Any portion indicates an
                    // unrefined sub-expression that will settle later.
                    Some(ValueType::Union(ref members)) if members.contains(&ValueType::Any) => {
                        any_unresolved = true;
                        let concrete: Vec<ValueType> = members.iter()
                            .filter(|m| !matches!(m, ValueType::Any))
                            .cloned().collect();
                        if !concrete.is_empty() {
                            let vt = ValueType::make_union(concrete);
                            if !resolved_this_round.contains(&vt) {
                                resolved_this_round.push(vt);
                            }
                        }
                    }
                    Some(vt) => {
                        if !resolved_this_round.contains(&vt) {
                            resolved_this_round.push(vt);
                        }
                    }
                    None => {
                        any_unresolved = true;
                    }
                }
            }
            // Merge new resolutions into the accumulated set.
            for vt in resolved_this_round {
                if !entry.resolved.contains(&vt) {
                    entry.resolved.push(vt);
                }
            }
            // Compute the new slot: resolved union, plus Any if anything is
            // still unresolved (preserving the placeholder until we know more).
            let mut members = entry.resolved.clone();
            if any_unresolved && !members.contains(&ValueType::Any) {
                members.push(ValueType::Any);
            }
            let new_type = if members.is_empty() {
                ValueType::Any
            } else {
                ValueType::make_union(members.clone())
            };
            let slot = &mut self.ir.functions[entry.function_idx.val()]
                .overloads[entry.overload_idx]
                .returns[entry.ret_pos];
            if *slot != new_type {
                *slot = new_type;
                progress = true;
            }
            // Always keep the entry for re-resolution on the next iteration,
            // since BranchMerge expressions may widen as dependent types settle.
            remaining.push(entry);
        }
        self.synth_return_overload_refinements = remaining;
        progress
    }

    /// After the fixpoint loop, deduplicate synthesized return-only overloads
    /// whose resolved return types have become identical (e.g. two branches both
    /// returning `(boolean, string?)` collapse to one). If fewer than 2 distinct
    /// overloads remain, remove them all — a single overload provides no sibling
    /// narrowing benefit over the plain return-type fallback.
    ///
    /// Also merges pairs of overloads that differ at exactly one position by
    /// unioning the differing types (e.g. `(false, T, nil)` + `(false, T, string)`
    /// → `(false, T, string | nil)`). This reduces display noise while preserving
    /// narrowing fidelity: the merged overload covers the same cases as the pair,
    /// so sibling narrowing produces identical results.
    fn dedup_synthesized_return_overloads(&mut self) {
        for func in &mut self.ir.functions {
            let synth_count = func.overloads.iter().filter(|o| o.is_return_only).count();
            if synth_count < 2 { continue; }

            // Exact dedup: remove overloads with identical return type tuples.
            let mut seen: Vec<Vec<ValueType>> = Vec::new();
            func.overloads.retain(|o| {
                if !o.is_return_only { return true; }
                if seen.iter().any(|s| s == &o.returns) { return false; }
                seen.push(o.returns.clone());
                true
            });

            // Single-position merge: repeatedly find pairs of synthesized overloads
            // that differ at exactly one position and collapse them by unioning that
            // position's type. Terminates because each merge reduces the overload count.
            // Note: with 3+ overloads, the greedy left-to-right scan may produce
            // different merge groupings depending on which pair is found first, but
            // this is purely cosmetic — sibling narrowing sees the same type space
            // regardless of merge order.
            loop {
                let n = func.overloads.len();
                let mut merged = false;
                'find: for i in 0..n {
                    if !func.overloads[i].is_return_only { continue; }
                    for j in (i + 1)..n {
                        if !func.overloads[j].is_return_only { continue; }
                        let a = &func.overloads[i].returns;
                        let b = &func.overloads[j].returns;
                        if a.len() != b.len() { continue; }
                        let diffs: Vec<usize> = (0..a.len())
                            .filter(|&k| a[k] != b[k])
                            .collect();
                        if diffs.len() != 1 { continue; }
                        let p = diffs[0];
                        // Don't merge when either side is Any — Any represents
                        // genuine uncertainty from an unresolved return expression.
                        // Merging would absorb it via make_union subsumption,
                        // collapsing the overloads and silently dropping the
                        // distinction between "returns concrete type" and "returns
                        // unknown type."
                        if a[p] == ValueType::Any || b[p] == ValueType::Any {
                            continue;
                        }
                        // Don't merge when either side is a TypeVariable —
                        // it represents a per-call-site generic type that
                        // gets substituted from the caller's argument.
                        if matches!(a[p], ValueType::TypeVariable(_)) || matches!(b[p], ValueType::TypeVariable(_)) {
                            continue;
                        }
                        let new_type = ValueType::make_union(vec![
                            func.overloads[i].returns[p].clone(),
                            func.overloads[j].returns[p].clone(),
                        ]);
                        func.overloads[i].returns[p] = new_type;
                        func.overloads.remove(j);
                        merged = true;
                        break 'find;
                    }
                }
                if !merged { break; }
            }

            // Subsumption absorption: when overload B is fully subsumed by overload A
            // (B[i] assignable to A[i] for all positions), absorb B into A. At positions
            // where A has T|nil and B has T (the "nil-only" difference), tighten A to T.
            // This handles the common pattern where branch-merged locals (errType: T?)
            // are returned alongside a concrete overload (FilterError.OVERFLOW: T),
            // collapsing the spurious nil from the BranchMerge.
            {
                // Extract synthesized overload returns for the shared absorption helper.
                let synth_indices: Vec<usize> = func.overloads.iter().enumerate()
                    .filter(|(_, o)| o.is_return_only)
                    .map(|(i, _)| i)
                    .collect();
                if synth_indices.len() >= 2 {
                    let mut returns: Vec<Vec<ValueType>> = synth_indices.iter()
                        .map(|&i| func.overloads[i].returns.clone())
                        .collect();
                    // Skip if any has unresolved Any slots; otherwise absorb
                    if !returns.iter().any(|r| r.contains(&ValueType::Any))
                        && Self::absorb_subsumed_overloads(&mut returns)
                    {
                        // Write back: rebuild overloads from non-synth + absorbed synth
                        let mut new_overloads: Vec<ResolvedOverload> = func.overloads.iter()
                            .filter(|o| !o.is_return_only)
                            .cloned()
                            .collect();
                        for ret in returns {
                            new_overloads.push(ResolvedOverload {
                                params: Vec::new(),
                                returns: ret,
                                is_return_only: true,
                                description: None,
                                has_vararg_tail: false,
                                is_vararg: false,
                                returns_self_type_args: None,
                            });
                        }
                        func.overloads = new_overloads;
                    }
                }
            }

            // If dedup+merge reduced to < 2, remove all — no narrowing benefit.
            let remaining = func.overloads.iter().filter(|o| o.is_return_only).count();
            if remaining < 2 {
                func.overloads.retain(|o| !o.is_return_only);
            }
        }
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
            // External symbols (stubs) are immutable — skip narrowing.
            if sym_idx.is_external() { continue; }
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
            // Cache invalidation not needed: this runs within the standard fixpoint
            // iteration which re-resolves dependent expressions naturally.
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
                        // Skip siblings reassigned (including via @cast) since
                        // the multi-return assignment — a user-specified type
                        // override takes precedence over inferred sibling narrowing.
                        if self.sibling_was_reassigned(sibling_idx, scope_idx, ret_index) { continue; }
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

    /// Process deferred event-param narrowings. For each stored `(event_sym, literal, scope)`,
    /// check if `event_sym` is the event param of a function with `event_params`. If so,
    /// look up the event payload and set vararg types for the scope. Returns true if progress was made.
    fn resolve_deferred_event_narrowings(&mut self) -> bool {
        if self.deferred_event_narrowings.is_empty() {
            return false;
        }
        let entries = std::mem::take(&mut self.deferred_event_narrowings);
        let mut remaining = Vec::new();
        let mut made_progress = false;
        for (sym_idx, string_literal, target_scope) in entries {
            let func_info = self.find_event_params_function_info(sym_idx);
            let Some((event_type_name, event_param_idx, func_args)) = func_info else {
                remaining.push((sym_idx, string_literal, target_scope));
                continue;
            };

            let Some(events) = self.ir.ext.event_types.get(&event_type_name) else { continue; };
            let Some(payload) = events.get(&string_literal) else { continue; };
            let payload_params = payload.params.clone();

            // Narrow named params beyond event_param_idx to payload types (scoped to target_scope)
            for (payload_idx, param_info) in payload_params.iter().enumerate() {
                let func_arg_idx = event_param_idx + 1 + payload_idx;
                if let Some(&arg_sym_idx) = func_args.get(func_arg_idx) {
                    if arg_sym_idx.is_external() { continue; }
                    if let Some(vt) = Self::resolve_event_param_type_static(&self.ir, param_info) {
                        self.push_type_narrowed_version(arg_sym_idx, vt, target_scope);
                    }
                }
            }

            // Store vararg types for this scope
            let vararg_types: Vec<ValueType> = payload_params.iter()
                .filter_map(|p| Self::resolve_event_param_type_static(&self.ir, p))
                .collect();
            if !vararg_types.is_empty() {
                self.event_vararg_types.insert(target_scope, vararg_types);
            }
            made_progress = true;
        }
        self.deferred_event_narrowings = remaining;
        made_progress
    }

    fn find_event_params_function_info(&self, sym_idx: SymbolIndex) -> Option<(String, usize, Vec<SymbolIndex>)> {
        if sym_idx.is_external() { return None; }
        for func in &self.ir.functions {
            let Some((ref event_type_name, event_param_idx)) = func.event_params else { continue };
            if let Some(&arg_sym) = func.args.get(event_param_idx)
                && arg_sym == sym_idx
            {
                return Some((event_type_name.clone(), event_param_idx, func.args.clone()));
            }
        }
        None
    }

    pub(super) fn resolve_event_param_type_static(ir: &super::Ir, param: &crate::pre_globals::EventPayloadParam) -> Option<ValueType> {
        let at = crate::annotations::annotation_types::parse_type(&param.type_name);
        // No generic type variables in scope for event params
        let base = crate::annotations::resolve_annotation_type(&at, &[], &ir.ext.classes, &ir.ext.aliases)
            .or_else(|| crate::annotations::resolve_annotation_type(&at, &[], &ir.classes, &ir.aliases))
            .unwrap_or(ValueType::Any);
        if param.nilable {
            Some(ValueType::union(base, ValueType::Nil))
        } else {
            Some(base)
        }
    }

    /// Retroactively redirect SymbolRef expressions for `sym_idx` that reside in `root_scope`
    /// or any of its descendant scopes to version `new_ver`. Also updates `symbol_version_at`,
    /// invalidates the resolved-expression cache for each rewritten site, and prunes stale
    /// diagnostics that were emitted based on the pre-narrowing type.
    ///
    /// Only rewrites sites whose current version is STRICTLY LESS than `new_ver` so that
    /// re-invoking this helper is idempotent and so that assignment-created reassignment
    /// versions (which are newer) aren't clobbered by a narrowing update.
    pub(crate) fn rewrite_sym_refs_in_subtree(&mut self, sym_idx: SymbolIndex, root_scope: ScopeIndex, new_ver: usize) -> bool {
        let Some(sites) = self.sym_ref_sites.get(&sym_idx).cloned() else { return false };
        let mut rewrote = false;
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
            // Don't overwrite a version that was created for a more specific (inner)
            // child scope. This prevents a parent-scope deferred narrowing (e.g.
            // early-exit StripFalsy continuation) from clobbering a child-scope
            // narrowing (e.g. then-branch StripTruthy) that was already applied.
            let old_created_scope = self.ir.symbols[sym_idx.val()].versions[old_ver].created_in_scope;
            if old_created_scope != root_scope && self.is_scope_in_subtree(old_created_scope, root_scope) {
                continue;
            }
            // Don't rewrite past an intermediate reassignment. If any version
            // from old_ver to new_ver-1 (inclusive) was created by a real
            // assignment (its def_node.start differs from version 0's) and that
            // assignment is at or before the reference offset, the reference
            // correctly picked up the reassignment during Phase 1 lowering.
            // Overwriting it with a deferred narrowing would create a resolution
            // cycle (narrowing → assignment → narrowing → …).
            {
                let versions = &self.ir.symbols[sym_idx.val()].versions;
                let v0_start = versions[0].def_node.start;
                let is_post_reassignment = (old_ver..new_ver).any(|v| {
                    if versions[v].def_node.start == v0_start
                        || versions[v].def_node.start > offset
                    {
                        return false;
                    }
                    // A version produced by guard narrowing of the same symbol
                    // (StripNil/StripFalsy from a nil/truthy guard, OverloadNarrow
                    // from a sibling tuple-union narrowing, or a SymbolRef alias)
                    // is NOT a reassignment — it refines the same value. Only a
                    // genuine reassignment (FunctionCall, literal, different RHS)
                    // should block the deferred narrowing rewrite.
                    //
                    // Special case: a FunctionCall version in a multi-return sibling
                    // group is only "transparent" when the SymbolRef is already at
                    // that version (v == old_ver) — it's the base being refined by
                    // OverloadNarrow. When the ref is at an earlier version, the
                    // FunctionCall is a real assignment boundary (e.g. param `value`
                    // at version 0 should not be rewritten past the multi-return
                    // reassignment at version 1).
                    if v != old_ver && self.is_function_call_on_multi_return_symbol(sym_idx, v) {
                        return true;
                    }
                    !self.is_narrowing_version_of(sym_idx, v)
                });
                if is_post_reassignment {
                    continue;
                }
            }
            self.ir.exprs[expr_id.val()] = Expr::SymbolRef(sym_idx, new_ver);
            self.symbol_version_at.insert(offset, new_ver);
            if let Some(slot) = self.resolved_expr_cache.get_mut(expr_id.val()) {
                *slot = None;
            }
            rewrote = true;
        }
        rewrote
    }

    /// Whether version `ver` of `sym_idx` was produced by narrowing the same
    /// symbol (a guard or sibling tuple-union refinement) rather than a genuine
    /// reassignment. Used by `rewrite_sym_refs_in_subtree` so a guard-narrowing
    /// version doesn't masquerade as a reassignment and block a later deferred
    /// OverloadNarrow rewrite.
    fn is_narrowing_version_of(&self, sym_idx: SymbolIndex, ver: usize) -> bool {
        let Some(ts) = self.ir.symbols[sym_idx.val()].versions[ver].type_source else {
            return false;
        };
        match self.ir.expr(ts) {
            Expr::StripNil(_) | Expr::StripFalsy(_) | Expr::OverloadNarrow { .. }
            | Expr::TypeFilter(..) | Expr::CastRemove(..) => true,
            Expr::SymbolRef(s, _) => *s == sym_idx,
            // A FunctionCall version that is part of a multi-return sibling group
            // is the base value being refined by the OverloadNarrow, not an
            // unrelated reassignment that should block the rewrite.
            Expr::FunctionCall { .. } => self.is_function_call_on_multi_return_symbol(sym_idx, ver),
            _ => false,
        }
    }

    /// Whether version `ver` of `sym_idx` is a FunctionCall whose symbol is
    /// tracked in `multi_return_siblings`. Used by `rewrite_sym_refs_in_subtree`
    /// to block rewrites that jump over the multi-return assignment from a
    /// pre-assignment version.
    ///
    /// Note: uses `contains_key` rather than matching call_range because
    /// `multi_return_siblings` may have been overwritten by a later multi-return
    /// reassignment of the same symbol. This means the check can return true for
    /// a single-return FunctionCall if the symbol was later involved in a
    /// multi-return assignment. In practice this is correct — any non-`old_ver`
    /// FunctionCall should block the rewrite regardless of its return arity.
    fn is_function_call_on_multi_return_symbol(&self, sym_idx: SymbolIndex, ver: usize) -> bool {
        let Some(ts) = self.ir.symbols[sym_idx.val()].versions[ver].type_source else {
            return false;
        };
        matches!(self.ir.expr(ts), Expr::FunctionCall { .. })
            && self.multi_return_siblings.contains_key(&sym_idx)
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
                    let mut has_any_synth = false;
                    let is_synth = self.ir.synthesized_overload_funcs.contains(&func_idx);
                    // For synthesized overloads with 2+ compatible cases, apply
                    // subsumption-based nil-tightening before collecting types.
                    // When overload B is subsumed by A and differs only by nil,
                    // use B's tighter types at those positions. This handles cases
                    // where the dedup couldn't run at synthesis time (types were
                    // still Any) but the overloads have since been refined.
                    let tightened_at_ret_index: Option<ValueType> = if is_synth && compatible.len() >= 2 {
                        Self::subsumption_tighten_for_position(&compatible, ret_index)
                    } else {
                        None
                    };
                    if let Some(t) = tightened_at_ret_index {
                        if !matches!(t, ValueType::Any) {
                            types.push(t);
                        } else {
                            has_any_synth = true;
                        }
                    } else {
                        for o in &compatible {
                            let t = o.return_type_at(ret_index);
                            // Skip `Any` from synthesized overloads — it's an
                            // unrefined placeholder that will settle later. For
                            // annotated overloads, `Any` is the intended type.
                            if matches!(t, ValueType::Any) && is_synth {
                                has_any_synth = true;
                                continue;
                            }
                            if !types.contains(&t) {
                                types.push(t);
                            }
                        }
                    }
                    if !types.is_empty() {
                        // We have concrete types from compatible overloads.
                        // Skip Any values — they represent unrefined slots that
                        // will settle on later fixpoint iterations.
                        // Substitute implicit generics from the call site's
                        // argument types (cached during resolve_function_call).
                        // Try the func_expr cache first, then fall back to
                        // tracing the inner SymbolRef → FunctionCall → CallResolution.
                        let subs = self.call_site_generic_subs.get(&func_expr).cloned()
                            .or_else(|| self.find_generic_subs_from_inner(inner));
                        if let Some(ref subs) = subs {
                            types = types.into_iter()
                                .map(|t| self.substitute_generics_deep(&t, subs))
                                .collect();
                        }
                        // Resolve return projections for tuple positions that
                        // resolved to Any or Nil.  Handles `@return (true,
                        // returns<F>) | (false, string)` where the success
                        // case's position 1+ needs F's projected return types.
                        let projs = self.func(func_idx).return_projections.clone();
                        if !projs.is_empty() {
                            types = types.into_iter().map(|t| {
                                if !matches!(t, ValueType::Any | ValueType::Nil) {
                                    return t;
                                }
                                self.resolve_projection_for_narrow(
                                    &projs, ret_index, subs.as_ref(),
                                ).unwrap_or(t)
                            }).collect();
                        }
                        return Some(ValueType::make_union(types));
                    } else if has_any_synth {
                        // All compatible synthesized overloads have Any at this
                        // position (unrefined slots). Return None so the fixpoint
                        // loop retries once the overload slots are refined.
                        return None;
                    }
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

    /// Apply subsumption-based nil-tightening on a set of compatible overloads and
    /// return the effective type at `ret_index`. Returns `Some(type)` if tightening
    /// was applicable (caller uses this instead of the normal union-of-all); returns
    /// `None` if no absorption happened (caller falls back to the normal logic).
    /// When `Some(Any)` is returned, it means all types were Any (unrefined) — the
    /// caller should treat it as a "retry later" signal.
    fn subsumption_tighten_for_position(compatible: &[&ResolvedOverload], ret_index: usize) -> Option<ValueType> {
        // Expand overloads to normalized return Vecs at the maximum arity,
        // using return_type_at to respect has_vararg_tail.
        let max_arity = compatible.iter().map(|o| o.returns.len()).max().unwrap_or(0);
        let mut returns: Vec<Vec<ValueType>> = compatible.iter()
            .map(|o| (0..max_arity).map(|k| o.return_type_at(k)).collect())
            .collect();
        // Skip if any overload has Any (unrefined slots)
        if returns.iter().any(|r| r.contains(&ValueType::Any)) {
            return None;
        }
        if !Self::absorb_subsumed_overloads(&mut returns) {
            return None;
        }
        // Collect the union of types at ret_index from the tightened set
        let mut types = Vec::new();
        for r in &returns {
            let t = if ret_index < r.len() { r[ret_index].clone() } else { ValueType::Nil };
            if !types.contains(&t) {
                types.push(t);
            }
        }
        Some(ValueType::make_union(types))
    }

    /// Subsumption absorption on a mutable set of return-type tuples. When overload
    /// B is fully subsumed by A (every B[k] assignable to A[k]) and they differ only
    /// by nil (A has T|nil where B has T), tighten A and remove B. Checks both
    /// directions (B subsumed by A, and A subsumed by B). Returns true if any
    /// absorption occurred.
    fn absorb_subsumed_overloads(returns: &mut Vec<Vec<ValueType>>) -> bool {
        let mut any_absorbed = false;
        loop {
            let n = returns.len();
            let mut absorbed = false;
            'absorb: for i in 0..n {
                for j in (i + 1)..n {
                    // Try B subsumed by A (tighten A, remove B)
                    if Self::try_absorb_pair(returns, i, j) {
                        absorbed = true;
                        any_absorbed = true;
                        break 'absorb;
                    }
                    // Try A subsumed by B (tighten B, remove A)
                    if Self::try_absorb_pair(returns, j, i) {
                        absorbed = true;
                        any_absorbed = true;
                        break 'absorb;
                    }
                }
            }
            if !absorbed { break; }
        }
        any_absorbed
    }

    /// Try to absorb overload at `narrow_idx` into overload at `wide_idx`.
    /// Returns true if absorption succeeded (narrow was removed, wide was tightened).
    fn try_absorb_pair(returns: &mut Vec<Vec<ValueType>>, wide_idx: usize, narrow_idx: usize) -> bool {
        let wide = &returns[wide_idx];
        let narrow = &returns[narrow_idx];
        if wide.len() != narrow.len() { return false; }
        // Check if narrow is subsumed by wide
        let subsumed = (0..wide.len()).all(|k| narrow[k].is_assignable_to(&wide[k]));
        if !subsumed { return false; }
        // Collect positions where wide has T|nil and narrow has T
        let mut tighten: Vec<(usize, ValueType)> = Vec::new();
        for k in 0..wide.len() {
            if wide[k] == narrow[k] { continue; }
            if narrow[k].contains_nil() || narrow[k] == ValueType::Nil { continue; }
            if narrow[k] == ValueType::Any || matches!(narrow[k], ValueType::TypeVariable(_)) { continue; }
            if wide[k].contains_nil() && wide[k].strip_nil() == narrow[k] {
                tighten.push((k, narrow[k].clone()));
            }
        }
        if tighten.is_empty() { return false; }
        for (k, t) in tighten {
            returns[wide_idx][k] = t;
        }
        returns.remove(narrow_idx);
        true
    }

    /// Check if an overload type at a return position is compatible with the given narrow kind.
    fn overload_type_compatible_with(&self, t: &ValueType, kind: &NarrowKind) -> bool {
        match kind {
            NarrowKind::StripNil => Self::overload_type_survives_strip_nil(t),
            NarrowKind::StripFalsy => Self::overload_type_survives_strip_falsy(t),
            NarrowKind::StripTruthy => Self::overload_type_survives_strip_truthy(t),
            NarrowKind::ClassEq(class_name) => self.overload_type_matches_class(t, class_name),
            NarrowKind::NumCompare { op, bound } => Self::overload_type_survives_num_compare(t, *op, bound),
        }
    }

    /// Check if an overload type at a position would survive a numeric comparison
    /// (`value <op> bound`). A number-literal member is eliminated only when the
    /// comparison is provably false; plain `number`/`any`/non-numeric members
    /// survive (we don't track ranges, so they can't be ruled out).
    fn overload_type_survives_num_compare(t: &ValueType, op: Operator, bound: &str) -> bool {
        fn member_survives(m: &ValueType, op: Operator, bound: f64) -> bool {
            match m {
                // An ordered comparison errors at runtime on nil, so a nil case
                // cannot reach the then-branch — eliminate it.
                ValueType::Nil => false,
                ValueType::NumberLiteral(v) => match parse_num_literal_str(v) {
                    Some(val) => match op {
                        Operator::GreaterThan => val > bound,
                        Operator::LessThan => val < bound,
                        Operator::GreaterThanOrEquals => val >= bound,
                        Operator::LessThanOrEquals => val <= bound,
                        _ => true,
                    },
                    // Unparseable literal: keep it (can't disprove).
                    None => true,
                },
                _ => true,
            }
        }
        let Some(bound) = parse_num_literal_str(bound) else { return true };
        match t {
            ValueType::Union(ts) => ts.iter().any(|m| member_survives(m, op, bound)),
            other => member_survives(other, op, bound),
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

    /// Find generic substitution bindings by tracing from an OverloadNarrow's
    /// inner SymbolRef back to its FunctionCall type_source, then looking up
    /// the CallResolution's generic_subs. This handles the case where each
    /// multi-return slot has its own FunctionCall ExprId (with different func
    /// ExprIds due to re-lowering), so the `call_site_generic_subs` cache keyed
    /// by func_expr may not contain the right entry.
    fn find_generic_subs_from_inner(&self, inner: ExprId) -> Option<HashMap<String, ValueType>> {
        // inner is a SymbolRef(sym, ver) — get its symbol's type_source
        let (sym_idx, _ver) = match self.expr(inner) {
            Expr::SymbolRef(si, v) => (*si, *v),
            _ => return None,
        };
        if sym_idx.is_external() { return None; }
        // Search this symbol and its multi-return siblings for a FunctionCall
        // with a CallResolution (only ret_index=0 stores one).
        let siblings = self.multi_return_siblings.get(&sym_idx);
        let candidates: Vec<SymbolIndex> = if let Some(sibs) = siblings {
            sibs.iter().map(|(_, si)| *si).collect()
        } else {
            vec![sym_idx]
        };
        for candidate in candidates {
            if candidate.is_external() { continue; }
            for v in self.ir.symbols[candidate.val()].versions.iter().rev() {
                let Some(ts) = v.type_source else { continue };
                if !matches!(self.ir.expr(ts), Expr::FunctionCall { .. }) { continue; }
                if let Some(cr) = self.ir.call_resolutions.get(&ts) {
                    let subs: HashMap<String, ValueType> = cr.generic_subs.iter()
                        .map(|(name, bound_type, _)| (name.clone(), bound_type.clone()))
                        .collect();
                    if !subs.is_empty() {
                        return Some(subs);
                    }
                }
            }
        }
        None
    }

    /// Resolve a `returns<F>` projection for a narrowed tuple position.
    /// Used when tuple-union narrowing selects overloads containing `returns<F>`
    /// at a given return index — the projection resolves to `Any` during generic
    /// substitution, so we look up F's concrete binding and extract its return type.
    fn resolve_projection_for_narrow(
        &self,
        projs: &std::collections::HashMap<usize, ProjectionKind>,
        ret_index: usize,
        subs: Option<&std::collections::HashMap<String, ValueType>>,
    ) -> Option<ValueType> {
        // Find the projection — direct hit at ret_index, or expansion from a lower index.
        let (proj, proj_base) = projs.get(&ret_index).map(|p| (p, ret_index))
            .or_else(|| {
                projs.iter()
                    .filter(|&(&k, _)| k < ret_index)
                    .max_by_key(|&(&k, _)| k)
                    .map(|(&k, p)| (p, k))
            })?;
        let ProjectionKind::Return(name, _) = proj else { return None };
        let bound = subs?.get(name)?;
        let ValueType::Function(Some(f_idx)) = bound else { return None };
        let f = self.func(*f_idx);
        let effective_index = ret_index - proj_base;
        f.return_annotations.get(effective_index).cloned()
            .or_else(|| {
                if f.has_vararg_return && !f.return_annotations.is_empty() {
                    f.return_annotations.last().cloned()
                } else {
                    None
                }
            })
    }

    /// After the fixpoint loop, resolve deep field injections (e.g. `self._plot.dot = expr`)
    /// by walking the intermediate chain to find the actual target table, then adding the
    /// field there so deferred undefined-field checks can find it.
    fn resolve_deep_field_injections(&mut self) {
        let injections = std::mem::take(&mut self.deep_field_injections);
        for inj in injections {
            let mut current_table = self.ir.find_table_for_symbol(&inj.root_name, inj.scope_idx);
            // Fallback: if the root symbol's table isn't found via type_source
            // or resolved_type (both checked by find_table_for_symbol), try
            // Union/Intersection resolved_type (e.g. `self` in a colon method
            // whose type is an intersection of mixins).
            if current_table.is_none()
                && let Some(sym_idx) = self.ir.get_symbol(
                    &SymbolIdentifier::Name(inj.root_name.clone()),
                    inj.scope_idx,
                )
            {
                let ver_idx = self.ir.version_for_scope(sym_idx, inj.scope_idx);
                current_table = match &self.ir.sym(sym_idx).versions[ver_idx].resolved_type {
                    Some(ValueType::Union(types)) => types.iter().find_map(|t| match t {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }),
                    Some(ValueType::Intersection(types)) => types.iter().find_map(|t| match t {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }),
                    _ => None,
                };
            }
            let Some(mut current_table) = current_table else { continue };

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
                    let mut has_unresolvable = false;
                    for e in all_exprs {
                        if let Some(vt) = self.resolve_expr(e) {
                            if !types.contains(&vt) {
                                types.push(vt);
                            }
                        } else {
                            has_unresolvable = true;
                        }
                    }
                    if has_unresolvable && skip_primary
                        && !types.contains(&ValueType::Any)
                    {
                        types.push(ValueType::Any);
                    }
                    if types.is_empty() { None } else { Some(ValueType::make_union(types)) }
                });
                match table_type {
                    Some(ValueType::Table(Some(idx))) => current_table = idx,
                    Some(ValueType::Union(ref types)) | Some(ValueType::Intersection(ref types)) => {
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
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
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
            let ver_idx = self.ir.version_for_scope(sym_idx, assign.scope_idx);
            let type_source = self.ir.sym(sym_idx).versions[ver_idx].type_source;
            let table_idx = type_source
                .and_then(|ts| self.ir.find_table_index(ts))
                .or_else(|| {
                    // Don't inject fields into tables obtained from bracket access —
                    // the resolved_type points to the collection's value_type prototype,
                    // not a writable instance.
                    if let Some(ts) = type_source
                        && matches!(self.ir.expr(ts), Expr::BracketIndex { .. })
                    {
                        return None;
                    }
                    match &self.ir.sym(sym_idx).versions[ver_idx].resolved_type {
                        Some(ValueType::Table(Some(idx))) => Some(*idx),
                        Some(ValueType::Union(types)) => types.iter().find_map(|t| match t {
                            ValueType::Table(Some(idx)) => Some(*idx),
                            _ => None,
                        }),
                        _ => None,
                    }
                });
            let Some(table_idx) = table_idx else {
                continue;
            };

            let field_existed = self.class_has_field(table_idx, &assign.field_name);
            self.ir.field_assignments.push(FieldAssignment {
                table_idx, root_name: assign.root_name.clone(), root_symbol: Some(sym_idx),
                field_name: assign.field_name.clone(),
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
                    // Propagate inline annotation if the existing field has none
                    if fi.annotation.is_none() {
                        if let Some(ref ann) = assign.inline_annotation {
                            fi.annotation = Some(ann.clone());
                        }
                        if assign.inline_annotation_text.is_some() {
                            fi.annotation_text = assign.inline_annotation_text.clone();
                        }
                        if fi.annotation_type_raw.is_none() {
                            fi.annotation_type_raw = assign.inline_type_raw.clone();
                        }
                    }
                    if assign.inline_is_lateinit { fi.lateinit = true; }
                } else {
                    self.ir.tables[table_idx.val()].fields.insert(assign.field_name.clone(), FieldInfo {
                        expr: assign.expr_id,
                        extra_exprs: Vec::new(),
                        visibility: vis,
                        annotation: assign.inline_annotation.clone(),
                        annotation_text: assign.inline_annotation_text.clone(),
                        annotation_type_raw: assign.inline_type_raw.clone(),
                        lateinit: assign.inline_is_lateinit,
                        def_range: None,
                        flavor_guard: 0,
                        description: None,
                        from_scan: false,
                    });
                }
            } else if let Some(overlay_fi) = self.ir.get_overlay_field_mut(table_idx, &assign.field_name) {
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
                        // Skip inherited Any — let the expression-based path
                        // resolve the concrete type from the assignment RHS.
                        .filter(|f| !matches!(f.annotation, Some(ValueType::Any)))
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
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
                });
            }
        }
    }

    /// Maximum recursion depth for expression resolution. Prevents stack overflow
    /// on deeply nested builder chains or pathological field access patterns.
    const MAX_RESOLVE_DEPTH: usize = 200;

    /// Maximum number of `resolve_expr` calls per analysis. Pathological inputs
    /// (e.g. deeply nested braces with repeated function patterns from fuzzing)
    /// can cause exponential re-resolution even within the depth and iteration
    /// limits. This cap ensures analysis terminates in bounded time.
    const MAX_RESOLVE_WORK: usize = 2_000_000;

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
            if self.resolved_expr_cache.get(current.val()).is_some_and(|v| v.is_some()) {
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
            self.resolve_work_count += 1;
            if self.resolve_work_count >= Self::MAX_RESOLVE_WORK {
                if self.safety_limit_hit.is_none() {
                    self.safety_limit_hit = Some(format!(
                        "expression resolution exceeded work limit ({} resolve_expr calls)",
                        Self::MAX_RESOLVE_WORK
                    ));
                }
                return None;
            }
            if let Some(cached) = self.resolved_expr_cache.get(expr_id.val()).and_then(|v| v.as_ref()) {
                last_result = Some(cached.clone());
                continue;
            }
            if let Some(slot) = self.resolving_exprs.get_mut(expr_id.val()) {
                if *slot { return None; }
                *slot = true;
            }
            self.resolve_depth += 1;
            let result = self.resolve_expr_inner(expr_id);
            self.resolve_depth -= 1;
            if let Some(slot) = self.resolving_exprs.get_mut(expr_id.val()) {
                *slot = false;
            }
            // Only cache successful resolutions — None means "not yet resolvable,
            // retry next fixpoint iteration", matching resolve_expr() semantics.
            if let Some(ref res) = result
                && let Some(slot) = self.resolved_expr_cache.get_mut(expr_id.val()) {
                *slot = Some(res.clone());
            }
            last_result = result;
            if last_result.is_none() {
                break;
            }
        }
        last_result
    }

    /// Extract string literals from a type (single literal or union of literals).
    /// Returns empty if the type contains an open `string` (non-literal) member,
    /// since we can't enumerate all possible keys in that case.
    fn extract_string_literals(vt: &ValueType) -> Vec<String> {
        match vt {
            ValueType::String(Some(s)) => vec![s.clone()],
            ValueType::String(None) => Vec::new(),
            ValueType::Union(types) => {
                let mut out = Vec::new();
                for t in types {
                    match t {
                        ValueType::String(Some(s)) => out.push(s.clone()),
                        ValueType::String(None) => return Vec::new(),
                        _ => {}
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    }

    /// Check if two types are equivalent for union deduplication purposes.
    /// Unlike `PartialEq`, this considers two `Table(Some(idx))` values equivalent
    /// when they have the same class name and key/value types (even if their indices differ).
    fn types_equivalent(&self, a: &ValueType, b: &ValueType) -> bool {
        self.types_equivalent_depth(a, b, 0)
    }

    fn types_equivalent_depth(&self, a: &ValueType, b: &ValueType, depth: usize) -> bool {
        if a == b { return true; }
        if depth > 8 { return false; }
        match (a, b) {
            (ValueType::Table(Some(ai)), ValueType::Table(Some(bi))) => {
                let ta = self.table(*ai);
                let tb = self.table(*bi);
                ta.class_name == tb.class_name
                    && self.opt_types_equivalent_depth(&ta.value_type, &tb.value_type, depth + 1)
                    && self.opt_types_equivalent_depth(&ta.key_type, &tb.key_type, depth + 1)
            }
            _ => false,
        }
    }

    fn opt_types_equivalent_depth(&self, a: &Option<ValueType>, b: &Option<ValueType>, depth: usize) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => self.types_equivalent_depth(a, b, depth),
            (None, None) => true,
            _ => false,
        }
    }

    pub(super) fn resolve_expr(&mut self, expr_id: ExprId) -> Option<ValueType> {
        self.resolve_work_count += 1;
        if self.resolve_work_count >= Self::MAX_RESOLVE_WORK {
            if self.safety_limit_hit.is_none() {
                self.safety_limit_hit = Some(format!(
                    "expression resolution exceeded work limit ({} resolve_expr calls)",
                    Self::MAX_RESOLVE_WORK
                ));
            }
            return None;
        }
        // Ultra-fast path for leaf expressions that never recurse:
        // skip cache check, cycle detection, and depth tracking entirely.
        // SymbolRef is never cached (reads directly from version), and
        // Literal/FunctionDef/TableConstructor always return immediately.
        if let Some(expr) = self.ir.exprs.get(expr_id.val()) {
            match expr {
                Expr::SymbolRef(sym_idx, ver_idx) => {
                    return self.sym(*sym_idx).versions[*ver_idx].resolved_type.clone();
                }
                Expr::Literal(vt) => return Some(vt.clone()),
                Expr::FunctionDef(func_idx) => return Some(ValueType::Function(Some(*func_idx))),
                Expr::TableConstructor(table_idx) => return Some(ValueType::Table(Some(*table_idx))),
                _ => {}
            }
        }
        // Return cached result if available (avoids re-creating tables/exprs
        // for builder chains on each fixpoint iteration)
        if let Some(cached) = self.resolved_expr_cache.get(expr_id.val()).and_then(|v| v.as_ref()) {
            return Some(cached.clone());
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
        if let Some(slot) = self.resolving_exprs.get_mut(expr_id.val()) {
            if *slot { return None; }
            *slot = true;
        }
        self.resolve_depth += 1;
        let result = self.resolve_expr_inner(expr_id);
        self.resolve_depth -= 1;
        if let Some(slot) = self.resolving_exprs.get_mut(expr_id.val()) {
            *slot = false;
        }
        // Cache successful resolutions (None = not yet resolvable, retry next iteration).
        // SymbolRef/Literal/FunctionDef/TableConstructor are handled by the leaf fast path
        // above and never reach here. Only cache local expressions (< EXT_BASE).
        if let Some(ref res) = result
            && let Some(slot) = self.resolved_expr_cache.get_mut(expr_id.val()) {
            *slot = Some(res.clone());
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
            Expr::StripTruthy(inner) => {
                let inner = *inner;
                return match self.resolve_expr(inner).map(|vt| vt.strip_truthy()) {
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
                return self.resolve_expr(inner).map(|vt| vt.strip_type_with(&cast_type, &|idx| self.table(idx).enum_kind));
            }
            Expr::TypeFilter(inner, guard_type) => {
                let inner = *inner;
                let guard_type = guard_type.clone();
                let resolved = self.resolve_expr(inner);
                return resolved.map(|vt| vt.filter_type_with(&guard_type, &|idx| self.table(idx).enum_kind));
            }
            Expr::BranchMerge(exprs) => {
                let exprs = exprs.clone();
                let mut types: Vec<ValueType> = Vec::new();
                let mut has_any = false;
                let mut has_none = false;
                for eid in exprs {
                    match self.resolve_expr(eid) {
                        // Skip Any — it typically comes from an unresolved
                        // forward-referenced call and would subsume all other
                        // branch contributions, collapsing the union to Any.
                        // BranchMerge is volatile, so once the forward ref
                        // resolves to a concrete type, re-evaluation produces
                        // the correct union.
                        Some(ValueType::Any) => { has_any = true; }
                        Some(vt) => { types.push(vt); }
                        // None means either (a) the expression hasn't resolved
                        // yet in the fixpoint loop (forward reference that
                        // will resolve on a later iteration) or (b) it is
                        // permanently unresolvable (e.g. call to an undefined
                        // function). We can't distinguish the two here, but
                        // BranchMerge is volatile: if case (a) resolves
                        // later, re-evaluation replaces any transient result.
                        // Tracked separately from has_any so that permanently
                        // unresolvable branches don't get the same treatment
                        // as forward-ref Any.
                        None => { has_none = true; }
                    }
                }
                return if types.is_empty() {
                    // All branches are Any or unresolved — return Any if at
                    // least one branch produced Any (so the merge doesn't
                    // block the fixpoint), otherwise None.
                    if has_any || has_none { Some(ValueType::Any) } else { None }
                } else if has_none && types.iter().all(|t| matches!(t, ValueType::Nil)) {
                    // All resolved branches contribute only Nil (typically
                    // the implicit-else path from `local x = nil; if cond
                    // then x = f() end`). The None branch represents a real
                    // assignment whose type couldn't be determined — returning
                    // just Nil would be a false narrowing (triggering
                    // false-positive redundant-condition on `if x then`).
                    // Note: this is narrow — a `local x = false` init would
                    // contribute Boolean (not Nil) and wouldn't hit this
                    // guard. That case doesn't cause a false positive though,
                    // since Boolean is not always-truthy or always-falsy.
                    Some(ValueType::Any)
                } else {
                    Some(self.ir.dedupe_union_tables(ValueType::make_union(types)))
                };
            }
            Expr::BinaryOp { op, lhs, rhs } => {
                let op = *op;
                let lhs = *lhs;
                let rhs = *rhs;
                let lhs_type = self.resolve_expr(lhs);
                let rhs_type = self.resolve_expr(rhs);
                // Lateinit fields (`T!`) are typed non-nil but can be nil at
                // runtime before initialization.  When used as the LHS of
                // `and`/`or` (e.g. `field and true or false`), inject nil so
                // the result type reflects possible nil truthiness.
                // The `is_guaranteed_truthy` guard is a no-op for `boolean!`
                // (which already handles both branches correctly) and avoids
                // unnecessary work when the LHS already includes nil/false.
                let lhs_type = if matches!(op, Operator::And | Operator::Or)
                    && lhs_type.as_ref().is_some_and(|t| t.is_guaranteed_truthy())
                    && self.is_lateinit_field_expr(lhs)
                {
                    lhs_type.map(|t| ValueType::union(t, ValueType::Nil))
                } else {
                    lhs_type
                };
                return match (lhs_type, rhs_type) {
                    (Some(l), Some(r)) => self.resolve_binary_op(op, l, r),
                    // When RHS is guaranteed falsy, returning it alone ignores
                    // the unknown LHS and causes false-positive redundant-and
                    // downstream (e.g. `local info = ctx.f and ctx.f() or nil`).
                    (None, Some(r)) if op == Operator::Or && !r.is_guaranteed_falsy() => Some(r),
                    (Some(ref l), None) if op == Operator::Or && l.is_guaranteed_truthy() => Some(l.clone()),
                    (Some(ValueType::Number | ValueType::NumberLiteral(_)), None) | (None, Some(ValueType::Number | ValueType::NumberLiteral(_)))
                        if op.is_arithmetic() => Some(ValueType::Number),
                    (Some(ref t), None) | (None, Some(ref t))
                        if op == Operator::Concatenate && t.can_concat_to_string() => Some(ValueType::String(None)),
                    _ if op.is_comparison() => Some(ValueType::Boolean(None)),
                    _ => None,
                };
            }
            Expr::UnaryOp { op, operand } => {
                let op = *op;
                let operand = *operand;
                let operand_type = self.resolve_expr(operand);
                return match operand_type {
                    // Unresolved operand: `not` still produces boolean (Lua semantics)
                    None if op == Operator::Not => Some(ValueType::Boolean(None)),
                    None => None,
                    Some(ref ot) => match op {
                        Operator::Not => Some(ValueType::Boolean(None)),
                        Operator::Subtract => {
                            match ot {
                                ValueType::Number | ValueType::NumberLiteral(_) => Some(ValueType::Number),
                                _ => self.resolve_unary_metamethod(op, ot),
                            }
                        }
                        Operator::ArrayLength => {
                            match ot {
                                ValueType::Table(Some(_)) => {
                                    self.resolve_unary_metamethod(op, ot)
                                        .or(Some(ValueType::Number))
                                }
                                _ => Some(ValueType::Number),
                            }
                        }
                        _ => None,
                    }
                };
            }
            Expr::Grouped(inner) => {
                let inner = *inner;
                return self.resolve_expr(inner);
            }
            Expr::Unknown => return None,
            _ => {}
        }
        // Remaining variants need &mut self — clone to release the borrow
        let expr = self.expr(expr_id).clone();
        match &expr {
            Expr::FunctionCall { func, args, arg_ranges, ret_index, discarded: _, is_method_call, .. } => {
                self.resolve_function_call(expr_id, func, args, arg_ranges, ret_index, super::resolve_call::CallSiteInfo {
                    is_method_call: *is_method_call,
                })
            }

            Expr::FieldAccess { table, field, field_range: _ } => {
                let table_type = self.resolve_expr(*table)?;
                // Field access on any yields any
                if matches!(table_type, ValueType::Any) { return Some(ValueType::Any); }
                // Unwrap opaque aliases — field access works on the inner type
                let table_type = table_type.into_strip_opaque();
                let table_indices: Vec<TableIndex> = match &table_type {
                    ValueType::Table(Some(idx)) => vec![*idx],
                    ValueType::Intersection(types) => types.iter().filter_map(|t| match t {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }).collect(),
                    ValueType::Union(types) => {
                        let mut indices = Vec::new();
                        for t in types {
                            match t {
                                ValueType::Table(Some(idx)) => indices.push(*idx),
                                ValueType::Intersection(itypes) => {
                                    for it in itypes {
                                        if let ValueType::Table(Some(idx)) = it {
                                            indices.push(*idx);
                                        }
                                    }
                                }
                                other => {
                                    if let Some(lib_idx) = self.ir.library_table_for_type(other) {
                                        indices.push(lib_idx);
                                    }
                                }
                            }
                        }
                        indices
                    }
                    // Primitive types with implicit metatables (e.g. string → string library)
                    vt => self.ir.library_table_for_type(vt).into_iter().collect(),
                };
                if table_indices.is_empty() { return None; }

                // Try each table in the union for the field, collecting types
                // Prefer @type annotation when available, else use expr + extra_exprs
                let mut field_types: Vec<ValueType> = Vec::new();
                let mut field_exists = false;
                for &idx in &table_indices {
                    if let Some(fi) = self.ir.get_field(idx, field) {
                        field_exists = true;
                        // Extract what we need before releasing the borrow on self.ir
                        let ann_vt = fi.annotation.clone();
                        let is_any = matches!(ann_vt, Some(ValueType::Any));
                        if let Some(ref ann_vt) = ann_vt {
                            if is_any {
                                // When the annotation is Any (inherited from a parent
                                // class), prefer concrete types from the primary expr
                                // and any extra_exprs (child-class assignments).
                                let primary = fi.expr;
                                let extras: Vec<ExprId> = fi.extra_exprs.clone();
                                let has_extras = !extras.is_empty();
                                let all_exprs: Vec<ExprId> = std::iter::once(primary).chain(extras).collect();
                                let mut found_specific = false;
                                let mut has_unresolvable = false;
                                for expr_id in all_exprs {
                                    if let Some(vt) = self.resolve_expr(expr_id) {
                                        if !matches!(vt, ValueType::Any | ValueType::Nil)
                                            && !field_types.contains(&vt) {
                                                field_types.push(vt);
                                                found_specific = true;
                                            }
                                    } else {
                                        has_unresolvable = true;
                                    }
                                }
                                // If the field was assigned multiple times and
                                // any assignment couldn't be resolved, the field
                                // could hold any type — keep as Any.  Uses
                                // has_extras (not skip_primary) because in this
                                // branch the primary is always Any, not Nil.
                                if has_unresolvable && has_extras {
                                    if !field_types.contains(ann_vt) {
                                        field_types.push(ann_vt.clone());
                                    }
                                } else if !found_specific && !field_types.contains(ann_vt) {
                                    field_types.push(ann_vt.clone());
                                }
                            } else if !field_types.contains(ann_vt) {
                                field_types.push(ann_vt.clone());
                            }
                        } else {
                            let primary = fi.expr;
                            let extras: Vec<ExprId> = fi.extra_exprs.clone();
                            let has_extras = !extras.is_empty();
                            // If there are reassignments and the initial value is nil,
                            // skip the nil — it's just a placeholder initializer.
                            let skip_primary = has_extras
                                && matches!(self.resolve_expr(primary), Some(ValueType::Nil));
                            let all_exprs: Vec<ExprId> = if skip_primary {
                                extras
                            } else {
                                std::iter::once(primary).chain(extras).collect()
                            };
                            let mut has_unresolvable = false;
                            for expr_id in all_exprs {
                                if let Some(vt) = self.resolve_expr(expr_id) {
                                    if !field_types.contains(&vt) {
                                        field_types.push(vt);
                                    }
                                } else {
                                    has_unresolvable = true;
                                }
                            }
                            // If the primary was a nil placeholder (skipped) and
                            // any reassignment couldn't be resolved, the field
                            // could hold any type — widen to Any.
                            if has_unresolvable && skip_primary
                                && !field_types.contains(&ValueType::Any)
                            {
                                field_types.push(ValueType::Any);
                            }
                        }
                    }
                }
                // If the field resolved to a meaningful type, return it.
                // But if all types are Table(None) placeholders (from unresolvable
                // self-referential builder chains), try parent classes first.
                let all_placeholder = !field_types.is_empty()
                    && field_types.iter().all(|vt| matches!(vt, ValueType::Table(None)));
                if !field_types.is_empty() && !all_placeholder {
                    return Some(ValueType::make_union(field_types));
                }
                // Field exists but type is unresolvable or only Table(None) placeholder.
                // For class tables, try parent classes for a better type. This handles
                // self-referential assignments like X.field = X.field:Method() where
                // the local field's expression can't resolve due to the cycle.
                if field_exists && table_indices.first().is_some_and(|&idx| self.table(idx).class_name.is_some()) {
                    let mut parent_field_types: Vec<ValueType> = Vec::new();
                    for &idx in &table_indices {
                        let parents = self.table(idx).parent_classes.clone();
                        for &parent_idx in &parents {
                            if let Some(fi) = self.ir.get_field(parent_idx, field) {
                                if let Some(ref ann_vt) = fi.annotation {
                                    if !matches!(ann_vt, ValueType::Any | ValueType::Table(None))
                                        && !parent_field_types.contains(ann_vt) {
                                        parent_field_types.push(ann_vt.clone());
                                    }
                                } else {
                                    let expr = fi.expr;
                                    if let Some(vt) = self.resolve_expr(expr)
                                        && !matches!(vt, ValueType::Any | ValueType::Table(None))
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
                // Return placeholder type if we have one, otherwise None
                if !field_types.is_empty() {
                    return Some(ValueType::make_union(field_types));
                }
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
                        0 => Some(ValueType::String(self.ir.addon_folder_name.clone())),
                        1 => {
                            if let Some(addon_idx) = self.ir.addon_table_idx() {
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
                    let ret_index = *ret_index;
                    if let Some(&scope_idx) = self.ir.varargs_scope.get(&expr_id)
                        && let Some(vt) = self.find_event_vararg_type(scope_idx, ret_index)
                    {
                        return Some(vt);
                    }
                    None
                }
            }
            Expr::BracketIndex { table, key, literal_key } => {
                let table_expr = *table;
                let literal_key = literal_key.clone();
                let table_type = self.resolve_expr(table_expr)?;
                // Bracket index on any yields any
                if matches!(table_type, ValueType::Any) { return Some(ValueType::Any); }
                // Unwrap opaque aliases — bracket index works on the inner type
                let table_type = table_type.into_strip_opaque();
                match &table_type {
                    ValueType::Table(Some(idx)) => {
                        // Literal key → try named field lookup first (e.g. op[1] → field "[1]")
                        if let Some(ref lk) = literal_key
                            && let Some(fi) = self.get_field(*idx, lk).cloned() {
                                if let Some(ann_vt) = fi.annotation {
                                    return Some(ann_vt);
                                }
                                if let Some(vt) = self.resolve_expr(fi.expr) {
                                    return Some(vt);
                                }
                            }
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
                        // No explicit value_type — try key-aware lookup, then field inference.
                        if vt.is_none() {
                            // If the key resolves to a string literal or string literal
                            // union, look up only the matching fields. This avoids unioning
                            // all field values (which produces redundant `table | table | ...`)
                            // and works correctly for named classes where all-field union is
                            // nonsensical.
                            let key_expr = *key;
                            let key_literals = self.resolve_expr(key_expr)
                                .map(|kt| Self::extract_string_literals(&kt))
                                .unwrap_or_default();
                            if !key_literals.is_empty() {
                                // Key is a string literal union (not bare `string`), so
                                // assume the table is designed for these keys — access is
                                // non-nil.
                                let mut field_types: Vec<ValueType> = Vec::new();
                                for lit in &key_literals {
                                    if let Some(fi) = self.get_field(*idx, lit).cloned()
                                        && let Some(vt) = fi.annotation.or_else(|| self.resolve_expr(fi.expr))
                                        && !field_types.iter().any(|existing| self.types_equivalent(existing, &vt))
                                    {
                                        field_types.push(vt);
                                    }
                                }
                                if !field_types.is_empty() {
                                    return Some(ValueType::make_union(field_types));
                                }
                            }
                            // Skip all-field inference for named classes — their fields are
                            // methods/properties, not dictionary values, so a union of all
                            // field types is nonsensical. Return Any so downstream expressions
                            // resolve (avoids fixpoint churn).
                            let is_named_class = self.table(*idx).class_name.is_some();
                            if is_named_class {
                                return Some(ValueType::Any);
                            }
                            let field_data: Vec<(ExprId, Option<ValueType>)> = {
                                let t = self.table(*idx);
                                if t.key_type.is_none() && !t.fields.is_empty() {
                                    t.fields.values()
                                        .map(|fi| (fi.expr, fi.annotation.clone()))
                                        .collect()
                                } else {
                                    Vec::new()
                                }
                            };
                            if !field_data.is_empty() {
                                let mut inferred: Vec<ValueType> = Vec::new();
                                for (expr, ann) in field_data {
                                    if let Some(field_vt) = ann.or_else(|| self.resolve_expr(expr))
                                        && !field_vt.contains_type_variable()
                                        && !inferred.iter().any(|existing| self.types_equivalent(existing, &field_vt)) {
                                            inferred.push(field_vt);
                                        }
                                }
                                if !inferred.is_empty() {
                                    inferred.push(ValueType::Nil);
                                    return Some(ValueType::make_union(inferred));
                                }
                            }
                        }
                        vt
                    }
                    ValueType::Union(types) => {
                        // If any member is plain `table`, bracket access is `any`.
                        if types.iter().any(|t| matches!(t, ValueType::Table(None))) {
                            return Some(ValueType::Any);
                        }
                        let mut value_types: Vec<ValueType> = Vec::new();
                        let key_type_resolved = self.resolve_expr(*key);
                        for t in types {
                            if let ValueType::Table(Some(idx)) = t {
                                // Literal key → try named field lookup first
                                if let Some(ref lk) = literal_key
                                    && let Some(fi) = self.get_field(*idx, lk).cloned() {
                                        let field_vt = fi.annotation.or_else(|| self.resolve_expr(fi.expr));
                                        if let Some(vt) = field_vt {
                                            if !value_types.iter().any(|existing| self.types_equivalent(existing, &vt)) {
                                                value_types.push(vt);
                                            }
                                            continue;
                                        }
                                    }
                                // Skip this union member if its key_type is incompatible
                                // with the actual key (e.g. indexing table<K,V>|T[] with K
                                // should only yield V, not the array element type)
                                if let Some(ref kt) = key_type_resolved
                                    && let Some(ref table_kt) = self.table(*idx).key_type
                                    && !kt.is_assignable_to(table_kt)
                                    && !self.is_table_subtype(kt, table_kt) {
                                        continue;
                                    }
                                if let Some(vt) = &self.table(*idx).value_type
                                    && !value_types.iter().any(|existing| self.types_equivalent(existing, vt)) {
                                        value_types.push(vt.clone());
                                    }
                            }
                        }
                        if value_types.is_empty() { None }
                        else { Some(ValueType::make_union(value_types)) }
                    }
                    // Plain `table` (no type params) is the most generic dictionary —
                    // bracket access yields any.
                    ValueType::Table(None) => Some(ValueType::Any),
                    _ => None,
                }
            }
            Expr::ForInVar { iterator_call, var_index, state_expr } => {
                let iter_call = *iterator_call;
                let var_idx = *var_index;
                let state_eid = *state_expr;

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
                                // var_idx 0: nil terminates the for-in loop (language guarantee).
                                // Other positions: strip nil when annotated with ! (e.g. V!).
                                if var_idx == 0 || is_forin_non_nil_return(self.func(func_idx), effective_var_idx) {
                                    return Some(self.ir.dedupe_union_tables(vt.strip_nil()));
                                }
                                return Some(self.ir.dedupe_union_tables(vt.clone()));
                            }
                        // Try return symbol
                        let func_scope = self.func(func_idx).scope;
                        let ret_id = SymbolIdentifier::FunctionRet(func_idx, var_idx);
                        if let Some(ret_sym_idx) = self.get_symbol(&ret_id, func_scope) {
                            let ret_type = self.sym(ret_sym_idx).versions.first()
                                .and_then(|v| v.resolved_type.clone());
                            if let Some(ref vt) = ret_type
                                && !vt.contains_type_variable() {
                                    if var_idx == 0 || is_forin_non_nil_return(self.func(func_idx), effective_var_idx) {
                                        return Some(self.ir.dedupe_union_tables(vt.strip_nil()));
                                    }
                                    return ret_type.map(|t| self.ir.dedupe_union_tables(t));
                                }
                        }
                        // For generic iterators with state expression (e.g. `for k, v in next, tbl`):
                        // prefer the table's explicit key_type/value_type when available,
                        // as these give the correct type without unwanted nil from the
                        // iterator's return annotations.
                        if let Some(state_eid) = state_eid {
                            if let Some(arg_type) = self.resolve_expr(state_eid) {
                                let table_indices = super::table_indices_from_type(&arg_type);
                                if !table_indices.is_empty() {
                                    match var_idx {
                                        0 => {
                                            let key_types: Vec<ValueType> = table_indices.iter()
                                                .filter_map(|&ti| self.table(ti).key_type.clone())
                                                .collect();
                                            if !key_types.is_empty() {
                                                // Control variable is never nil inside the loop body
                                                return Some(self.ir.dedupe_union_tables(ValueType::make_union(key_types)).strip_nil());
                                            }
                                        }
                                        1 => {
                                            let val_types: Vec<ValueType> = table_indices.iter()
                                                .filter_map(|&ti| self.table(ti).value_type.clone())
                                                .collect();
                                            if !val_types.is_empty() {
                                                // Lua tables cannot store nil values, so iteration
                                                // never yields nil — strip nil from the value type.
                                                return Some(self.ir.dedupe_union_tables(ValueType::make_union(val_types)).strip_nil());
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            // Fall back to generic resolution (handles tables with only named fields)
                            if let Some(substituted) = self.resolve_forin_generic_iterator(func_idx, var_idx, state_eid) {
                                // var_idx 0: nil terminates the loop (language guarantee).
                                // Other positions: strip nil when annotated with ! (e.g. V!).
                                let effective = self.func(func_idx).effective_return_index(var_idx);
                                if var_idx == 0 || is_forin_non_nil_return(self.func(func_idx), effective) {
                                    return Some(substituted.strip_nil());
                                }
                                return Some(substituted);
                            }
                        }
                    }
                    ValueType::Table(Some(table_idx)) => {
                        if let Some(call_func_idx) = self.table(table_idx).call_func {
                            let class_type_params = self.table(table_idx).class_type_params.clone();
                            let type_args = self.get_expr_type_args(iter_call);
                            // Check for returns<F> projection with type_args substitution
                            if let Some(crate::types::ProjectionKind::Return(ref name, _)) =
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

                // Fallback: infer from the table's key_type/value_type.
                // This handles generic iterators (pairs/ipairs) where K/V aren't fully inferred.
                // Check both the function call's first arg (pairs(tbl)) and the state expr (next, tbl).
                let table_arg_expr = {
                    let iter_expr = self.expr(iter_call).clone();
                    if let Expr::FunctionCall { args, .. } = &iter_expr {
                        args.first().copied()
                    } else {
                        state_eid
                    }
                };
                if let Some(table_arg) = table_arg_expr
                    && let Some(arg_type) = self.resolve_expr(table_arg) {
                        let table_indices = super::table_indices_from_type(&arg_type);
                        if !table_indices.is_empty() {
                            match var_idx {
                                // Lua tables cannot store nil keys or nil values — strip nil
                                // so that iterating a `(T|nil)[]` or `table<K|nil, V|nil>`
                                // gives T/K/V instead of T?/K?/V? in the loop variables.
                                0 => {
                                    let key_types: Vec<ValueType> = table_indices.iter()
                                        .filter_map(|&ti| self.table(ti).key_type.clone())
                                        .collect();
                                    if !key_types.is_empty() {
                                        return Some(self.ir.dedupe_union_tables(ValueType::make_union(key_types)).strip_nil());
                                    }
                                }
                                1 => {
                                    let val_types: Vec<ValueType> = table_indices.iter()
                                        .filter_map(|&ti| self.table(ti).value_type.clone())
                                        .collect();
                                    if !val_types.is_empty() {
                                        return Some(self.ir.dedupe_union_tables(ValueType::make_union(val_types)).strip_nil());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                None
            }

            _ => None,
        }
    }

    /// Bind a generic iterator function's type variables from the state expression
    /// in a multi-expression for-in (e.g. `for k, v in next, tbl`).
    fn resolve_forin_generic_iterator(&mut self, func_idx: FunctionIndex, var_idx: usize, state_eid: ExprId) -> Option<ValueType> {
        let generics: Vec<(String, Option<ValueType>)> = self.func(func_idx).generics.clone();
        if generics.is_empty() {
            return None;
        }
        let param_annotations: Vec<crate::annotations::AnnotationType> = self.func(func_idx).param_annotations.clone();
        let generic_names: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();

        let mut generic_subs = HashMap::new();

        // Bind generics from the state expression as if it were the first param
        if let Some(first_annotation) = param_annotations.first() {
            let arg_type = self.resolve_expr(state_eid);
            if let Some(ref arg_vt) = arg_type {
                // Direct TypeVariable binding
                if let crate::annotations::AnnotationType::Simple(name) = first_annotation
                    && generic_names.contains(name) {
                        generic_subs.insert(name.clone(), arg_vt.clone());
                }
                // Structural inference (handles table<K,V>, T[], etc.)
                self.infer_generics_from_annotation(first_annotation, &generic_names, &generics, &None, state_eid, &mut generic_subs);
            }
        }

        if generic_subs.is_empty() {
            return None;
        }

        let effective_var_idx = self.func(func_idx).effective_return_index(var_idx);
        let ret_vt = self.func(func_idx).return_annotations.get(effective_var_idx).cloned()?;
        let substituted = self.substitute_generics_deep(&ret_vt, &generic_subs);
        if substituted.contains_type_variable() {
            None
        } else {
            Some(substituted)
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

    fn find_event_vararg_type(&self, scope_idx: ScopeIndex, ret_index: usize) -> Option<ValueType> {
        super::ancestor_scopes(&self.ir.scopes, scope_idx)
            .find_map(|s| self.event_vararg_types.get(&s))
            .and_then(|types| types.get(ret_index).cloned())
    }

    /// Returns true when `expr_id` is a field access on a table where the
    /// field is declared as lateinit (`T!`).  Unwraps narrowing wrappers.
    fn is_lateinit_field_expr(&mut self, expr_id: ExprId) -> bool {
        let mut id = expr_id;
        while let Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::StripTruthy(inner)
            | Expr::Grouped(inner) = &self.ir.exprs[id.val()] {
            id = *inner;
        }
        let Expr::FieldAccess { table, field, .. } = &self.ir.exprs[id.val()] else { return false };
        // Clone to release borrow on self.ir.exprs before resolve_expr
        let (table, field) = (*table, field.clone());
        let Some(table_type) = self.resolve_expr(table) else { return false };
        let table_type = table_type.into_strip_opaque();
        self.ir.any_table_field_matches(&table_type, &field, |fi| fi.lateinit)
    }

}

/// Parse a number-literal type spelling (`0`, `-1`, `0xFF`, `3.14`, `1e9`) to f64.
/// Handles an optional leading `-` and `0x`/`0X` hex; returns None if unparseable.
pub(crate) fn parse_num_literal_str(s: &str) -> Option<f64> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let val = if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()? as f64
    } else {
        body.parse::<f64>().ok()?
    };
    Some(if neg { -val } else { val })
}

/// Pure function for binary op type resolution (no `self` needed).
/// Called from both `Analysis::resolve_binary_op` and `AnalysisResult::resolve_expr_type_inner`.
pub(super) fn resolve_binary_op_standalone(op: Operator, lhs_type: ValueType, rhs_type: ValueType) -> Option<ValueType> {
    // Unwrap opaque aliases — operators work on the inner type, results decay to base type
    let lhs_type = lhs_type.into_strip_opaque();
    let rhs_type = rhs_type.into_strip_opaque();
    // Number literals decay to plain numbers under operators (we don't track ranges).
    let lhs_type = lhs_type.into_decay_number_literal();
    let rhs_type = rhs_type.into_decay_number_literal();
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
                    let has_falsy = types.iter().any(|t| matches!(t, ValueType::Nil | ValueType::Boolean(Some(false)) | ValueType::Boolean(None)));
                    if has_falsy {
                        let mut remaining: Vec<ValueType> = types.iter()
                            .filter_map(|t| match t {
                                ValueType::Nil | ValueType::Boolean(Some(false)) => None,
                                // Boolean(None) can be false; keep only the truthy part
                                ValueType::Boolean(None) => Some(ValueType::Boolean(Some(true))),
                                other => Some(other.clone()),
                            })
                            .collect();
                        remaining.push(rhs_type.clone());
                        Some(ValueType::make_union(remaining))
                    } else {
                        Some(lhs_type)
                    }
                },
                // OpaqueAlias is already unwrapped at the top of this function
                (ValueType::Number | ValueType::String(_) | ValueType::Function(_) | ValueType::Table(_) | ValueType::Intersection(_) | ValueType::TypeVariable(_) | ValueType::Userdata | ValueType::Thread, _) => Some(lhs_type),
                _ => Some(lhs_type), // unreachable after unwrap, but satisfies exhaustiveness
            }
        },
        Operator::And => {
            match (&lhs_type, &rhs_type) {
                (ValueType::Any, _) => Some(ValueType::make_union(vec![rhs_type.clone(), ValueType::Nil])),
                (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => Some(lhs_type),
                (ValueType::Union(types), _) => {
                    let falsy: Vec<ValueType> = types.iter()
                        .filter_map(|t| match t {
                            ValueType::Nil | ValueType::Boolean(Some(false)) => Some(t.clone()),
                            // Boolean(None) can be false; extract the falsy part
                            ValueType::Boolean(None) => Some(ValueType::Boolean(Some(false))),
                            _ => None,
                        })
                        .collect();
                    if falsy.is_empty() {
                        Some(rhs_type)
                    } else {
                        let mut result = falsy;
                        result.push(rhs_type.clone());
                        Some(ValueType::make_union(result))
                    }
                },
                // OpaqueAlias is already unwrapped at the top of this function
                (ValueType::Boolean(Some(true)) | ValueType::Number | ValueType::String(_) | ValueType::Function(_) | ValueType::Table(_) | ValueType::Intersection(_) | ValueType::TypeVariable(_) | ValueType::Userdata | ValueType::Thread, _) => Some(rhs_type),
                (ValueType::Boolean(None), ValueType::Boolean(Some(true))) => Some(lhs_type),
                (_, ValueType::Boolean(Some(false)) | ValueType::Nil) => Some(rhs_type),
                (ValueType::Boolean(None), _) => {
                    Some(ValueType::union(ValueType::Boolean(Some(false)), rhs_type.clone()))
                },
                _ => Some(rhs_type), // unreachable after opaque unwrap, satisfies exhaustiveness
            }
        },
        Operator::LessThan | Operator::GreaterThan | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals => {
            fn can_ordered_cmp(lhs: &ValueType, rhs: &ValueType) -> bool {
                match (lhs, rhs) {
                    (ValueType::Number, ValueType::Number) => true,
                    (ValueType::String(_), ValueType::String(_)) => true,
                    (ValueType::Any, _) | (_, ValueType::Any) => true,
                    (ValueType::TypeVariable(_), _) | (_, ValueType::TypeVariable(_)) => true,
                    (ValueType::Table(_), _) | (_, ValueType::Table(_)) => true,
                    (ValueType::Userdata, _) | (_, ValueType::Userdata) => true,
                    (ValueType::Union(types), _) => types.iter().all(|t| can_ordered_cmp(t, rhs)),
                    (_, ValueType::Union(types)) => types.iter().all(|t| can_ordered_cmp(lhs, t)),
                    (ValueType::Intersection(types), _) => types.iter().any(|t| can_ordered_cmp(t, rhs)),
                    (_, ValueType::Intersection(types)) => types.iter().any(|t| can_ordered_cmp(lhs, t)),
                    (ValueType::OpaqueAlias(_, inner), _) => can_ordered_cmp(inner, rhs),
                    (_, ValueType::OpaqueAlias(_, inner)) => can_ordered_cmp(lhs, inner),
                    _ => false,
                }
            }
            if can_ordered_cmp(&lhs_type, &rhs_type) {
                Some(ValueType::Boolean(None))
            } else {
                None
            }
        }
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

