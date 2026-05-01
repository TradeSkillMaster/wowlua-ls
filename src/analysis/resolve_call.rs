use std::collections::{HashMap, HashSet};

use crate::types::*;
use super::Analysis;

pub(super) struct CallSiteInfo {
    pub(super) is_method_call: bool,
}

// ── Function call resolution ──────────────────────────────────────────────────

impl<'a> Analysis<'a> {
    pub(super) fn resolve_function_call(
        &mut self,
        expr_id: ExprId,
        func: &ExprId,
        args: &[ExprId],
        arg_ranges: &[(u32, u32)],
        ret_index: &usize,
        call_site: CallSiteInfo,
    ) -> Option<ValueType> {
        let func_expr_id = *func;
        let arg_ranges = arg_ranges.to_vec();
        let CallSiteInfo { is_method_call, .. } = call_site;
        // Resolve the function expression to get its type
        let func_type = self.resolve_expr(func_expr_id)?;
        let mut constructor_table_idx: Option<TableIndex> = None;
        let mut call_func_table_idx: Option<TableIndex> = None;
        let func_idx = match func_type {
            ValueType::Function(Some(idx)) => idx,
            ValueType::Table(Some(table_idx)) => {
                if let Some(fi) = self.table(table_idx).call_func {
                    call_func_table_idx = Some(table_idx);
                    fi
                } else if let Some(fi) = self.resolve_constructor_func(table_idx) {
                    // @constructor: use the named method for arg checking
                    constructor_table_idx = Some(table_idx);
                    fi
                } else {
                    return None;
                }
            }
            ValueType::Union(ref types) | ValueType::Intersection(ref types) => {
                let func_from_composite = types.iter().find_map(|t| match t {
                    ValueType::Function(Some(idx)) => Some(*idx),
                    _ => None,
                });
                func_from_composite?
            }
            _ => return None,
        };


        // setmetatable / getmetatable: metatable type inference
        if *ret_index == 0 {
            if let Some(smt_idx) = self.ir.ext.setmetatable_func_idx
                && func_idx == smt_idx {
                    return self.resolve_setmetatable(args);
                }
            if let Some(gmt_idx) = self.ir.ext.getmetatable_func_idx
                && func_idx == gmt_idx {
                    return self.resolve_getmetatable(args);
                }
        }

        let ret_index = self.func(func_idx).effective_return_index(*ret_index);

        // Extract scalar fields without cloning the full Function struct
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
        let self_offset = if ((call_func_table_idx.is_some() || constructor_table_idx.is_some()) && has_self)
            || (is_method_call && (has_self || !func_args.is_empty())) { 1 } else { 0 };

        // If the callee has `@param ... params<F>` and F is
        // bound via the receiver's `@type X<fun(...)>`, the vararg
        // slot expands to F's param list — per-slot type-mismatch
        // checks use F's args.
        let projected_f_idx: Option<FunctionIndex> = {
            let proj_name: Option<String> = match &self.func(func_idx).vararg_projection {
                Some(crate::types::ProjectionKind::Params(n)) => Some(n.clone()),
                _ => None,
            };
            if let Some(proj_name) = proj_name {
                if is_method_call {
                    let callee_expr = *func;
                    let param0 = param_annotations.first().cloned();
                    if let Some(crate::annotations::AnnotationType::Parameterized(_, type_arg_anns)) = param0 {
                        if let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(callee_expr).clone() {
                            let receiver_type_args = self.get_expr_type_args(receiver_expr);
                            if receiver_type_args.len() == type_arg_anns.len() {
                                let mut found = None;
                                for (pos, type_arg_ann) in type_arg_anns.iter().enumerate() {
                                    if let crate::annotations::AnnotationType::Simple(gname) = type_arg_ann
                                        && *gname == proj_name
                                            && let Some(ValueType::Function(Some(f_idx))) = receiver_type_args.get(pos) {
                                                found = Some(*f_idx);
                                                break;
                                            }
                                }
                                found
                            } else { None }
                        } else { None }
                    } else { None }
                } else { None }
            } else { None }
        };


        // Propagate callee's fun() param annotation types into inline function params
        for (i, arg_expr_id) in args.iter().enumerate() {
            // Check if this argument is an inline function definition
            let inline_func_idx = match self.ir.expr(*arg_expr_id) {
                Expr::FunctionDef(idx) => *idx,
                _ => continue,
            };
            if inline_func_idx.is_external() { continue; }
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
            let inline_args = self.ir.functions[inline_func_idx.val()].args.clone();
            for (j, param_info) in sig.params.iter().enumerate() {
                let Some(&inline_sym_idx) = inline_args.get(j) else { continue };
                if inline_sym_idx.is_external() { continue; }
                if self.ir.symbols[inline_sym_idx.val()].versions.first()
                    .is_some_and(|v| v.resolved_type.is_some()) { continue; }
                if let Some(vt) = self.resolve_annotation_type(&param_info.typ) {
                    let vt = if param_info.optional {
                        ValueType::union(vt, ValueType::Nil)
                    } else {
                        vt
                    };
                    self.ir.symbols[inline_sym_idx.val()].versions[0].resolved_type = Some(vt);
                }
            }
            // Propagate return types from fun() signature into inline function
            if self.ir.functions[inline_func_idx.val()].return_annotations.is_empty() {
                if sig.returns.is_empty() {
                    // fun() with no return type — mark as explicitly void
                    self.ir.functions[inline_func_idx.val()].explicit_void_return = true;
                } else {
                    let mut return_vts = Vec::new();
                    for ret_annotation in &sig.returns {
                        if let Some(vt) = self.resolve_annotation_type(ret_annotation) {
                            return_vts.push(vt);
                        }
                    }
                    if !return_vts.is_empty() {
                        self.ir.functions[inline_func_idx.val()].return_annotations = return_vts;
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
        let mut substitutable_generic_names: HashSet<String> = HashSet::new();
        if !generics.is_empty() {
            let generic_names: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
            // Receiver binding runs FIRST so that class-generic parameters
            // (e.g. `@class C<T>` + `@param self C<T>` + `@param v T`) are
            // bound from the receiver's `@type C<X>` declaration rather than
            // from the arg's runtime type. The arg-binding loop below then
            // skips names already in `generic_subs`.
            if is_method_call {
                for (name, concrete) in self.bind_receiver_type_args(func_idx, *func) {
                    generic_subs.entry(name.clone()).or_insert_with(|| {
                        substitutable_generic_names.insert(name);
                        concrete
                    });
                }
            }
            if let Some(cf_table_idx) = call_func_table_idx {
                let class_type_params = self.table(cf_table_idx).class_type_params.clone();
                if !class_type_params.is_empty() {
                    let type_args = self.get_expr_type_args(func_expr_id);
                    for (pos, param_name) in class_type_params.iter().enumerate() {
                        if generic_names.contains(param_name) && !generic_subs.contains_key(param_name)
                            && let Some(concrete) = type_args.get(pos) {
                                generic_subs.insert(param_name.clone(), concrete.clone());
                                substitutable_generic_names.insert(param_name.clone());
                            }
                    }
                }
            }
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
                            // For backtick params (`T` or unions containing `T`), resolve the string literal to a type
                            let inferred = if param_annotations.get(i + self_offset).is_some_and(crate::annotations::annotation_contains_backtick) {
                                if let Some(class_name) = self.ir.string_literals.get(arg_expr_id) {
                                    self.ir.classes.get(class_name).copied()
                                        .or_else(|| self.ir.ext.classes.get(class_name).copied())
                                        .map(|idx| ValueType::Table(Some(idx)))
                                        .or_else(|| crate::annotations::resolve_primitive_type_name(class_name))
                                        .unwrap_or(ValueType::Any)
                                } else {
                                    arg_type.clone()
                                }
                            } else {
                                arg_type.clone()
                            };
                            // Backtick-bound primitives (string, number, boolean)
                            // are safe for sibling substitution. Class types are
                            // promotional — the arg is the definition, not an instance.
                            if !matches!(&inferred, ValueType::Table(Some(_))) {
                                substitutable_generic_names.insert(name.clone());
                            }
                            generic_subs.insert(name.clone(), inferred);
                            generic_arg_indices.insert(name.clone(), i);
                            }
                        }
                    } else if let Some(ValueType::Union(ref types)) = param_type {
                        // Optional params have type Union(TypeVariable("P"), Nil) —
                        // extract the TypeVariable to infer the generic, stripping nil.
                        // If the arg is literally nil, skip insertion so the constraint
                        // fallback applies (avoids false generic-constraint-mismatch).
                        //
                        // When the raw annotation is `(fun(): T) | T`, prefer structural
                        // inference: matching the arg against the `fun(): T` member gives
                        // us T from its return, instead of binding T = Function.
                        let has_fun_member = param_annotations.get(i + self_offset)
                            .is_some_and(|ann| match ann {
                                crate::annotations::AnnotationType::Union(members) =>
                                    members.iter().any(|m| matches!(m, crate::annotations::AnnotationType::Fun(..))),
                                _ => false,
                            });
                        if has_fun_member
                            && let Some(annotation) = param_annotations.get(i + self_offset).cloned() {
                                self.infer_generics_from_annotation(&annotation, &generic_names, &generics, &defclass, *arg_expr_id, &mut generic_subs);
                            }
                        if let Some(name) = types.iter().find_map(|t| match t {
                            ValueType::TypeVariable(n) => Some(n),
                            _ => None,
                        })
                            && !generic_subs.contains_key(name) {
                                let stripped = arg_type.strip_nil();
                                let is_nil_like = matches!(&stripped, ValueType::Nil) || matches!(&stripped, ValueType::Union(t) if t.is_empty());
                                if !is_nil_like {
                                    // Check if any member of the param annotation is a Backtick type —
                                    // if so, try to resolve a string literal argument as a type.
                                    let inferred = if let Some(annotation) = param_annotations.get(i + self_offset) {
                                        if crate::annotations::annotation_contains_backtick(annotation) {
                                            if let Some(class_name) = self.ir.string_literals.get(arg_expr_id) {
                                                self.ir.classes.get(class_name).copied()
                                                    .or_else(|| self.ir.ext.classes.get(class_name).copied())
                                                    .map(|idx| ValueType::Table(Some(idx)))
                                                    .or_else(|| crate::annotations::resolve_primitive_type_name(class_name))
                                                    .unwrap_or(ValueType::Any)
                                            } else {
                                                stripped
                                            }
                                        } else {
                                            stripped
                                        }
                                    } else {
                                        stripped
                                    };
                                    generic_subs.insert(name.clone(), inferred.clone());
                                    generic_arg_indices.insert(name.clone(), i);
                                    if !matches!(&inferred, ValueType::Table(Some(_))) {
                                        substitutable_generic_names.insert(name.clone());
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
                                substitutable_generic_names.insert(name.clone());
                            }
                            generic_arg_indices.entry(name.clone()).or_insert(i);
                        }
                    }
                }
            }
            // Receiver binding now runs before the arg-inference loop above.

            // Fallback: for any generic not inferred, use its constraint type
            for (name, constraint) in &generics {
                if !generic_subs.contains_key(name)
                    && let Some(ct) = constraint {
                        generic_subs.insert(name.clone(), ct.clone());
                    }
            }
        }

        // Find the matching overload (if any) — used for both diagnostics and return type.
        // Skip return-only overloads (from tuple-union `@return` cases) which only affect narrowing.
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
                                    // Nil against a non-optional param is always a mismatch
                                    if !param.optional && matches!(arg_t, ValueType::Nil) {
                                        return true;
                                    }
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
                                // Nil against a non-optional param is always a mismatch
                                if !param.optional && matches!(arg_t, ValueType::Nil) {
                                    return true;
                                }
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
        if has_generics
            && let Some(overload) = matching_overload {
                let generic_names: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
                for (i, arg_expr_id) in args.iter().enumerate() {
                    let Some(arg_type) = self.resolve_expr(*arg_expr_id) else { continue };
                    let param_type = overload.params.get(i + overload_self_offset)
                        .and_then(|p| p.typ.as_ref());
                    let Some(param_type) = param_type else { continue };
                    // Direct TypeVariable: T → infer T = arg_type
                    if let ValueType::TypeVariable(name) = param_type
                        && generic_names.contains(name) && !generic_subs.contains_key(name) {
                            generic_subs.insert(name.clone(), arg_type.clone());
                            generic_arg_indices.insert(name.clone(), i);
                            substitutable_generic_names.insert(name.clone());
                        }
                    // Table with TypeVariable value_type: T[] → infer T from array elements
                    if let ValueType::Table(Some(idx)) = param_type {
                        let vt_name = self.table(*idx).value_type.clone();
                        if let Some(ValueType::TypeVariable(name)) = &vt_name
                            && generic_names.contains(name) && !generic_subs.contains_key(name)
                                && let Some(elem_type) = self.infer_array_element_type(*arg_expr_id) {
                                    generic_subs.insert(name.clone(), elem_type);
                                    generic_arg_indices.entry(name.clone()).or_insert(i);
                                    substitutable_generic_names.insert(name.clone());
                                }
                    }
                }
            }

        // Defer type-mismatch / need-check-nil diagnostics to post-resolution.
        // Resolve each arg so side effects (e.g. undefined-field checks on
        // FieldAccess expressions) are triggered during the fixpoint loop.
        let mut resolved_call_args: Vec<ResolvedCallArg> = Vec::new();
        for (i, arg_expr_id) in args.iter().enumerate() {
            self.resolve_expr(*arg_expr_id);
            let skip_if_nil = matching_overload.and_then(|o| o.params.get(i + overload_self_offset))
                .is_some_and(|p| p.optional);
            // Compute expected parameter type
            let expected_type = if let Some(overload) = matching_overload {
                overload.params.get(i + overload_self_offset).and_then(|p| p.typ.clone())
            } else if let Some(&param_sym_idx) = func_args.get(i + self_offset) {
                self.sym(param_sym_idx).versions.first()
                    .and_then(|ver| ver.resolved_type.clone())
            } else if let Some(f_idx) = projected_f_idx {
                let non_vararg_count = func_args.len() - self_offset;
                i.checked_sub(non_vararg_count).and_then(|pos| {
                    let f_arg_sym = *self.func(f_idx).args.get(pos)?;
                    self.sym(f_arg_sym).versions.first()
                        .and_then(|ver| ver.resolved_type.clone())
                })
            } else {
                None
            };
            let Some(expected_type) = expected_type else { continue };
            // Apply generic substitutions
            let expected_type = if !substitutable_generic_names.is_empty() {
                let structural_subs: HashMap<String, ValueType> = generic_subs.iter()
                    .filter(|(k, _)| substitutable_generic_names.contains(k.as_str()))
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
            if matches!(expected_type, ValueType::TypeVariable(_)) { continue; }
            // Skip backtick params (type-name string literals)
            if matching_overload.is_none()
                && param_annotations.get(i + self_offset).is_some_and(crate::annotations::annotation_contains_backtick) {
                    continue;
                }
            let param_name: String = if let Some(overload) = matching_overload {
                overload.params.get(i + overload_self_offset).map(|p| p.name.clone()).unwrap_or_else(|| "?".to_string())
            } else if let Some(&param_sym_idx) = func_args.get(i + self_offset) {
                if let SymbolIdentifier::Name(n) = &self.sym(param_sym_idx).id { n.clone() } else { "?".to_string() }
            } else {
                "?".to_string()
            };
            let primary_param_type = if matching_overload.is_some() {
                func_args.get(i + self_offset).and_then(|&sym_idx| {
                    let sym = self.sym(sym_idx);
                    let name_matches = if let SymbolIdentifier::Name(n) = &sym.id {
                        *n == param_name
                    } else {
                        false
                    };
                    if name_matches {
                        sym.versions.first().and_then(|ver| ver.resolved_type.clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            };
            if let Some(&(start, end)) = arg_ranges.get(i) {
                resolved_call_args.push(ResolvedCallArg {
                    expected_type, arg_expr: *arg_expr_id, param_name,
                    skip_if_nil, primary_param_type, start, end,
                });
            }
        }

        // Build per-call generic bindings with arg source ranges
        if ret_index == 0 {
            let generic_subs_ir: Vec<GenericBinding> = generic_subs.iter()
                .map(|(name, vt)| {
                    let arg_range = generic_arg_indices.get(name)
                        .and_then(|&idx| arg_ranges.get(idx).copied());
                    (name.clone(), vt.clone(), arg_range)
                })
                .collect();
            self.ir.call_resolutions.insert(expr_id, CallResolution {
                func_idx,
                expected_args: resolved_call_args,
                generic_subs: generic_subs_ir,
                projected_f_idx: None,
                is_expansion: false,
                first_arg_range: arg_ranges.first().copied(),
            });
        }

        // @constructor: return the class table type
        if let Some(ctor_table_idx) = constructor_table_idx {
            return if ret_index == 0 {
                Some(ValueType::Table(Some(ctor_table_idx)))
            } else {
                None
            };
        }

        // @return self: resolve receiver type for method calls
        if returns_self && ret_index == 0 {
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
                        let new_idx = if let Some(&memo) = self.builder_call_memo.get(&expr_id) {
                            memo
                        } else {
                            let resolved_field_vt = if !generic_subs.is_empty() {
                                self.substitute_generics_deep(&field_vt, &generic_subs)
                            } else {
                                field_vt
                            };
                            let new_idx = self.clone_table_with_built_field(*recv_idx, &name, resolved_field_vt, field_lateinit);
                            self.builder_call_memo.insert(expr_id, new_idx);
                            new_idx
                        };
                        return Some(ValueType::Table(Some(new_idx)));
                    }
                }
                // @built-name: set the built_table's class_name from a string literal argument
                if let (Some(param_idx), ValueType::Table(Some(recv_idx))) = (built_name_param, &rt) {
                    let class_name = args.get(param_idx - 1)
                        .and_then(|&arg_expr| self.ir.string_literals.get(&arg_expr))
                        .cloned();
                    if let Some(name) = class_name {
                        let new_idx = if let Some(&memo) = self.builder_call_memo.get(&expr_id) {
                            memo
                        } else {
                            let new_idx = self.clone_table_with_built_name(*recv_idx, &name, built_extends);
                            self.builder_call_memo.insert(expr_id, new_idx);
                            new_idx
                        };
                        return Some(ValueType::Table(Some(new_idx)));
                    }
                }
                return Some(rt);
            }
        }

        // @return built: return the accumulated built_table
        if self.func(func_idx).returns_built && ret_index == 0 {
            let returns_built_parent = self.func(func_idx).returns_built_parent.clone();
            let receiver_type = if let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(*func).clone() {
                self.resolve_expr(receiver_expr)
            } else {
                None
            };
            if let Some(ValueType::Table(Some(recv_idx))) = receiver_type {
                if let Some(built_idx) = self.table(recv_idx).built_table {
                    // Optionally add parent class to the built table
                    if let Some(parent_name) = returns_built_parent
                        && let Some(&parent_idx) = self.ir.classes.get(&parent_name)
                            .or_else(|| self.ir.ext.classes.get(&parent_name))
                            && !self.table(built_idx).parent_classes.contains(&parent_idx) {
                                self.ir_mut_table(built_idx).parent_classes.push(parent_idx);
                            }
                    return Some(ValueType::Table(Some(built_idx)));
                }
                // No built fields accumulated — return empty table
                return Some(ValueType::Table(None));
            }
        }

        // Pick the matching overload signature for return types

        // Gap 4: if this return slot carries a `returns<F>` projection
        // and F is bound to a concrete `Function(Some(f_idx))`, the
        // resolved return type is F's return at this ret_index.
        // When only one return annotation exists with a projection at
        // index 0, higher ret_indices expand into F's returns (e.g.
        // `@overload fun(): returns<F>` where F returns multiple values).
        let is_expansion = ret_index > 0
            && self.func(func_idx).return_annotations.len() <= 1
            && !self.func(func_idx).return_projections.contains_key(&ret_index);
        let proj = if is_expansion {
            self.func(func_idx).return_projections.get(&0).cloned()
        } else {
            self.func(func_idx).return_projections.get(&ret_index).cloned()
        };
        if let Some(proj) = proj
            && let crate::types::ProjectionKind::Return(ref name) = proj
                && let Some(bound) = generic_subs.get(name).cloned()
                    && let ValueType::Function(Some(f_idx)) = bound {
                        let f_returns = self.func(f_idx).return_annotations.clone();
                        let f_has_vararg = self.func(f_idx).has_vararg_return;
                        let vt = f_returns.get(ret_index).cloned()
                            .or_else(|| {
                                if f_has_vararg && !f_returns.is_empty() {
                                    f_returns.last().cloned()
                                } else if f_returns.is_empty() {
                                    let f_scope = self.func(f_idx).scope;
                                    let ret_id = SymbolIdentifier::FunctionRet(f_idx, ret_index);
                                    self.get_symbol(&ret_id, f_scope)
                                        .and_then(|si| self.sym(si).versions.first()
                                            .and_then(|v| v.resolved_type.clone()))
                                } else { None }
                            })
                            .unwrap_or(ValueType::Nil);
                        if let Some(cr) = self.ir.call_resolutions.get_mut(&expr_id) {
                            cr.projected_f_idx = Some(f_idx);
                            cr.is_expansion = is_expansion;
                        }
                        return Some(vt);
                    }

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
            // Backtick return: `@return \`K\`` returns the type name as a string literal
            if let Some(raw_ret) = self.func(func_idx).return_annotations_raw.get(ret_index)
                && let crate::annotations::AnnotationType::Backtick(inner) = raw_ret
                    && let crate::annotations::AnnotationType::Simple(name) = inner.as_ref()
                        && let Some(bound_type) = generic_subs.get(name)
                            && let Some(type_name) = crate::annotations::value_type_to_name(bound_type, &self.ir) {
                                return Some(ValueType::String(Some(type_name)));
                            }
            if let Some(ret_vt) = return_annotations.get(ret_index) {
                let substituted = self.substitute_generics_deep(ret_vt, &generic_subs);
                if !matches!(substituted, ValueType::TypeVariable(_)) {
                    // If the raw return annotation is `Parameterized("C", [..])`,
                    // compute the substituted type_args and cache them under this
                    // call's ExprId. This lets `get_expr_type_args` return the
                    // concrete type arguments so subsequent method calls on the
                    // receiver can re-substitute T via the receiver-type_args path.
                    if ret_index == 0 {
                        let raw_ret = self.func(func_idx).return_annotations_raw
                            .get(ret_index).cloned();
                        if let Some(crate::annotations::AnnotationType::Parameterized(_, type_arg_anns)) = raw_ret {
                            // Pass the function's own generic names so that
                            // `Simple("T")` resolves to `TypeVariable("T")`,
                            // which `substitute_generics_deep` can then replace.
                            let fn_generics = self.func(func_idx)
                                .generic_constraints_raw.clone();
                            let mut substituted_args: Vec<ValueType> = type_arg_anns.iter()
                                .map(|ta| {
                                    self.resolve_annotation_type_mut_gen(ta, &fn_generics)
                                        .unwrap_or(ValueType::Any)
                                })
                                .collect();
                            for arg in &mut substituted_args {
                                *arg = self.substitute_generics_deep(arg, &generic_subs);
                            }
                            if !substituted_args.is_empty() {
                                self.call_type_args.insert(expr_id, substituted_args);
                            }
                        }
                    }
                    if return_overloads_may_nil && !substituted.contains_nil() && !matches!(substituted, ValueType::Any) {
                        return Some(ValueType::make_union(vec![substituted, ValueType::Nil]));
                    }
                    return Some(substituted);
                }
            }
        }

        // For non-generic functions returning parameterized types (e.g.
        // `@return IteratorObject<fun(): number, string>`), populate
        // call_type_args so downstream ForInVar / method-call resolution
        // can access the concrete type arguments.
        if generic_subs.is_empty() && ret_index == 0 {
            let raw_ret = self.func(func_idx).return_annotations_raw
                .get(ret_index).cloned();
            if let Some(crate::annotations::AnnotationType::Parameterized(_, type_arg_anns)) = raw_ret {
                let substituted_args: Vec<ValueType> = type_arg_anns.iter()
                    .map(|ta| {
                        self.resolve_annotation_type_mut_gen(ta, &[])
                            .unwrap_or(ValueType::Any)
                    })
                    .collect();
                if !substituted_args.is_empty() {
                    self.call_type_args.insert(expr_id, substituted_args);
                }
            }
        }

        // Non-overload: union the resolved types of every return statement
        // at this slot.
        // Synthesized correlated return-only overloads (from
        // `inference.correlated_return_overloads`) encode types for ALL return
        // statements. When such overloads are present and there are no
        // `@return` annotations, prefer them — they're already deduped and
        // carry across-slot correlation that a per-slot scan can't reproduce.
        //
        // (Use `self.func(func_idx).return_annotations` directly because the
        // local `return_annotations` is only cloned when the function has
        // generics — see line ~1249.)
        let synthesized_return_only = self.func(func_idx).return_annotations.is_empty()
            && self.func(func_idx).overloads.iter().any(|o| o.is_return_only);
        let ret_type = if synthesized_return_only {
            // Use `return_type_at` so `has_vararg_tail` cases fall
            // through to the vararg element type. (Today this branch
            // only fires when `return_annotations.is_empty()`, which
            // tuple-union never produces — but keeping the lookup
            // symmetric with `resolve_overload_narrow` avoids a
            // footgun if that invariant ever changes.)
            let return_only_types: Vec<ValueType> = self.func(func_idx).overloads.iter()
                .filter(|o| o.is_return_only)
                .map(|o| o.return_type_at(ret_index))
                .collect();
            if return_only_types.is_empty() {
                return None;
            }
            Some(ValueType::make_union(return_only_types))
        } else {
            // Walk every `FunctionRet` symbol in `func.rets` rather than
            // looking up just the body-scope one. Each `return` registers
            // its symbol at its own scope (if/else/for/while/...), and a
            // body-scope-only lookup loses both pure-branched returns
            // (no body-scope return at all) and the branched contributions
            // to mixed body+branched returns.
            super::queries::return_type_at_slot(
                &self.ir,
                &self.func(func_idx).rets,
                ret_index,
            )
        };
        // Implicit nil return: a bare `return` statement or fall-through
        // from the end of the function body contributes nil at every
        // return slot. When there are no `@return` annotations and no
        // synthesized return-only overloads, union nil into the inferred
        // type. If the resolved type is unknown (None), leave it alone —
        // we don't have enough signal to decide.
        let ret_type = if !synthesized_return_only
            && self.func(func_idx).return_annotations.is_empty()
            && self.func(func_idx).implicit_nil_return
        {
            match ret_type {
                // Only bare returns / fall-through, no typed returns: nil.
                None if self.func(func_idx).rets.is_empty() => Some(ValueType::Nil),
                // `make_union` preserves `Any | Nil` (it's how optionality
                // is tracked) so keep Any as-is rather than expanding to
                // `any | nil` on hover.
                Some(ValueType::Any) => Some(ValueType::Any),
                Some(t) if t.contains_nil() => Some(t),
                Some(t) => Some(ValueType::make_union(vec![t, ValueType::Nil])),
                None => None,
            }
        } else {
            ret_type
        };
        // If we still have no ret_type, there's no meaningful inference to make.
        ret_type.as_ref()?;
        // If this function has generics and the return type is still a
        // TypeVariable, don't return it — keep unresolved so a later
        // fixpoint pass can substitute the concrete type.
        if !generics.is_empty()
            && let Some(ref vt) = ret_type
                && vt.contains_type_variable() {
                    return None;
                }
        // @built-name: if this function has @built-name, set the built_table's class_name
        // on the returned table from the specified string literal argument
        if let Some(built_name_idx) = self.func(func_idx).built_name
            && ret_index == 0
                && let Some(ValueType::Table(Some(table_idx))) = &ret_type {
                    let class_name = args.get(built_name_idx - 1)
                        .and_then(|&arg_expr| self.ir.string_literals.get(&arg_expr))
                        .cloned();
                    if let Some(name) = class_name {
                        let new_idx = if let Some(&memo) = self.builder_call_memo.get(&expr_id) {
                            memo
                        } else {
                            let extends = self.func(func_idx).built_extends;
                            let new_idx = self.clone_table_with_built_name(*table_idx, &name, extends);
                            self.builder_call_memo.insert(expr_id, new_idx);
                            new_idx
                        };
                        return Some(ValueType::Table(Some(new_idx)));
                    }
                }
        // Propagate @built-name through wrapper functions: if this function returns
        // a class table whose __init method has @built-name, apply it using this
        // call's arguments.
        if self.func(func_idx).built_name.is_none() && ret_index == 0
            && let Some(ValueType::Table(Some(table_idx))) = &ret_type {
                let init_built_name = self.table(*table_idx).fields.get("__init")
                    .map(|f| f.expr)
                    .and_then(|eid| {
                        if let Expr::FunctionDef(fi) = self.expr(eid) {
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
                        let new_idx = if let Some(&memo) = self.builder_call_memo.get(&expr_id) {
                            memo
                        } else {
                            let new_idx = self.clone_table_with_built_name(*table_idx, &name, false);
                            self.builder_call_memo.insert(expr_id, new_idx);
                            new_idx
                        };
                        return Some(ValueType::Table(Some(new_idx)));
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

    // ── Metatable helpers ─────────────────────────────────────────────────────

    fn resolve_setmetatable(&mut self, args: &[ExprId]) -> Option<ValueType> {
        let tbl_expr = args.first()?;
        let tbl_type = self.resolve_expr(*tbl_expr);

        // If the first argument isn't a resolved table, return None so fixpoint retries
        let tbl_idx = match tbl_type {
            Some(ValueType::Table(Some(idx))) => idx,
            _ => return None,
        };

        // Can only mutate local tables (not external)
        if tbl_idx.is_external() {
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
        self.ir.tables[tbl_idx.val()].metatable = Some(mt_idx);

        // Resolve __index on the metatable once; use the result for both
        // metatable_index and class_name propagation fallbacks below.
        let index_resolved = self.resolve_metatable_index_expr(mt_idx);

        // Case 1: __index resolved to a table directly (table ref or function with @return)
        if let Some(index_idx) = index_resolved.as_ref().and_then(|vt| self.extract_table_from_type(vt)) {
            self.ir.tables[tbl_idx.val()].metatable_index = Some(index_idx);
            if self.ir.tables[tbl_idx.val()].class_name.is_none()
                && let Some(name) = self.table(index_idx).class_name.clone() {
                    self.ir.tables[tbl_idx.val()].class_name = Some(name);
                }
        }

        // Case 2: propagate class_name from the metatable itself.
        if self.ir.tables[tbl_idx.val()].class_name.is_none()
            && let Some(name) = self.table(mt_idx).class_name.clone() {
                self.ir.tables[tbl_idx.val()].class_name = Some(name);
            }

        // Case 3: when __index is a function without @return annotations,
        // scan its return expressions for bracket/field accesses on class-typed
        // tables.
        if self.ir.tables[tbl_idx.val()].class_name.is_none()
            && let Some(class_idx) = self.find_class_in_index_function(&index_resolved) {
                let name = self.table(class_idx).class_name.clone();
                self.ir.tables[tbl_idx.val()].class_name = name;
                if self.ir.tables[tbl_idx.val()].metatable_index.is_none() {
                    self.ir.tables[tbl_idx.val()].metatable_index = Some(class_idx);
                }
            }

        // Resolve __call on the metatable and set call_func on the table
        if self.ir.tables[tbl_idx.val()].call_func.is_none()
            && let Some(func_idx) = self.resolve_metatable_call_func(mt_idx) {
                self.ir.tables[tbl_idx.val()].call_func = Some(func_idx);
                // Propagate the table type to __call's first parameter ("self")
                // so that body expressions like self.field can resolve.
                if let Some(&self_sym) = self.func(func_idx).args.first()
                    && !self_sym.is_external()
                    && matches!(&self.sym(self_sym).id, SymbolIdentifier::Name(n) if n == "self")
                    && self.sym(self_sym).versions.first()
                        .is_some_and(|v| v.resolved_type.is_none())
                {
                    self.ir.symbols[self_sym.val()].versions[0].resolved_type =
                        Some(ValueType::Table(Some(tbl_idx)));
                }
            }

        Some(ValueType::Table(Some(tbl_idx)))
    }

    fn resolve_getmetatable(&mut self, args: &[ExprId]) -> Option<ValueType> {
        let tbl_expr = args.first()?;
        let tbl_type = self.resolve_expr(*tbl_expr)?;
        let tbl_idx = match tbl_type {
            ValueType::Table(Some(idx)) => idx,
            _ => return None,
        };
        match self.table(tbl_idx).metatable {
            Some(mt_idx) => Some(ValueType::Table(Some(mt_idx))),
            None => Some(ValueType::Table(None)),
        }
    }

    fn resolve_metatable_index_expr(&mut self, mt_idx: TableIndex) -> Option<ValueType> {
        let fi = self.ir.get_field(mt_idx, "__index")?;
        let expr = fi.expr;
        self.resolve_expr(expr)
    }

    fn resolve_metatable_call_func(&mut self, mt_idx: TableIndex) -> Option<FunctionIndex> {
        let fi = self.ir.get_field(mt_idx, "__call")?;
        let expr = fi.expr;
        let resolved = self.resolve_expr(expr)?;
        match resolved {
            ValueType::Function(Some(idx)) => Some(idx),
            ValueType::Union(ref types) | ValueType::Intersection(ref types) => {
                types.iter().find_map(|t| match t {
                    ValueType::Function(Some(idx)) => Some(*idx),
                    _ => None,
                })
            }
            _ => None,
        }
    }

    fn find_class_in_index_function(&mut self, index_resolved: &Option<ValueType>) -> Option<TableIndex> {
        let func_idx = match index_resolved {
            Some(ValueType::Function(Some(idx))) => *idx,
            _ => return None,
        };
        if !self.func(func_idx).return_annotations.is_empty() {
            return None;
        }
        let rets: Vec<SymbolIndex> = self.func(func_idx).rets.clone();
        for ret_sym_idx in rets {
            let type_source = self.ir.symbols.get(ret_sym_idx.val())
                .and_then(|s| s.versions.last())
                .and_then(|v| v.type_source);
            let expr_id = match type_source {
                Some(id) => id,
                None => continue,
            };
            let expr = match self.ir.exprs.get(expr_id.val()) {
                Some(e) => e.clone(),
                None => continue,
            };
            let base_expr = match &expr {
                Expr::BracketIndex { table, .. } => Some(*table),
                Expr::FieldAccess { table, .. } => Some(*table),
                _ => None,
            };
            if let Some(base) = base_expr
                && let Some(base_type) = self.resolve_expr(base)
                    && let ValueType::Table(Some(idx)) = base_type
                        && self.table(idx).class_name.is_some() {
                            return Some(idx);
                        }
        }
        None
    }

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
                let ret = self.func(*func_idx).return_annotations.first()?;
                match ret {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn resolve_constructor_func(&self, table_idx: TableIndex) -> Option<FunctionIndex> {
        self.ir.resolve_constructor_func(table_idx)
    }

    // ── Builder pattern helpers ───────────────────────────────────────────────

    fn ir_mut_table(&mut self, idx: TableIndex) -> &mut TableInfo {
        &mut self.ir.tables[idx.val()]
    }

    const MAX_IR_TABLES: usize = 50_000;

    fn clone_table_with_built_field(&mut self, source_idx: TableIndex, field_name: &str, field_type: ValueType, lateinit: bool) -> TableIndex {
        if self.ir.tables.len() >= Self::MAX_IR_TABLES {
            if self.safety_limit_hit.is_none() {
                self.safety_limit_hit = Some(format!(
                    "builder chain exceeded table limit ({})", Self::MAX_IR_TABLES
                ));
            }
            return source_idx;
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

        let has_overlay = built_fields.get(field_name)
            .is_some_and(|f| f.annotation_type_raw.is_some());
        if !has_overlay {
            let dummy_expr = self.ir.push_expr(Expr::Literal(field_type.clone()));
            built_fields.insert(field_name.to_string(), crate::types::FieldInfo {
                expr: dummy_expr,
                extra_exprs: Vec::new(),
                visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                annotation: Some(field_type),
                annotation_text: None,
                annotation_type_raw: None,
                lateinit,
                def_range: None,
                flavor_guard: 0,
            });
        }

        let new_built_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo {
            fields: built_fields, class_name: built_class_name.clone(),
            parent_classes: built_parent_classes, ..Default::default()
        });

        if let Some(ref name) = built_class_name
            && self.ir.classes.contains_key(name) {
                self.ir.classes.insert(name.clone(), new_built_idx);
            }

        let new_schema_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo {
            fields: schema_fields, class_name, class_type_params,
            parent_classes, accessors, call_func,
            built_table: Some(new_built_idx), metatable_index, ..Default::default()
        });

        new_schema_idx
    }

    fn clone_table_with_built_name(&mut self, source_idx: TableIndex, class_name: &str, extends: bool) -> TableIndex {
        if self.ir.tables.len() >= Self::MAX_IR_TABLES {
            if self.safety_limit_hit.is_none() {
                self.safety_limit_hit = Some(format!(
                    "builder chain exceeded table limit ({})", Self::MAX_IR_TABLES
                ));
            }
            return source_idx;
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

        let (mut built_fields, built_parents) = if extends {
            let mut parents = Vec::new();
            if let Some(bt_idx) = existing_built {
                parents.push(bt_idx);
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

        let mut final_parents = built_parents;
        if final_parents.is_empty()
            && let Some(&old_idx) = self.ir.classes.get(class_name) {
                let old_parents = &self.table(old_idx).parent_classes;
                if !old_parents.is_empty() {
                    final_parents = old_parents.clone();
                }
            }

        let mut overlay_correlated = Vec::new();
        if let Some(&overlay_idx) = self.ir.classes.get(class_name)
            && !overlay_idx.is_external() {
                let overlay_fields: Vec<(String, FieldInfo)> = self.ir.tables[overlay_idx.val()].fields.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (fname, fi) in overlay_fields {
                    built_fields.insert(fname, fi);
                }
                overlay_correlated = self.ir.tables[overlay_idx.val()].correlated_groups.clone();
            }

        let new_built_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo {
            fields: built_fields, class_name: Some(class_name.to_string()),
            parent_classes: final_parents, correlated_groups: overlay_correlated,
            ..Default::default()
        });

        self.ir.classes.insert(class_name.to_string(), new_built_idx);

        let new_schema_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo {
            fields: schema_fields, class_name: schema_class_name,
            class_type_params, parent_classes, accessors, call_func,
            built_table: Some(new_built_idx), metatable_index, ..Default::default()
        });

        new_schema_idx
    }

    // ── Generic type arg helpers ──────────────────────────────────────────────

    fn bind_receiver_type_args(
        &mut self,
        func_idx: FunctionIndex,
        func_expr: ExprId,
    ) -> std::collections::HashMap<String, ValueType> {
        use std::collections::HashMap;
        let param_anns = self.ir.func(func_idx).param_annotations.clone();
        let Some(crate::annotations::AnnotationType::Parameterized(_, type_arg_anns)) =
            param_anns.first()
        else {
            return HashMap::new();
        };
        let generic_names: Vec<String> = self.ir.func(func_idx)
            .generic_constraints_raw.iter()
            .map(|(n, _)| n.clone()).collect();
        let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(func_expr).clone() else {
            return HashMap::new();
        };
        let receiver_type_args = self.get_expr_type_args(receiver_expr);
        if receiver_type_args.len() != type_arg_anns.len() {
            return HashMap::new();
        }
        let mut subs = HashMap::new();
        for (pos, type_arg_ann) in type_arg_anns.iter().enumerate() {
            if let crate::annotations::AnnotationType::Simple(name) = type_arg_ann
                && generic_names.contains(name)
                    && let Some(concrete) = receiver_type_args.get(pos) {
                        subs.insert(name.clone(), concrete.clone());
                    }
        }
        subs
    }

    pub(super) fn get_expr_type_args(&mut self, expr_id: ExprId) -> Vec<ValueType> {
        if let Some(args) = self.call_type_args.get(&expr_id) {
            return args.clone();
        }
        let expr = self.expr(expr_id).clone();
        match expr {
            Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::Grouped(inner) => {
                self.get_expr_type_args(inner)
            }
            Expr::SymbolRef(sym_idx, ver) => {
                let sym = self.sym(sym_idx);
                if let Some(version) = sym.versions.get(ver) {
                    if !version.type_args.is_empty() {
                        return version.type_args.clone();
                    }
                    if let Some(src_expr) = version.type_source
                        && let Some(args) = self.call_type_args.get(&src_expr) {
                            return args.clone();
                        }
                }
                Vec::new()
            }
            Expr::FieldAccess { table, field, .. } => {
                if let Some(ValueType::Table(Some(table_idx))) = self.resolve_expr(table) {
                    if let Some(cached) = self.field_type_args_cache.get(&(table_idx, field.clone())) {
                        return cached.clone();
                    }
                    let fi_info = self.table(table_idx).fields.get(&field).map(|fi| {
                        (fi.annotation_type_raw.clone(), fi.expr, fi.extra_exprs.clone())
                    });
                    if let Some((raw_ann, field_expr, extra_exprs)) = fi_info {
                        if let Some(crate::annotations::AnnotationType::Parameterized(_, type_arg_anns)) = raw_ann {
                            let resolved: Vec<ValueType> = type_arg_anns.iter()
                                .filter_map(|ta| {
                                    let vt = self.resolve_annotation_type_mut_gen(ta, &[]);
                                    if matches!(&vt, Some(ValueType::Function(None)))
                                        && let crate::annotations::AnnotationType::Simple(name) = ta {
                                            let body = self.ir.alias_fun_types.get(name)
                                                .or_else(|| self.ir.ext.alias_fun_types.get(name))
                                                .cloned();
                                            if let Some(body) = body {
                                                return self.resolve_annotation_type_mut_gen(&body, &[]);
                                            }
                                        }
                                    vt
                                })
                                .collect();
                            self.field_type_args_cache.insert((table_idx, field), resolved.clone());
                            return resolved;
                        }
                        if let Some(args) = self.call_type_args.get(&field_expr) {
                            let args = args.clone();
                            self.field_type_args_cache.insert((table_idx, field), args.clone());
                            return args;
                        }
                        for extra in &extra_exprs {
                            if let Some(args) = self.call_type_args.get(extra) {
                                let args = args.clone();
                                self.field_type_args_cache.insert((table_idx, field), args.clone());
                                return args;
                            }
                        }
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    // ── Generic substitution ─────────────────────────────────────────────────

    pub(super) fn type_contains_type_variable_deep(&self, vt: &ValueType) -> bool {
        let mut visited: HashSet<TableIndex> = HashSet::new();
        self.type_contains_type_variable_deep_inner(vt, &mut visited)
    }

    fn type_contains_type_variable_deep_inner(
        &self,
        vt: &ValueType,
        visited: &mut HashSet<TableIndex>,
    ) -> bool {
        match vt {
            ValueType::TypeVariable(_) => true,
            ValueType::Union(types) | ValueType::Intersection(types) => {
                types.iter().any(|t| self.type_contains_type_variable_deep_inner(t, visited))
            }
            ValueType::Table(Some(idx)) => {
                if !visited.insert(*idx) { return false; }
                let t = self.table(*idx);
                if let Some(v) = &t.value_type
                    && self.type_contains_type_variable_deep_inner(v, visited) { return true; }
                if let Some(k) = &t.key_type
                    && self.type_contains_type_variable_deep_inner(k, visited) { return true; }
                t.fields.values().any(|fi| {
                    fi.annotation.as_ref().is_some_and(|a|
                        self.type_contains_type_variable_deep_inner(a, visited))
                })
            }
            _ => false,
        }
    }

    pub(super) fn substitute_generics_deep(&mut self, vt: &ValueType, subs: &HashMap<String, ValueType>) -> ValueType {
        match vt {
            ValueType::TypeVariable(name) => {
                subs.get(name).cloned().unwrap_or_else(|| vt.clone())
            }
            ValueType::Union(types) => {
                let subst: Vec<_> = types.iter()
                    .map(|t| self.substitute_generics_deep(t, subs))
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
                let has_tv = func.args.iter().any(|&sym_idx| {
                    self.sym(sym_idx).versions.iter()
                        .any(|v| v.resolved_type.as_ref().is_some_and(|t| t.contains_type_variable()))
                }) || func.return_annotations.iter().any(|vt| vt.contains_type_variable());
                if !has_tv {
                    return vt.clone();
                }
                let dummy_node = func.def_node;
                let is_vararg = func.is_vararg;
                let has_vararg_return_clone = func.has_vararg_return;
                let param_optional = func.param_optional.clone();
                let param_annotations = func.param_annotations.clone();
                let return_annotations = func.return_annotations.clone();
                let return_annotations_raw = func.return_annotations_raw.clone();
                let return_labels = func.return_labels.clone();
                let explicit_void_return = func.explicit_void_return;
                let implicit_nil_return = func.implicit_nil_return;
                let arg_infos: Vec<(SymbolIdentifier, Option<ValueType>)> = func.args.iter().map(|&sym_idx| {
                    let sym = self.sym(sym_idx);
                    let resolved = sym.versions.first().and_then(|v| v.resolved_type.clone());
                    (sym.id.clone(), resolved)
                }).collect();

                let func_scope = self.ir.insert_scope(None);
                let mut new_args = Vec::new();
                for (id, resolved) in &arg_infos {
                    let substituted = resolved.as_ref().map(|t| self.substitute_generics_deep(t, subs));
                    let sym_idx = SymbolIndex(self.ir.symbols.len());
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
                            original_type_source: None,
                        }],
                        flavor_guard: 0,
                    });
                    new_args.push(sym_idx);
                }

                let new_func_idx = FunctionIndex(self.ir.functions.len());
                let subst_return_annotations: Vec<ValueType> = return_annotations.iter()
                    .map(|t| self.substitute_generics_deep(t, subs))
                    .collect();
                let mut new_rets = Vec::new();
                for (i, ret_vt) in subst_return_annotations.iter().enumerate() {
                    let sym_idx = SymbolIndex(self.ir.symbols.len());
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
                            original_type_source: None,
                        }],
                        flavor_guard: 0,
                    });
                    new_rets.push(sym_idx);
                }

                self.ir.functions.push(Function {
                    def_node: dummy_node,
                    scope: func_scope,
                    args: new_args,
                    rets: new_rets,
                    return_annotations: subst_return_annotations,
                    return_annotations_raw,
                    return_labels,
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
                    implicit_nil_return,

                    constructor: false,
                    builds_field: None,
                    built_name: None,
                    built_extends: false,
                    returns_built: false,
                    returns_built_parent: None,
                    type_narrows: None,
                    type_narrows_class: None,
                    has_vararg_return: has_vararg_return_clone,
                    see: Vec::new(),
                    flavors: 0,
                    flavor_guard: 0,
                    return_projections: std::collections::HashMap::new(),
                    vararg_projection: None,
                });
                ValueType::Function(Some(new_func_idx))
            }
            ValueType::Table(Some(table_idx)) => {
                let table = self.table(*table_idx);
                let has_tv = table.value_type.as_ref().is_some_and(|t| t.contains_type_variable())
                    || table.key_type.as_ref().is_some_and(|t| t.contains_type_variable())
                    || table.fields.values().any(|fi| fi.annotation.as_ref().is_some_and(|t| t.contains_type_variable()));
                if !has_tv {
                    return vt.clone();
                }
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
                        flavor_guard: fi.flavor_guard,
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
                        flavor_guard: fi.flavor_guard,
                    })
                }).collect();
                let new_table_idx = TableIndex(self.ir.tables.len());
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

    // ── Backward inference ───────────────────────────────────────────────────

    pub(super) fn infer_backward_param_types(&mut self) -> bool {
        use crate::annotations::AnnotationType;
        use std::collections::HashSet;

        let mut candidates: HashSet<SymbolIndex> = HashSet::new();
        for func in &self.ir.functions {
            for (i, &sym_idx) in func.args.iter().enumerate() {
                if sym_idx.is_external() { continue; }
                if matches!(&self.ir.symbols[sym_idx.val()].id, SymbolIdentifier::Name(n) if n == "self") {
                    continue;
                }
                if let Some(ann) = func.param_annotations.get(i)
                    && !matches!(ann, AnnotationType::Simple(s) if s.is_empty()) {
                        continue;
                    }
                let already_resolved = self.ir.symbols[sym_idx.val()].versions.first()
                    .and_then(|v| v.resolved_type.as_ref()).is_some();
                if already_resolved { continue; }
                candidates.insert(sym_idx);
            }
        }
        if candidates.is_empty() { return false; }

        let mut overall_progress = false;
        const MAX_ITER: usize = 8;
        for _ in 0..MAX_ITER {
            let hints = self.collect_backward_inference_hints(&candidates);
            let mut iter_progress = false;
            for (sym_idx, sym_hints) in hints {
                let is_subtype = |a: &ValueType, b: &ValueType| -> bool {
                    self.is_table_subtype(a, b)
                };
                let Some(baseline_intersect) = intersect_hints(&sym_hints.baseline, &is_subtype) else { continue };
                let baseline_has_nil = sym_hints.baseline.iter().all(|h| h.contains_nil());
                let mut combined = sym_hints.baseline.clone();
                combined.extend(sym_hints.narrowing.iter().cloned());
                let inferred = intersect_hints(&combined, &is_subtype).unwrap_or(baseline_intersect);
                let inferred = if baseline_has_nil && !inferred.contains_nil() {
                    ValueType::union(inferred, ValueType::Nil)
                } else {
                    inferred
                };
                if !self.caller_types_mutually_compatible(&sym_hints.caller) { continue; }
                let current = self.ir.symbols[sym_idx.val()].versions.first()
                    .and_then(|v| v.resolved_type.clone());
                if current.as_ref() == Some(&inferred) { continue; }
                if let Some(ver) = self.ir.symbols[sym_idx.val()].versions.first_mut() {
                    ver.resolved_type = Some(inferred);
                    iter_progress = true;
                    overall_progress = true;
                }
            }
            if !iter_progress { break; }
            self.resolved_expr_cache.clear();
        }
        overall_progress
    }

    fn collect_backward_inference_hints(
        &mut self,
        candidates: &std::collections::HashSet<SymbolIndex>,
    ) -> std::collections::HashMap<SymbolIndex, BackwardInferenceHints> {
        use crate::annotations::AnnotationType;
        use crate::ast::Operator;
        use std::collections::{HashMap, HashSet};

        let mut baseline_hints: HashMap<SymbolIndex, Vec<ValueType>> = HashMap::new();
        let mut narrowing_hints: HashMap<SymbolIndex, Vec<ValueType>> = HashMap::new();
        let mut caller_types: HashMap<SymbolIndex, Vec<ValueType>> = HashMap::new();
        let concat_hint = ValueType::union(ValueType::String(None), ValueType::Number);

        for expr_id in 0..self.ir.exprs.len() {
            let expr = self.ir.exprs[expr_id].clone();
            let conditional = self.conditionally_reached_exprs.contains(&ExprId(expr_id));
            match expr {
                Expr::BinaryOp { op, lhs, rhs } => {
                    let lhs_sym = self.candidate_ref_in(lhs, candidates);
                    let rhs_sym = self.candidate_ref_in(rhs, candidates);
                    if lhs_sym.is_none() && rhs_sym.is_none() { continue; }

                    if op.is_arithmetic() {
                        let lhs_ty = self.resolve_expr(lhs);
                        let rhs_ty = self.resolve_expr(rhs);
                        if let Some(s) = lhs_sym
                            && matches!(rhs_ty, Some(ValueType::Number)) {
                                record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, s, ValueType::Number);
                            }
                        if let Some(s) = rhs_sym
                            && matches!(lhs_ty, Some(ValueType::Number)) {
                                record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, s, ValueType::Number);
                            }
                    } else if op == Operator::Concatenate {
                        let lhs_ty = self.resolve_expr(lhs);
                        let rhs_ty = self.resolve_expr(rhs);
                        if let Some(s) = lhs_sym
                            && rhs_ty.as_ref().is_some_and(|t| t.can_concat_to_string()) {
                                record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, s, concat_hint.clone());
                            }
                        if let Some(s) = rhs_sym
                            && lhs_ty.as_ref().is_some_and(|t| t.can_concat_to_string()) {
                                record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, s, concat_hint.clone());
                            }
                    }
                }
                Expr::UnaryOp { op, operand } => {
                    if op == Operator::Subtract
                        && let Some(s) = self.candidate_ref_in(operand, candidates) {
                            record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, s, ValueType::Number);
                        }
                }
                Expr::FunctionCall { func, ref args, is_method_call, .. } => {
                    let candidate_args: Vec<(usize, SymbolIndex)> = args.iter().enumerate()
                        .filter_map(|(i, &a)| self.candidate_ref_in(a, candidates).map(|s| (i, s)))
                        .collect();
                    let Some(func_vt) = self.resolve_expr(func) else { continue };
                    let func_idx = match func_vt {
                        ValueType::Function(Some(idx)) => idx,
                        _ => continue,
                    };
                    let vararg_annotation = self.ir.func(func_idx).vararg_annotation.clone();
                    let called_args = self.ir.func(func_idx).args.clone();
                    let self_offset = if is_method_call && !called_args.is_empty() { 1 } else { 0 };

                    for (param_i, &callee_sym) in called_args.iter().enumerate() {
                        if callee_sym.is_external() { continue; }
                        if !candidates.contains(&callee_sym) { continue; }
                        let Some(arg_i) = param_i.checked_sub(self_offset) else { continue };
                        let Some(&arg_expr) = args.get(arg_i) else { continue };
                        if self.candidate_ref_in(arg_expr, candidates) == Some(callee_sym) {
                            continue;
                        }
                        let Some(arg_type) = self.resolve_expr(arg_expr) else { continue };
                        if matches!(arg_type, ValueType::Nil) { continue; }
                        if arg_type.contains_type_variable() { continue; }
                        caller_types.entry(callee_sym).or_default().push(arg_type);
                    }

                    if candidate_args.is_empty() { continue; }

                    let signatures = self.collect_backward_inference_signatures(
                        func_idx, is_method_call, args.len());
                    let candidate_positions: HashSet<usize> = candidate_args.iter()
                        .map(|(i, _)| *i).collect();

                    let receiver_generic_subs: HashMap<String, ValueType> = if is_method_call {
                        self.bind_receiver_type_args(func_idx, func)
                    } else { HashMap::new() };

                    for sig in &signatures {
                        let mut generic_subs: HashMap<String, ValueType> = receiver_generic_subs.clone();
                        for (arg_i, arg_expr_id) in args.iter().enumerate() {
                            if candidate_positions.contains(&arg_i) { continue; }
                            let Some(sig_param) = sig.param_at(arg_i) else { continue };
                            let param_t = &sig_param.ty;
                            if let ValueType::TypeVariable(name) = param_t {
                                if !generic_subs.contains_key(name)
                                    && let Some(arg_type) = self.resolve_expr(*arg_expr_id) {
                                        generic_subs.insert(name.clone(), arg_type);
                                    }
                                continue;
                            }
                            if let ValueType::Table(Some(idx)) = param_t {
                                let vt = self.table(*idx).value_type.clone();
                                if let Some(ValueType::TypeVariable(name)) = vt
                                    && !generic_subs.contains_key(&name)
                                        && let Some(elem_type) = self.infer_array_element_type(*arg_expr_id) {
                                            generic_subs.insert(name, elem_type);
                                        }
                            }
                        }

                        for &(arg_i, sym) in &candidate_args {
                            let Some(sig_param) = sig.param_at(arg_i) else { continue };
                            let substituted = if generic_subs.is_empty() {
                                sig_param.ty.clone()
                            } else {
                                self.substitute_generics_deep(&sig_param.ty, &generic_subs)
                            };
                            if self.type_contains_type_variable_deep(&substituted) { continue; }
                            if sig_param.optional {
                                narrowing_hints.entry(sym).or_default().push(substituted);
                            } else {
                                record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, sym, substituted);
                            }
                        }
                    }

                    let vararg_vt = vararg_annotation.as_ref()
                        .and_then(|a| self.resolve_annotation_type(a))
                        .filter(|t| !t.contains_type_variable());
                    for (arg_i, sym) in candidate_args {
                        let covered_by_signature = signatures.iter()
                            .any(|sig| sig.param_at(arg_i).is_some());
                        let target_idx = arg_i + self_offset;

                        let target_unannotated = match self.ir.func(func_idx)
                            .param_annotations.get(target_idx)
                        {
                            None => true,
                            Some(AnnotationType::Simple(s)) => s.is_empty(),
                            _ => false,
                        };
                        if !covered_by_signature && target_unannotated
                            && let Some(&target_sym) = called_args.get(target_idx)
                                && !target_sym.is_external() {
                                    let inferred = self.ir.symbols.get(target_sym.val())
                                        .and_then(|s| s.versions.first())
                                        .and_then(|v| v.resolved_type.clone())
                                        .filter(|t| !t.contains_type_variable());
                                    if let Some(vt) = inferred {
                                        record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, sym, vt);
                                        continue;
                                    }
                                }

                        if !covered_by_signature
                            && let Some(ref vt) = vararg_vt {
                                narrowing_hints.entry(sym).or_default().push(vt.clone());
                            }
                    }
                }
                Expr::BracketIndex { table, key, .. } => {
                    if let Some(sym) = self.candidate_ref_in(key, candidates) {
                        let table_expr = table;
                        if let Some(table_type) = self.resolve_expr(table_expr)
                            && let ValueType::Table(Some(idx)) = &table_type {
                                let kt = self.table(*idx).key_type.clone();
                                if let Some(mut key_vt) = kt {
                                    if key_vt.contains_type_variable() {
                                        let type_args = self.get_expr_type_args(table_expr);
                                        if !type_args.is_empty() {
                                            let params = self.table(*idx).class_type_params.clone();
                                            let subs: HashMap<String, ValueType> = params.into_iter()
                                                .zip(type_args.into_iter())
                                                .collect();
                                            key_vt = self.substitute_generics_deep(&key_vt, &subs);
                                        }
                                    }
                                    if !self.type_contains_type_variable_deep(&key_vt) {
                                        record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, sym, key_vt);
                                    }
                                }
                            }
                    }
                }
                _ => {}
            }
        }

        for fa in &self.ir.field_assignments {
            if !fa.had_annotation_at_build { continue; }
            if let Some(field_info) = self.ir.get_field(fa.table_idx, &fa.field_name)
                && let Some(ref expected) = field_info.annotation
                && let Some(s) = self.candidate_ref_in(fa.actual_expr, candidates)
            {
                narrowing_hints.entry(s).or_default().push(expected.clone());
            }
        }
        for (&sym_idx, expected) in &self.ir.symbol_type_annotations {
            let sym = self.sym(sym_idx);
            for ver in &sym.versions {
                let Some(original_expr) = ver.original_type_source else { continue };
                if let Some(s) = self.candidate_ref_in(original_expr, candidates) {
                    narrowing_hints.entry(s).or_default().push(expected.clone());
                }
            }
        }
        for func in &self.ir.functions {
            for &ret_sym_idx in &func.rets {
                let sym = self.sym(ret_sym_idx);
                let SymbolIdentifier::FunctionRet(_, ret_index) = &sym.id else { continue };
                for ver in &sym.versions {
                    let Some(rhs_expr) = ver.type_source else { continue };
                    if let Some(s) = self.candidate_ref_in(rhs_expr, candidates)
                        && let Some(expected) = func.return_annotations.get(*ret_index)
                    {
                        narrowing_hints.entry(s).or_default().push(expected.clone());
                    }
                }
            }
        }

        let mut out: HashMap<SymbolIndex, BackwardInferenceHints> = HashMap::new();
        for (s, baseline) in baseline_hints {
            let narrowing: Vec<ValueType> = narrowing_hints.remove(&s)
                .unwrap_or_default()
                .into_iter()
                .filter(|t| !matches!(t, ValueType::Any))
                .collect();
            let caller = caller_types.remove(&s).unwrap_or_default();
            out.insert(s, BackwardInferenceHints { baseline, narrowing, caller });
        }
        out
    }

    fn candidate_ref_in(&self, expr_id: ExprId, candidates: &std::collections::HashSet<SymbolIndex>) -> Option<SymbolIndex> {
        match self.expr(expr_id) {
            Expr::SymbolRef(sym, _) if candidates.contains(sym) => Some(*sym),
            Expr::Grouped(inner) => self.candidate_ref_in(*inner, candidates),
            _ => None,
        }
    }

    fn param_symbol_resolved_type(&mut self, sym_idx: SymbolIndex) -> Option<ValueType> {
        let ver = self.sym(sym_idx).versions.first()?;
        if let Some(rt) = ver.resolved_type.clone() { return Some(rt); }
        let src = ver.type_source?;
        self.resolve_expr(src)
    }

    fn caller_types_mutually_compatible(&self, caller_types: &[ValueType]) -> bool {
        for (i, a) in caller_types.iter().enumerate() {
            for b in &caller_types[i + 1..] {
                if intersect_pair(a, b).is_some() { continue; }
                if self.is_table_subtype(a, b) || self.is_table_subtype(b, a) { continue; }
                return false;
            }
        }
        true
    }

    fn collect_backward_inference_signatures(
        &mut self,
        func_idx: FunctionIndex,
        is_method_call: bool,
        n_args: usize,
    ) -> Vec<BackwardInferenceSignature> {
        let called = self.ir.func(func_idx);
        let param_optional = called.param_optional.clone();
        let param_args = called.args.clone();
        let called_args_len = called.args.len();
        let is_vararg_primary = called.is_vararg;
        let overloads = called.overloads.clone();
        let primary_self_offset = if is_method_call && called_args_len > 0 { 1 } else { 0 };

        let mut out: Vec<BackwardInferenceSignature> = Vec::new();

        let primary_non_self_opts: &[bool] = param_optional.get(primary_self_offset..).unwrap_or(&[]);
        let primary_required = primary_non_self_opts.iter().filter(|&&o| !o).count();
        let primary_total = called_args_len.saturating_sub(primary_self_offset);
        let primary_arity_ok = n_args >= primary_required
            && (is_vararg_primary || n_args <= primary_total);
        if primary_arity_ok {
            let params: Vec<Option<BackwardInferenceSigParam>> = param_args.iter()
                .skip(primary_self_offset)
                .map(|&sym_idx| {
                    let vt = self.param_symbol_resolved_type(sym_idx)?;
                    build_sig_param(vt)
                })
                .collect();
            out.push(BackwardInferenceSignature { params });
        }

        for overload in &overloads {
            if overload.is_return_only { continue; }
            let off = if overload.params.first().is_some_and(|p| p.name == "self") { 1 } else { 0 };
            let non_self_params = &overload.params[off..];
            let required = non_self_params.iter().filter(|p| !p.optional).count();
            let total = non_self_params.len();
            if n_args < required || n_args > total { continue; }
            let params: Vec<Option<BackwardInferenceSigParam>> = non_self_params.iter().map(|p| {
                let vt = p.typ.clone()?;
                let vt = if p.optional { ValueType::union(vt, ValueType::Nil) } else { vt };
                build_sig_param(vt)
            }).collect();
            out.push(BackwardInferenceSignature { params });
        }

        out
    }
}

// ── Free functions ───────────────────────────────────────────────────────────

fn build_sig_param(vt: ValueType) -> Option<BackwardInferenceSigParam> {
    if vt.contains_nil() {
        let stripped = vt.strip_nil();
        if matches!(&stripped, ValueType::Union(m) if m.is_empty()) {
            return None;
        }
        Some(BackwardInferenceSigParam { ty: stripped, optional: true })
    } else {
        Some(BackwardInferenceSigParam { ty: vt, optional: false })
    }
}

struct BackwardInferenceSignature {
    params: Vec<Option<BackwardInferenceSigParam>>,
}

struct BackwardInferenceSigParam {
    ty: ValueType,
    optional: bool,
}

struct BackwardInferenceHints {
    baseline: Vec<ValueType>,
    narrowing: Vec<ValueType>,
    caller: Vec<ValueType>,
}

impl BackwardInferenceSignature {
    fn param_at(&self, arg_i: usize) -> Option<&BackwardInferenceSigParam> {
        self.params.get(arg_i).and_then(|p| p.as_ref())
    }
}

fn record_hint(
    baseline: &mut std::collections::HashMap<SymbolIndex, Vec<ValueType>>,
    narrowing: &mut std::collections::HashMap<SymbolIndex, Vec<ValueType>>,
    conditional: bool,
    sym: SymbolIndex,
    vt: ValueType,
) {
    if conditional {
        narrowing.entry(sym).or_default().push(vt);
    } else {
        baseline.entry(sym).or_default().push(vt);
    }
}

fn intersect_hints(
    hints: &[ValueType],
    is_subtype: &dyn Fn(&ValueType, &ValueType) -> bool,
) -> Option<ValueType> {
    let mut iter = hints.iter();
    let mut acc = iter.next()?.clone();
    for h in iter {
        acc = intersect_pair_impl(&acc, h, is_subtype)?;
    }
    Some(acc)
}

pub(super) fn intersect_pair(a: &ValueType, b: &ValueType) -> Option<ValueType> {
    intersect_pair_impl(a, b, &|_, _| false)
}

fn intersect_pair_impl(
    a: &ValueType,
    b: &ValueType,
    is_subtype: &dyn Fn(&ValueType, &ValueType) -> bool,
) -> Option<ValueType> {
    if matches!(a, ValueType::Any) || matches!(b, ValueType::Any) {
        return None;
    }
    if a == b { return Some(a.clone()); }
    let split = |t: &ValueType| -> Vec<ValueType> {
        match t {
            ValueType::Union(members) => members.clone(),
            other => vec![other.clone()],
        }
    };
    let assignable_or_subtype = |m: &ValueType, target: &ValueType| -> bool {
        m.is_assignable_to(target) || is_subtype(m, target)
    };
    let keep: Vec<ValueType> = split(a).into_iter()
        .filter(|m| assignable_or_subtype(m, b))
        .collect();
    if !keep.is_empty() {
        return Some(ValueType::make_union(keep));
    }
    let keep: Vec<ValueType> = split(b).into_iter()
        .filter(|m| assignable_or_subtype(m, a))
        .collect();
    if !keep.is_empty() {
        return Some(ValueType::make_union(keep));
    }
    None
}
