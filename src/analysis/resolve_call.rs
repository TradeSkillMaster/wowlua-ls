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
        let CallSiteInfo { is_method_call, .. } = call_site;
        // Resolve the function expression to get its type
        let func_type = self.resolve_expr(func_expr_id)?;
        // Unwrap opaque aliases — calling an opaque-wrapped function works on the inner type
        let func_type = func_type.into_strip_opaque();
        let mut constructor_table_idx: Option<TableIndex> = None;
        let mut call_func_table_idx: Option<TableIndex> = None;
        let mut call_func_is_metamethod = false;
        let func_idx = match func_type {
            ValueType::Function(Some(idx)) => idx,
            ValueType::Table(Some(table_idx)) => {
                if let Some(fi) = self.table(table_idx).call_func {
                    call_func_table_idx = Some(table_idx);
                    call_func_is_metamethod = self.table(table_idx).call_func_is_metamethod;
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
        let self_offset = super::call_self_offset(
            call_func_is_metamethod,
            call_func_table_idx.is_some() && !call_func_is_metamethod,
            constructor_table_idx.is_some(),
            is_method_call,
            has_self,
            !func_args.is_empty(),
        );

        // Resolve receiver info for method calls: projected_f_idx (for params<F>)
        // and class_type_param_subs (for class-level generics like Box<T>).
        // Both need the receiver expression and type_args, so they share a single
        // receiver-analysis block to avoid redundant expression cloning/resolution.
        let mut projected_f_idx: Option<FunctionIndex> = None;
        let mut class_type_param_subs: HashMap<String, ValueType> = HashMap::new();
        if is_method_call
            && let Expr::FieldAccess { table: receiver_expr, field_range, .. } = self.expr(*func).clone()
        {
            let receiver_type_args = self.get_expr_type_args(receiver_expr);

            // params<F> projection: bind F from receiver's @type X<fun(...)>
            let proj_name: Option<String> = match &self.func(func_idx).vararg_projection {
                Some(crate::types::ProjectionKind::Params(n)) => Some(n.clone()),
                _ => None,
            };
            if let Some(proj_name) = proj_name {
                let param0 = param_annotations.first().cloned();
                if let Some(crate::annotations::AnnotationType::Parameterized(_, type_arg_anns)) = param0
                    && receiver_type_args.len() == type_arg_anns.len()
                {
                    for (pos, type_arg_ann) in type_arg_anns.iter().enumerate() {
                        if let crate::annotations::AnnotationType::Simple(gname) = type_arg_ann
                            && *gname == proj_name
                                && let Some(ValueType::Function(Some(f_idx))) = receiver_type_args.get(pos) {
                                    projected_f_idx = Some(*f_idx);
                                    break;
                                }
                    }
                }
            }

            // Class-level type param substitution for inline callback resolution.
            // When calling a method on a parameterized class (e.g. Box<boolean>:Apply(fun(value: T))),
            // we need to resolve T → boolean from the receiver's type_args.
            let receiver_type = self.resolve_expr(receiver_expr);
            let ctp = match &receiver_type {
                Some(ValueType::Table(Some(tidx))) => self.table(*tidx).class_type_params.clone(),
                _ => Vec::new(),
            };
            if !ctp.is_empty() {
                for (pos, name) in ctp.iter().enumerate() {
                    if let Some(vt) = receiver_type_args.get(pos) {
                        class_type_param_subs.insert(name.clone(), vt.clone());
                    }
                }
            }
            // Record the substitution keyed by the method-name token range so
            // hover can display the bound concrete types (e.g. `T → string`) in
            // the method signature. Only stored when at least one type param
            // resolved to a non-type-variable concrete type.
            if let Some(range) = field_range
                && !class_type_param_subs.is_empty()
                && class_type_param_subs.values()
                    .any(|vt| !matches!(vt, ValueType::TypeVariable(_)))
            {
                // Overwrite each fixpoint iteration so the converged (most
                // concrete) substitution wins over earlier partial ones.
                self.method_decl_subs.insert(range, class_type_param_subs.clone());
            }
        }
        // Order doesn't matter here: resolve_annotation_type_gen only uses the names
        // for TypeVariable recognition, not positional binding.
        let class_gen_context: Vec<(String, Option<String>)> = class_type_param_subs.keys()
            .map(|k| (k.clone(), None)).collect();

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
                Some(ann) => {
                    // Resolve aliases and optional fun types (e.g. fun(...)?, @alias commsHandler fun(...))
                    let Some((crate::annotations::AnnotationType::Fun(params, returns, is_vararg), _)) =
                        crate::annotations::reduce_to_fun_alias(
                            ann, &self.ir.alias_fun_types, &self.ir.ext.alias_fun_types,
                        ) else { continue };
                    crate::annotations::OverloadSig {
                        params: params.clone(),
                        returns: returns.clone(),
                        is_vararg: *is_vararg,
                        is_return_only: false,
                    }
                }
                None => continue,
            };
            let inline_args = self.ir.functions[inline_func_idx.val()].args.clone();
            for (j, param_info) in sig.params.iter().enumerate() {
                let Some(&inline_sym_idx) = inline_args.get(j) else { continue };
                if inline_sym_idx.is_external() { continue; }
                if self.ir.symbols[inline_sym_idx.val()].versions.first()
                    .is_some_and(|v| v.resolved_type.is_some()) { continue; }
                if let Some(vt) = self.resolve_annotation_with_class_generics(
                    &param_info.typ, &class_gen_context, &class_type_param_subs,
                ) {
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
                        if let Some(vt) = self.resolve_annotation_with_class_generics(
                            ret_annotation, &class_gen_context, &class_type_param_subs,
                        ) {
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
        // Track generics set by constraint fallback so overload re-inference can
        // override them. Lifecycle:
        //  1. Populated at constraint fallback (~line 400): unbound generics get
        //     their constraint type and their name is recorded here.
        //  2. Consumed at overload re-inference (~line 600): when the matched
        //     overload maps args to different positions than the primary, bindings
        //     from phase 1 can be overridden with actual call-site arg types.
        //  3. After string→function resolution (~line 630): if a function-
        //     constrained generic was re-bound to a string but couldn't resolve
        //     to a global function, it is restored to the constraint type.
        let mut constraint_fallback_names: HashSet<String> = HashSet::new();
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
                                self.resolve_backtick_arg(arg_expr_id, &arg_type)
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
                                    let inferred = if param_annotations.get(i + self_offset)
                                        .is_some_and(crate::annotations::annotation_contains_backtick)
                                    {
                                        self.resolve_backtick_arg(arg_expr_id, &stripped)
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

            // Bind variadic generics (`@generic ...M`): collect types from
            // excess arguments (beyond the positional params) into an Intersection.
            // Only one variadic generic per function is supported (first `...`-prefixed wins).
            if let Some(variadic_name) = generic_names.iter().find(|n| n.starts_with("...")).cloned()
                && !generic_subs.contains_key(&variadic_name)
            {
                // func_args contains declared params (excluding `...` vararg),
                // so this count gives the number of positional (non-variadic) params.
                let non_vararg_count = func_args.len() - self_offset;
                let excess_types: Vec<ValueType> = args.iter()
                    .skip(non_vararg_count)
                    .filter_map(|&arg_expr_id| self.resolve_expr(arg_expr_id))
                    .collect();
                if !excess_types.is_empty() {
                    let bound = if excess_types.len() == 1 {
                        excess_types.into_iter().next().unwrap()
                    } else {
                        ValueType::Intersection(excess_types)
                    };
                    generic_subs.insert(variadic_name.clone(), bound);
                    substitutable_generic_names.insert(variadic_name);
                }
            }

            // Bind generic from vararg projection (returns<F>): when the vararg
            // has a Return projection and F is not yet bound, look at the last
            // vararg argument — if it's a function call, bind F to that callee.
            let vararg_return_proj_name = match &self.func(func_idx).vararg_projection {
                Some(crate::types::ProjectionKind::Return(n, _)) => Some(n.clone()),
                _ => None,
            };
            if let Some(proj_name) = vararg_return_proj_name
                && !generic_subs.contains_key(&proj_name)
            {
                // The last argument in the vararg region may be a multi-return
                // function call — bind F to its callee's function type.
                let non_vararg_count = func_args.len() - self_offset;
                if args.len() > non_vararg_count
                    && let Some(&last_arg) = args.last()
                    && let Expr::FunctionCall { func: callee_expr, .. } = self.expr(last_arg).clone()
                    && let Some(ValueType::Function(Some(f_idx))) = self.resolve_expr(callee_expr)
                {
                    generic_subs.insert(proj_name.clone(), ValueType::Function(Some(f_idx)));
                    substitutable_generic_names.insert(proj_name);
                }
            }

            // Fallback: for any generic not inferred, use its constraint type.
            // Track which generics were set by this fallback so overload
            // re-inference can override them with actual call-site bindings.
            for (name, constraint) in &generics {
                if !generic_subs.contains_key(name)
                    && let Some(ct) = constraint {
                        generic_subs.insert(name.clone(), ct.clone());
                        constraint_fallback_names.insert(name.clone());
                    }
            }
        }

        // Extend projected_f_idx for non-method calls: if the vararg has a
        // projection (Params or Return) and the generic is now bound via
        // argument inference, use it. Skip method calls — those are handled
        // by the receiver-type-args path above.
        let projected_f_idx = projected_f_idx.or_else(|| {
            if is_method_call { return None; }
            let proj_name = match &self.func(func_idx).vararg_projection {
                Some(crate::types::ProjectionKind::Params(n)) => Some(n),
                Some(crate::types::ProjectionKind::Return(n, _)) => Some(n),
                _ => None,
            }?;
            match generic_subs.get(proj_name)? {
                ValueType::Function(Some(idx)) => Some(*idx),
                _ => None,
            }
        });

        // When multiple overloads tie at zero mismatches (no single best match),
        // we union their return types at this ret_index. Computed lazily below.
        let mut ambiguous_overload_ret_type: Option<ValueType> = None;

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
                    n_args >= required && (o.is_vararg || n_args <= total)
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
                let best_score = scored.iter().map(|(_, m)| *m).min();
                if let Some(0) = best_score {
                    let zero_mismatch: Vec<&ResolvedOverload> = scored.iter()
                        .filter(|(_, m)| *m == 0)
                        .map(|(o, _)| *o)
                        .collect();
                    if zero_mismatch.len() == 1 {
                        Some(zero_mismatch[0])
                    } else {
                        // Multiple equally-good overloads: union their returns at ret_index.
                        // Diagnostics will fall through to the primary signature, which is
                        // acceptable for indistinguishable overloads (typically zero-param).
                        let types: Vec<ValueType> = zero_mismatch.iter()
                            .map(|o| o.return_type_at(ret_index))
                            .collect();
                        ambiguous_overload_ret_type = Some(ValueType::make_union(types));
                        None
                    }
                } else {
                    None
                }
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
                    // Direct TypeVariable: T → infer T = arg_type.
                    // Allow overriding constraint-fallback bindings since
                    // the overload may map args to different positions than
                    // the primary (e.g. 2-arg overload vs 3-arg primary).
                    if let ValueType::TypeVariable(name) = param_type
                        && generic_names.contains(name)
                        && (!generic_subs.contains_key(name) || constraint_fallback_names.contains(name))
                    {
                            generic_subs.insert(name.clone(), arg_type.clone());
                            constraint_fallback_names.remove(name);
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

        // Resolve string-bound function-constrained generics to global function types.
        // When a generic with constraint `function` is bound to a string literal
        // (e.g. via `@overload fun(name: `F`, hook: F)` matching a call like
        // hooksecurefunc("FuncName", callback)), look up the string as a global
        // function name and re-bind the generic to the actual function type.
        // If the lookup fails (unknown function name), restore the constraint
        // type to avoid false-positive type-mismatch / generic-constraint-mismatch.
        if has_generics {
            for (name, constraint) in &generics {
                if matches!(constraint, Some(ValueType::Function(None)))
                    && matches!(generic_subs.get(name), Some(ValueType::String(_)))
                    && let Some(&arg_idx) = generic_arg_indices.get(name)
                    && let Some(&arg_expr) = args.get(arg_idx)
                {
                    let fn_name = self.ir.string_literals.get(&arg_expr)
                        .cloned()
                        .or_else(|| self.resolve_string_literal_through_expr(&arg_expr));
                    if let Some(fn_name) = fn_name
                        && let Some(func_type) = self.resolve_global_function_type(&fn_name)
                    {
                        generic_subs.insert(name.clone(), func_type);
                    } else {
                        // Unknown function name — restore the constraint type
                        // so F stays as `function` (not `string`), preventing
                        // generic-constraint-mismatch and type-mismatch false positives.
                        generic_subs.insert(name.clone(), constraint.clone().unwrap());
                    }
                }
            }
        }

        // Propagate matched overload's fun() callback types into inline function params.
        // This enables contextual typing for patterns like:
        //   obj:SetScript("OnEvent", function(self, event, ...) end)
        // where the overload specifies the handler signature.
        if let Some(overload) = matching_overload {
            let receiver_type: Option<ValueType> = if is_method_call {
                if let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(func_expr_id).clone() {
                    self.resolve_expr(receiver_expr)
                } else { None }
            } else { None };
            for (i, arg_expr_id) in args.iter().enumerate() {
                let inline_func_idx = match self.ir.expr(*arg_expr_id) {
                    Expr::FunctionDef(idx) => *idx,
                    _ => continue,
                };
                if inline_func_idx.is_external() { continue; }
                let param_idx = i + overload_self_offset;
                let Some(param) = overload.params.get(param_idx) else { continue };
                // Apply generic substitution so TypeVariable("F") → Function(idx)
                let param_type = param.typ.as_ref().map(|t| {
                    if generic_subs.is_empty() { t.clone() }
                    else { self.substitute_generics_deep(t, &generic_subs) }
                });
                let expected_fn_idx = match &param_type {
                    Some(ValueType::Function(Some(idx))) => *idx,
                    // Unwrap optional fun types: fun(...)? → Union([Fun(...), nil])
                    Some(ValueType::Union(members)) => {
                        match members.iter().find_map(|m| match m {
                            ValueType::Function(Some(idx)) => Some(*idx),
                            _ => None,
                        }) {
                            Some(idx) => idx,
                            None => continue,
                        }
                    }
                    _ => continue,
                };
                let expected_args = self.func(expected_fn_idx).args.clone();
                let inline_args = self.ir.functions[inline_func_idx.val()].args.clone();
                for (j, &expected_sym) in expected_args.iter().enumerate() {
                    let Some(&inline_sym_idx) = inline_args.get(j) else { continue };
                    if inline_sym_idx.is_external() { continue; }
                    if self.ir.symbols[inline_sym_idx.val()].versions.first()
                        .is_some_and(|v| v.resolved_type.is_some()) { continue; }
                    let vt = self.sym(expected_sym).versions.first()
                        .and_then(|v| v.resolved_type.clone());
                    if let Some(mut vt) = vt {
                        // Self-substitution: first param named "self" gets the receiver's type
                        if j == 0
                            && matches!(&self.ir.symbols[inline_sym_idx.val()].id, SymbolIdentifier::Name(n) if n == "self")
                            && let Some(ref recv_type) = receiver_type
                        {
                            vt = recv_type.clone();
                        }
                        self.ir.symbols[inline_sym_idx.val()].versions[0].resolved_type = Some(vt);
                    }
                }
                // Propagate event_params from the expected function to the inline callback
                if let Some(ref ep) = self.func(expected_fn_idx).event_params.clone() {
                    self.ir.functions[inline_func_idx.val()].event_params = Some(ep.clone());
                    // Two mechanisms preserve the event type name for hover display:
                    //  1. event_type_display — propagates through SymbolRef so
                    //     `local e = event` also shows the alias (resolve.rs).
                    //  2. param_annotations — makes the param declaration hover
                    //     use the annotation-text path (queries.rs).
                    let event_param_idx = ep.1;
                    if let Some(&inline_sym_idx) = inline_args.get(event_param_idx)
                        && !inline_sym_idx.is_external()
                    {
                        self.ir.event_type_display.insert((inline_sym_idx, 0), ep.0.clone());
                        let inline_func = &mut self.ir.functions[inline_func_idx.val()];
                        // Skip if the user already wrote a @param annotation (Simple("") is
                        // the empty placeholder used when no annotation exists).
                        let needs_annotation = inline_func.param_annotations
                            .get(event_param_idx)
                            .is_none_or(|a| matches!(a, crate::annotations::AnnotationType::Simple(s) if s.is_empty()));
                        if needs_annotation {
                            while inline_func.param_annotations.len() <= event_param_idx {
                                inline_func.param_annotations.push(crate::annotations::AnnotationType::Simple(String::new()));
                            }
                            inline_func.param_annotations[event_param_idx] =
                                crate::annotations::AnnotationType::Simple(ep.0.clone());
                        }
                    }
                }
                // Propagate vararg_annotation for inlay hints
                if self.ir.functions[inline_func_idx.val()].vararg_annotation.is_none()
                    && let Some(ann) = self.func(expected_fn_idx).vararg_annotation.clone()
                {
                    self.ir.functions[inline_func_idx.val()].vararg_annotation = Some(ann);
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
            } else if let Some(f_idx) = projected_f_idx
                && matches!(self.func(func_idx).vararg_projection, Some(crate::types::ProjectionKind::Params(_)))
            {
                let non_vararg_count = func_args.len() - self_offset;
                i.checked_sub(non_vararg_count).and_then(|pos| {
                    let f_arg_sym = *self.func(f_idx).args.get(pos)?;
                    self.sym(f_arg_sym).versions.first()
                        .and_then(|ver| ver.resolved_type.clone())
                })
            } else {
                None
            };
            // Apply generic substitutions and filter out unresolved type variables.
            // Exclude generics bound from THIS argument — checking an argument
            // against a type derived from itself is circular and produces false
            // positives when the argument's type is later widened (e.g. table
            // mutation inside a `for ... in pairs(t)` loop body).
            let expected_type = expected_type.map(|et| {
                if !substitutable_generic_names.is_empty() {
                    let structural_subs: HashMap<String, ValueType> = generic_subs.iter()
                        .filter(|(k, _)| substitutable_generic_names.contains(k.as_str()))
                        .filter(|(k, _)| generic_arg_indices.get(k.as_str()).copied() != Some(i))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    if !structural_subs.is_empty() {
                        self.substitute_generics_deep(&et, &structural_subs)
                    } else {
                        et
                    }
                } else {
                    et
                }
            }).filter(|et| !matches!(et, ValueType::TypeVariable(_)))
            .filter(|et| !self.type_contains_type_variable_deep(et));
            // When multiple overloads tied at 0 mismatches, we know the call is
            // valid against at least one overload — suppress type-mismatch checks.
            let expected_type = if ambiguous_overload_ret_type.is_some() { None } else { expected_type };
            // Skip backtick params (type-name string literals)
            let skip_backtick = matching_overload.is_none()
                && param_annotations.get(i + self_offset).is_some_and(crate::annotations::annotation_contains_backtick);
            if skip_backtick && expected_type.is_some() {
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
                let actual_type_args = self.get_expr_type_args(*arg_expr_id);
                let expected_parameterized = if actual_type_args.is_empty() {
                    Vec::new()
                } else {
                    param_annotations.get(i + self_offset)
                        .map(|ann| self.param_parameterized_constraints(ann, &generic_subs))
                        .unwrap_or_default()
                };
                resolved_call_args.push(ResolvedCallArg {
                    expected_type, arg_expr: *arg_expr_id, param_name,
                    skip_if_nil, primary_param_type, start, end,
                    actual_type_args, expected_parameterized,
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
                receiver_param_subs: class_type_param_subs.clone(),
            });

            // Detect expression<C, R> parameters and record them for diagnostics/queries
            let receiver_table_idx = if is_method_call {
                let receiver_expr = match self.expr(func_expr_id) {
                    Expr::FieldAccess { table, .. } => Some(*table),
                    _ => None,
                };
                receiver_expr.and_then(|re| match self.resolve_expr(re) {
                    Some(ValueType::Table(Some(idx))) => Some(idx),
                    _ => None,
                })
            } else { None };
            for (i, arg_expr_id) in args.iter().enumerate() {
                if let Some(crate::annotations::AnnotationType::Parameterized(base, type_args)) =
                    param_annotations.get(i + self_offset)
                    && base == "expression" && !type_args.is_empty()
                    && let Some(&(start, end)) = arg_ranges.get(i)
                {
                    let table_idxs = self.resolve_expression_tables(&type_args[0], receiver_table_idx);
                    if !table_idxs.is_empty() {
                        // When the result type `R` is one of the method's generic
                        // type parameters, infer it from the expression body and
                        // bind it (so `@return Schema<R>` becomes `Schema<number>`
                        // etc.). Otherwise R is a fixed constraint to check against.
                        let r_generic = match type_args.get(1) {
                            Some(crate::annotations::AnnotationType::Simple(name))
                                if generics.iter().any(|(g, _)| g == name) => Some(name.clone()),
                            _ => None,
                        };
                        let return_type = if r_generic.is_some() {
                            None
                        } else {
                            type_args.get(1).and_then(|rt| self.resolve_annotation_type(rt))
                        };
                        if let Some(rname) = r_generic
                            && !generic_subs.contains_key(&rname)
                            && let Some(content) = self.ir.string_literals.get(arg_expr_id).cloned()
                        {
                            let wrapped = format!("return {content}");
                            let expr_tree = crate::syntax::parser::Parser::new(&wrapped).parse();
                            let inferred = crate::diagnostics::expression_type::infer_expression_type(
                                &expr_tree,
                                &|word: &str| table_idxs.iter()
                                    .find_map(|&idx| self.get_field(idx, word)
                                        .and_then(|fi| {
                                            let ty = fi.annotation.clone()?;
                                            // Lateinit (T!) fields strip nil for static access
                                            // (no need-check-nil), but in expression<> strings
                                            // the expression evaluates at runtime when the field
                                            // may still be unset, so include nil to avoid a false
                                            // positive type-mismatch on the generic R binding.
                                            if fi.lateinit {
                                                Some(ValueType::make_union(vec![ty, ValueType::Nil]))
                                            } else {
                                                Some(ty)
                                            }
                                        })),
                            );
                            if let Some(t) = inferred
                                && !matches!(t, ValueType::Any)
                            {
                                substitutable_generic_names.insert(rname.clone());
                                generic_subs.insert(rname, t);
                            }
                        }
                        self.ir.expression_args.insert(*arg_expr_id, crate::analysis::ExpressionArg {
                            table_idxs,
                            return_type,
                            str_range: (start, end),
                        });
                    }
                }
            }
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
            let receiver_expr_id = if let Expr::FieldAccess { table: receiver_expr, .. } = self.expr(*func).clone() {
                Some(receiver_expr)
            } else {
                None
            };
            let receiver_type = receiver_expr_id.and_then(|re| self.resolve_expr(re));
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
                // @return self<X>: re-parameterize the receiver with the given
                // class type args. Resolve them (substituting any of the method's
                // own generics) and cache under this call's ExprId so display and
                // downstream resolution reconstruct `Class<X>`.
                if let Some(self_args) = self.func(func_idx).returns_self_type_args.clone() {
                    let fn_generics = self.func(func_idx).generic_constraints_raw.clone();
                    let mut resolved: Vec<ValueType> = self_args.iter()
                        .map(|ta| self.resolve_annotation_type_mut_gen(ta, &fn_generics)
                            .unwrap_or(ValueType::Any))
                        .collect();
                    for (i, arg) in resolved.iter_mut().enumerate() {
                        *arg = self.substitute_generics_deep(arg, &generic_subs);
                        // T! (NonNil) strips nil after generic substitution so that
                        // e.g. @return self<T!> on Publisher<string?> yields Publisher<string>.
                        if matches!(self_args.get(i), Some(crate::annotations::AnnotationType::NonNil(_))) {
                            *arg = arg.strip_nil();
                        }
                    }
                    if !resolved.is_empty() {
                        self.call_type_args.insert(expr_id, resolved);
                    }
                } else {
                    // Plain @return self: propagate receiver's type args so that
                    // chained calls (e.g. pub:Filter():IgnoreNil()) can resolve
                    // the class type params from the intermediate call result.
                    // This is a general chaining fix — without it, any @return self
                    // method in a chain loses its receiver's type parameterization.
                    let receiver_args = self.get_expr_type_args(receiver_expr_id.unwrap());
                    if !receiver_args.is_empty() {
                        self.call_type_args.insert(expr_id, receiver_args);
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
        // When the last `returns<F>` projection is at or beyond the last
        // return annotation, higher ret_indices expand into F's returns
        // (e.g. `@return returns<F>` alone, or `@return boolean` followed
        // by `@return returns<F>` as in pcall).
        let last_proj_index = self.func(func_idx).return_projections.keys().max().copied();
        let is_expansion = last_proj_index.is_some_and(|lpi|
            ret_index > lpi
            && !self.func(func_idx).return_projections.contains_key(&ret_index));
        let proj = if is_expansion {
            last_proj_index.and_then(|lpi| self.func(func_idx).return_projections.get(&lpi).cloned())
        } else {
            self.func(func_idx).return_projections.get(&ret_index).cloned()
        };
        // Skip the projection when a concrete overload matched — the overload's
        // return type is more specific.  E.g. select("#", ...) matches the
        // `fun(index: "#", ...): integer` overload; without this guard the
        // returns<F, index> projection would fire and return F's first return.
        if matching_overload.is_none()
            && let Some(proj) = proj
            && let crate::types::ProjectionKind::Return(ref name, ref offset_param) = proj
                && let Some(bound) = generic_subs.get(name).cloned()
                    && let ValueType::Function(Some(f_idx)) = bound {
                        let f_returns = self.func(f_idx).return_annotations.clone();
                        let f_has_vararg = self.func(f_idx).has_vararg_return;

                        // Evaluate offset from the named parameter's literal integer value.
                        // `returns<F, index>` where index=8 means start from F's 8th return (1-indexed).
                        let offset = offset_param.as_ref().and_then(|param_name| {
                            // Find which argument position corresponds to `param_name`
                            let param_pos = func_args.iter().enumerate().find_map(|(i, &sym_idx)| {
                                if let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id
                                    && n == param_name
                                {
                                    return Some(i);
                                }
                                None
                            })?;
                            // Get the actual argument expression at that position (accounting for self_offset)
                            let arg_idx = param_pos.checked_sub(self_offset)?;
                            let arg_expr = args.get(arg_idx)?;
                            // Check if it's a numeric literal
                            let num_str = self.ir.number_literals.get(arg_expr)?;
                            let n: usize = num_str.parse().ok()?;
                            // Convert from 1-indexed Lua convention to 0-indexed
                            Some(n.saturating_sub(1))
                        }).unwrap_or(0);

                        // When the projection sits at a non-zero return slot
                        // (e.g. pcall's `@return boolean` at 0, `@return returns<F>`
                        // at 1), subtract the projection's base index so F's returns
                        // start from position 0.
                        let proj_base = if is_expansion {
                            last_proj_index.unwrap_or(0)
                        } else {
                            ret_index
                        };
                        let effective_index = (ret_index - proj_base) + offset;
                        let vt = f_returns.get(effective_index).cloned()
                            .or_else(|| {
                                if f_has_vararg && !f_returns.is_empty() {
                                    f_returns.last().cloned()
                                } else if f_returns.is_empty() {
                                    let f_scope = self.func(f_idx).scope;
                                    let ret_id = SymbolIdentifier::FunctionRet(f_idx, effective_index);
                                    self.get_symbol(&ret_id, f_scope)
                                        .and_then(|si| self.sym(si).versions.first()
                                            .and_then(|v| v.resolved_type.clone()))
                                } else { None }
                            })
                            .unwrap_or(ValueType::Nil);
                        // Union with the static annotation at this return slot.
                        // Strips `Any` and `Nil` members — unresolved projection
                        // placeholders (`returns<F>` before binding) can resolve
                        // to either, and including them would subsume (Any) or
                        // widen (Nil) the projected type incorrectly.
                        let vt = if !is_expansion {
                            if let Some(static_ann) = self.func(func_idx).return_annotations.get(ret_index) {
                                fn is_projection_placeholder(v: &ValueType) -> bool {
                                    matches!(v, ValueType::Any | ValueType::Nil)
                                }
                                let concrete = match static_ann {
                                    v if is_projection_placeholder(v) => None,
                                    ValueType::Union(parts) => {
                                        let filtered: Vec<_> = parts.iter()
                                            .filter(|p| !is_projection_placeholder(p))
                                            .cloned().collect();
                                        if filtered.is_empty() { None }
                                        else { Some(ValueType::make_union(filtered)) }
                                    }
                                    other => Some(other.clone()),
                                };
                                if let Some(ann) = concrete {
                                    ValueType::make_union(vec![vt, ann])
                                } else { vt }
                            } else { vt }
                        } else { vt };
                        // For expansion slots, shorter return-only overloads
                        // (e.g. pcall's `(false, string)`) produce nil at
                        // positions past their arity. Union with nil so the
                        // un-narrowed type reflects this.
                        let vt = if is_expansion
                            && self.func(func_idx).return_overload_may_nil(ret_index)
                            && !vt.contains_nil()
                            && !matches!(vt, ValueType::Any)
                        {
                            ValueType::make_union(vec![vt, ValueType::Nil])
                        } else { vt };
                        if let Some(cr) = self.ir.call_resolutions.get_mut(&expr_id) {
                            cr.projected_f_idx = Some(f_idx);
                            cr.is_expansion = is_expansion;
                        }
                        // Cache generic subs for resolve_overload_narrow to
                        // resolve projections during sibling narrowing.
                        if !generic_subs.is_empty() {
                            self.call_site_generic_subs.entry(*func)
                                .or_insert_with(|| generic_subs.clone());
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

        // When multiple overloads tied at zero mismatches, use their unioned return type.
        // Also apply return_overloads_may_nil so return-only overloads can contribute nil.
        if let Some(rt) = ambiguous_overload_ret_type {
            let rt = if !generic_subs.is_empty() {
                self.substitute_generics_deep(&rt, &generic_subs)
            } else {
                rt
            };
            if return_overloads_may_nil && !rt.contains_nil() && !matches!(rt, ValueType::Any) {
                return Some(ValueType::make_union(vec![rt, ValueType::Nil]));
            }
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
                self.projection_deferred = false;
                let substituted = self.substitute_generics_deep(ret_vt, &generic_subs);
                if self.projection_deferred {
                    // A projection (returns<F>/params<F>) couldn't be resolved
                    // because the bound F's type isn't available yet. Defer
                    // resolution so the fixpoint loop retries on the next iteration.
                    self.projection_deferred = false;
                    return None;
                }
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

        // When `@return` annotations exist and no overload matched, use the
        // annotation directly. The body's `FunctionRet` symbols may also exist
        // in `func.rets` (from build_ir walking the body), but the annotation
        // is authoritative — mixing body-inferred types in would widen the
        // declared return type.
        let func_return_annotations = &self.func(func_idx).return_annotations;
        let has_return_annotations = !func_return_annotations.is_empty();
        let synthesized_return_only = !has_return_annotations
            && self.func(func_idx).overloads.iter().any(|o| o.is_return_only);
        let ret_type = if has_return_annotations {
            let has_vararg_return = self.func(func_idx).has_vararg_return;
            func_return_annotations.get(ret_index).cloned()
                .or_else(|| {
                    if has_vararg_return {
                        func_return_annotations.last().cloned()
                    } else {
                        None
                    }
                })
        } else if synthesized_return_only {
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
            // Substitute implicit generics (pass-through param TypeVariables)
            // bound from the caller's argument types. Cache the subs so
            // resolve_overload_narrow can apply them during sibling narrowing.
            let return_only_types = if generic_subs.is_empty() {
                return_only_types
            } else {
                self.call_site_generic_subs.insert(*func, generic_subs.clone());
                return_only_types.into_iter()
                    .map(|t| self.substitute_generics_deep(&t, &generic_subs))
                    .collect()
            };
            Some(ValueType::make_union(return_only_types))
        } else {
            // Walk every `FunctionRet` symbol in `func.rets` rather than
            // looking up just the body-scope one. Each `return` registers
            // its symbol at its own scope (if/else/for/while/...), and a
            // body-scope-only lookup loses both pure-branched returns
            // (no body-scope return at all) and the branched contributions
            // to mixed body+branched returns.
            let slot_type = super::queries::return_type_at_slot(
                &self.ir,
                &self.func(func_idx).rets,
                ret_index,
            );
            // Tail-call passthrough: when this slot has no direct symbol but
            // the function has pure tail calls at slot 0, resolve through to
            // the callee's return at the same slot. This handles the common
            // pattern `function f() return g() end` where g returns multiple
            // values — without @return annotations, f only has FunctionRet at
            // slot 0, so higher slots would otherwise be lost.
            //
            // This complements `expand_resolved_tail_call_returns` (which
            // creates proper FunctionRet symbols in the stall-recovery phase).
            // The passthrough is needed during the inner fixpoint loop when a
            // caller already has FunctionRet at slot > 0 (from Phase 1's
            // `expand_tail_call_returns`) but the tail-call wrapper hasn't been
            // expanded yet by the Phase 2 pass (which runs only on stall).
            if slot_type.is_none() && ret_index > 0 {
                self.tail_call_passthrough_return(func_idx, ret_index)
            } else {
                slot_type
            }
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
        // Inferred return (no `@return` annotation): propagate the type
        // arguments of the returned expression so callers can re-substitute
        // class type vars through the call chain. Without this, a function
        // like `function f() return x end` (where `x` is `Schema<string>`)
        // loses the `<string>` binding at the call site because the inferred
        // return type carries only the bare class table, with type args
        // tracked out-of-band in `call_type_args` / `version.type_args`.
        if ret_index == 0
            && !has_return_annotations
            && !synthesized_return_only
            && !self.call_type_args.contains_key(&expr_id)
            && !self.func(func_idx).rets.is_empty()
            && matches!(ret_type, Some(ValueType::Table(Some(_))))
        {
            let ret_syms: Vec<SymbolIndex> = self.func(func_idx).rets.clone();
            for sym_idx in ret_syms {
                if !matches!(&self.sym(sym_idx).id, SymbolIdentifier::FunctionRet(_, 0)) {
                    continue;
                }
                let Some(src_expr) = self.sym(sym_idx).versions.first().and_then(|v| v.type_source)
                else {
                    continue;
                };
                let args = self.get_expr_type_args(src_expr);
                if !args.is_empty() {
                    self.call_type_args.insert(expr_id, args);
                    break;
                }
            }
        }
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
                self.ir.tables[tbl_idx.val()].call_func_is_metamethod = true;
                if let Some(&self_sym) = self.func(func_idx).args.first()
                    && !self_sym.is_external()
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
            // Unwrap narrowing wrappers (StripNil/StripFalsy/Grouped) that may
            // have been applied by condition-based narrowing.
            let unwrapped = match &expr {
                Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::Grouped(inner) => {
                    self.ir.exprs.get(inner.val()).cloned()
                }
                _ => None,
            };
            let check_expr = unwrapped.as_ref().unwrap_or(&expr);
            let base_expr = match check_expr {
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

    /// When a function has no return symbol at the requested slot but has pure
    /// tail-call returns at slot 0, resolve through to the callee's return at
    /// the same slot. Handles `function f() return g() end` where g returns
    /// multiple values.
    ///
    /// Mutual recursion (A tail-calls B, B tail-calls A) is safe because
    /// `resolve_expr` tracks in-progress resolutions — re-entering an
    /// expression already on the resolution stack returns `None`, breaking
    /// the cycle.
    fn tail_call_passthrough_return(
        &mut self,
        func_idx: FunctionIndex,
        ret_index: usize,
    ) -> Option<ValueType> {
        use std::collections::BTreeMap;

        // Only handle local functions (external functions have annotations)
        if func_idx.is_external() { return None; }

        let rets = self.func(func_idx).rets.clone();
        if rets.is_empty() { return None; }

        // Group rets by DefNode to identify return statements.
        let mut groups: BTreeMap<(u32, u32), Vec<(usize, SymbolIndex)>> = BTreeMap::new();
        for &sym_idx in &rets {
            if sym_idx.is_external() { continue; }
            let sym = self.sym(sym_idx);
            let SymbolIdentifier::FunctionRet(_, slot) = sym.id else { continue };
            let Some(ver) = sym.versions.first() else { continue };
            let key = (ver.def_node.start, ver.def_node.end);
            groups.entry(key).or_default().push((slot, sym_idx));
        }

        // Only proceed if the max arity across all groups is exactly 1
        // (single-slot returns). If any group already has arity >= 2, the
        // expand_tail_call_returns pass should have handled it.
        let max_arity = groups.values().map(|g| g.len()).max().unwrap_or(0);
        if max_arity > 1 { return None; }

        // For each group that is a pure tail call at slot 0, resolve the callee
        // and look up its return at the requested slot.
        let mut acc: Option<ValueType> = None;
        for group in groups.values() {
            if group.len() != 1 { continue; }
            let &(slot, sym_idx) = &group[0];
            if slot != 0 { continue; }

            let sym = self.sym(sym_idx);
            let Some(ver) = sym.versions.first() else { continue };
            let Some(type_source) = ver.type_source else { continue };

            // Check if this is a function call expression
            let expr = self.ir.expr(type_source).clone();
            let callee_func_expr = match &expr {
                Expr::FunctionCall { func, ret_index: 0, .. } => *func,
                _ => continue,
            };

            // Resolve the callee's function type to get its function index
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

            // Look up the callee's return at the requested slot
            let callee_rets = self.func(callee_func_idx).rets.clone();
            let callee_ret_type = if !self.func(callee_func_idx).return_annotations.is_empty() {
                // Callee has annotations — use them directly
                self.func(callee_func_idx).return_annotations.get(ret_index).cloned()
            } else {
                super::queries::return_type_at_slot(&self.ir, &callee_rets, ret_index)
            };

            if let Some(vt) = callee_ret_type {
                acc = Some(match acc.take() {
                    Some(prev) => self.ir.dedupe_union_tables(
                        ValueType::make_union(vec![prev, vt])
                    ),
                    None => vt,
                });
            }
        }
        acc
    }

    fn extract_table_from_type(&self, vt: &ValueType) -> Option<TableIndex> {
        match vt.strip_opaque() {
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
        let call_func_is_metamethod = source.call_func_is_metamethod;
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
                description: None,
                from_scan: false,
            });
        }

        let old_has_source_fields = built_class_name.as_ref()
            .and_then(|name| self.ir.classes.get(name))
            .map(|&idx| self.table(idx).has_source_fields)
            .unwrap_or(false);

        // Only update ir.classes when the built table has a distinct name from the
        // schema (i.e. it was assigned by @built-name). When the built table inherits
        // the schema's own class_name, overwriting ir.classes would corrupt the original
        // class definition and cause false undefined-field diagnostics on chain methods.
        let update_classes = built_class_name.as_ref() != class_name.as_ref();

        let new_built_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo {
            fields: built_fields, class_name: built_class_name.clone(),
            parent_classes: built_parent_classes,
            has_source_fields: old_has_source_fields,
            ..Default::default()
        });

        if update_classes
            && let Some(ref name) = built_class_name
            && self.ir.classes.contains_key(name) {
                self.ir.classes.insert(name.clone(), new_built_idx);
            }

        let new_schema_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo {
            fields: schema_fields, class_name, class_type_params,
            parent_classes, accessors, call_func, call_func_is_metamethod,
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
        let call_func_is_metamethod = source.call_func_is_metamethod;
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
        let mut overlay_has_source_fields = false;
        if let Some(&overlay_idx) = self.ir.classes.get(class_name)
            && !overlay_idx.is_external() {
                let overlay_fields: Vec<(String, FieldInfo)> = self.ir.tables[overlay_idx.val()].fields.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (fname, fi) in overlay_fields {
                    built_fields.insert(fname, fi);
                }
                overlay_correlated = self.ir.tables[overlay_idx.val()].correlated_groups.clone();
                overlay_has_source_fields = self.ir.tables[overlay_idx.val()].has_source_fields;
            }

        let new_built_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo {
            fields: built_fields, class_name: Some(class_name.to_string()),
            parent_classes: final_parents, correlated_groups: overlay_correlated,
            has_source_fields: overlay_has_source_fields,
            ..Default::default()
        });

        self.ir.classes.insert(class_name.to_string(), new_built_idx);

        let new_schema_idx = TableIndex(self.ir.tables.len());
        self.ir.tables.push(TableInfo {
            fields: schema_fields, class_name: schema_class_name,
            class_type_params, parent_classes, accessors, call_func, call_func_is_metamethod,
            built_table: Some(new_built_idx), metatable_index, ..Default::default()
        });

        new_schema_idx
    }

    /// Resolve an annotation type, substituting class-level type params when available.
    /// Used by inline callback param/return propagation (Stage 1 of resolve_function_call).
    /// Uses the `_mut_gen` resolver so structural types (`T[]`, `table<K,V>`, `fun(x: T)`)
    /// materialize a `TableInfo`/`Function` whose nested `TypeVariable`s are then
    /// substituted by `substitute_generics_deep`. Top-level type params (`T`, `T|nil`)
    /// work too.
    fn resolve_annotation_with_class_generics(
        &mut self,
        ann: &crate::annotations::AnnotationType,
        class_gen_context: &[(String, Option<String>)],
        class_type_param_subs: &HashMap<String, ValueType>,
    ) -> Option<ValueType> {
        if class_gen_context.is_empty() {
            self.resolve_annotation_type(ann)
        } else {
            self.resolve_annotation_type_mut_gen(ann, class_gen_context)
                .map(|vt| self.substitute_generics_deep(&vt, class_type_param_subs))
        }
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

    /// Extract parameterized class constraints from a raw parameter annotation.
    /// For each `Class<...>` form (directly or as a union member), resolve the
    /// class name to its table index and resolve+substitute its type arguments.
    /// Builtin parameterized forms (`table<K,V>`, `expression<C,R>`, etc.) and
    /// non-class names are skipped. Used for generic type-argument variance checks.
    fn param_parameterized_constraints(
        &mut self,
        ann: &crate::annotations::AnnotationType,
        generic_subs: &HashMap<String, ValueType>,
    ) -> Vec<(TableIndex, Vec<ValueType>)> {
        use crate::annotations::AnnotationType;
        let members: Vec<&AnnotationType> = match ann {
            AnnotationType::Union(m) => m.iter().collect(),
            other => vec![other],
        };
        let mut out = Vec::new();
        for m in members {
            if let AnnotationType::Parameterized(name, args) = m {
                let Some(&table_idx) = self.ir.classes.get(name)
                    .or_else(|| self.ir.ext.classes.get(name)) else { continue };
                let resolved: Vec<ValueType> = args.iter()
                    .map(|a| {
                        let vt = self.resolve_annotation_type(a).unwrap_or(ValueType::Any);
                        if generic_subs.is_empty() { vt }
                        else { self.substitute_generics_deep(&vt, generic_subs) }
                    })
                    .collect();
                out.push((table_idx, resolved));
            }
        }
        out
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
                // Flatten nested intersections (from variadic generic substitution)
                let flat: Vec<_> = subst.into_iter()
                    .flat_map(|t| match t {
                        ValueType::Intersection(inner) => inner,
                        other => vec![other],
                    })
                    .collect();
                // Deduplicate anonymous empty tables and same-class tables
                let mut deduped: Vec<ValueType> = Vec::with_capacity(flat.len());
                let mut seen_anon = false;
                let mut seen_class_names: Vec<String> = Vec::new();
                for m in flat {
                    match &m {
                        ValueType::Table(Some(idx)) => {
                            if let Some(cn) = self.table(*idx).class_name.clone() {
                                if seen_class_names.iter().any(|n| n == &cn) { continue; }
                                seen_class_names.push(cn);
                            } else if self.ir.is_anonymous_empty_table(*idx) {
                                if seen_anon { continue; }
                                seen_anon = true;
                            }
                            deduped.push(m);
                        }
                        _ => deduped.push(m),
                    }
                }
                match deduped.len() {
                    0 => ValueType::Table(None),
                    1 => deduped.into_iter().next().unwrap(),
                    _ => ValueType::Intersection(deduped),
                }
            }
            ValueType::Function(Some(func_idx)) => {
                let func = self.func(*func_idx);
                let has_tv = func.args.iter().any(|&sym_idx| {
                    self.sym(sym_idx).versions.iter()
                        .any(|v| v.resolved_type.as_ref().is_some_and(|t| t.contains_type_variable()))
                }) || func.return_annotations.iter().any(|vt| vt.contains_type_variable());
                let has_projections = !func.return_projections.is_empty()
                    || func.vararg_projection.is_some();
                if !has_tv && !has_projections {
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
                let vararg_proj = func.vararg_projection.clone();
                let ret_projections = func.return_projections.clone();
                let arg_infos: Vec<(SymbolIdentifier, Option<ValueType>)> = func.args.iter().map(|&sym_idx| {
                    let sym = self.sym(sym_idx);
                    let resolved = sym.versions.first().and_then(|v| v.resolved_type.clone());
                    (sym.id.clone(), resolved)
                }).collect();

                let func_scope = self.ir.insert_scope(None);
                let mut new_args = Vec::new();
                let mut new_param_annotations = Vec::new();
                let mut new_param_optional = Vec::new();

                // Add the original (non-projected) named args first
                for (idx, (id, resolved)) in arg_infos.iter().enumerate() {
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
                    if let Some(pa) = param_annotations.get(idx) {
                        new_param_annotations.push(pa.clone());
                    }
                    new_param_optional.push(param_optional.get(idx).copied().unwrap_or(false));
                }

                // Then expand params<F> projection: append F's params after named args
                let mut expanded_vararg = false;
                if let Some(crate::types::ProjectionKind::Params(ref proj_name)) = vararg_proj
                    && let Some(ValueType::Function(Some(f_idx))) = subs.get(proj_name) {
                        let f_func = self.func(*f_idx);
                        let f_arg_infos: Vec<(SymbolIdentifier, Option<ValueType>, bool)> = f_func.args.iter().enumerate().map(|(i, &sym_idx)| {
                            let sym = self.sym(sym_idx);
                            let resolved = sym.versions.first().and_then(|v| v.resolved_type.clone());
                            let optional = f_func.param_optional.get(i).copied().unwrap_or(false);
                            (sym.id.clone(), resolved, optional)
                        }).collect();
                        let f_param_annotations = f_func.param_annotations.clone();
                        for (i, (id, resolved, optional)) in f_arg_infos.iter().enumerate() {
                            let sym_idx = SymbolIndex(self.ir.symbols.len());
                            let order = self.ir.next_order();
                            self.ir.symbols.push(Symbol {
                                id: id.clone(),
                                scope_idx: func_scope,
                                versions: vec![SymbolVersion {
                                    def_node: dummy_node,
                                    type_source: None,
                                    resolved_type: resolved.clone(),
                                    type_args: Vec::new(),
                                    created_in_scope: func_scope,
                                    creation_order: order,
                                    original_type_source: None,
                                }],
                                flavor_guard: 0,
                            });
                            new_args.push(sym_idx);
                            if let Some(pa) = f_param_annotations.get(i) {
                                new_param_annotations.push(pa.clone());
                            }
                            new_param_optional.push(*optional);
                        }
                        expanded_vararg = true;
                    }

                // Resolve return projections: returns<F> → F's return types
                let new_func_idx = FunctionIndex(self.ir.functions.len());
                let subst_return_annotations: Vec<ValueType> = if !ret_projections.is_empty() {
                    let mut result = Vec::new();
                    for (i, ret_vt) in return_annotations.iter().enumerate() {
                        if let Some(crate::types::ProjectionKind::Return(proj_name, _)) = ret_projections.get(&i) {
                            if let Some(ValueType::Function(Some(f_idx))) = subs.get(proj_name) {
                                let f_returns = self.func(*f_idx).return_annotations.clone();
                                if f_returns.is_empty() {
                                    // Check resolved ret symbols
                                    let f_scope = self.func(*f_idx).scope;
                                    let ret_id = SymbolIdentifier::FunctionRet(*f_idx, 0);
                                    if let Some(si) = self.get_symbol(&ret_id, f_scope) {
                                        if let Some(resolved) = self.sym(si).versions.first().and_then(|v| v.resolved_type.clone()) {
                                            result.push(resolved);
                                        } else {
                                            // F's return type not yet resolved — defer
                                            self.projection_deferred = true;
                                            result.push(ValueType::Any);
                                        }
                                    } else {
                                        result.push(ValueType::Any);
                                    }
                                } else {
                                    result.extend(f_returns);
                                }
                            } else {
                                result.push(self.substitute_generics_deep(ret_vt, subs));
                            }
                        } else {
                            result.push(self.substitute_generics_deep(ret_vt, subs));
                        }
                    }
                    result
                } else {
                    return_annotations.iter()
                        .map(|t| self.substitute_generics_deep(t, subs))
                        .collect()
                };
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

                let effective_is_vararg = if expanded_vararg { false } else { is_vararg };

                self.ir.functions.push(Function {
                    def_node: dummy_node,
                    scope: func_scope,
                    args: new_args,
                    rets: new_rets,
                    return_annotations: subst_return_annotations,
                    return_annotations_raw,
                    return_labels,
                    return_descriptions: Vec::new(),
                    overloads: Vec::new(),
                    doc: None,
                    deprecated: false,
                    nodiscard: false,
                    generics: Vec::new(),
                    generic_constraints_raw: Vec::new(),
                    param_annotations: new_param_annotations,
                    param_descriptions: Vec::new(),
                    defclass: None,
                    defclass_parent: None,
                    is_vararg: effective_is_vararg,
                    vararg_annotation: None,
                    vararg_description: None,
                    param_optional: new_param_optional,
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
                    event_params: None,
                    narrows_arg: None,
                    requires_constraints: Vec::new(),
                    returns_self_type_args: None,
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
                let call_func_is_metamethod = table.call_func_is_metamethod;
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
                        description: fi.description.clone(),
                        from_scan: false,
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
                        description: fi.description,
                        from_scan: false,
                    })
                }).collect();
                let new_table_idx = TableIndex(self.ir.tables.len());
                self.ir.tables.push(TableInfo {
                    fields, class_name, class_type_params, parent_classes,
                    array_fields, key_type: new_key, value_type: new_val,
                    accessors, call_func, call_func_is_metamethod, metatable_index, ..Default::default()
                });
                ValueType::Table(Some(new_table_idx))
            }
            ValueType::OpaqueAlias(name, inner) => {
                ValueType::OpaqueAlias(name.clone(), Box::new(self.substitute_generics_deep(inner, subs)))
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
            self.resolved_expr_cache.fill(None);
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
                    if op == Operator::ArrayLength
                        && let Some(s) = self.candidate_ref_in(operand, candidates) {
                            record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, s, ValueType::Union(vec![ValueType::String(None), ValueType::Table(None)]));
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
                            if self.type_contains_type_variable_deep(&substituted) {
                                // The full generic type can't be used, but the structural
                                // shape (e.g. T[] → table) is still a valid constraint.
                                if matches!(substituted, ValueType::Table(_)) {
                                    let structural = ValueType::Table(None);
                                    if sig_param.optional {
                                        narrowing_hints.entry(sym).or_default().push(structural);
                                    } else {
                                        record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, sym, structural);
                                    }
                                }
                                continue;
                            }
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
                    if let Some(sym) = self.candidate_ref_in(table, candidates) {
                        record_hint(&mut baseline_hints, &mut narrowing_hints, conditional, sym, ValueType::Table(None));
                    }
                }
                Expr::FieldAccess { table, .. } => {
                    if let Some(sym) = self.candidate_ref_in(table, candidates) {
                        narrowing_hints.entry(sym).or_default().push(ValueType::Table(None));
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

    /// Infer parameter types for inline functions defined inside table constructors
    /// whose expected type is a known class. E.g. `---@type X / local x = { field1 = function(self, arg) end }`
    /// or `RegisterX({ field1 = function(self, arg) end })` where the param is `@param x X`.
    pub(super) fn infer_table_constructor_field_params(&mut self) -> bool {
        // Phase 1: Collect (class_table_idx, constructor_table_idx) pairs
        let mut tc_pairs: Vec<(TableIndex, TableIndex)> = Vec::new();

        // Case 1: @type X on a local variable
        for (&sym_idx, expected_type) in &self.ir.symbol_type_annotations {
            let mut class_indices = Vec::new();
            extract_all_table_indices_from_type(expected_type, &mut class_indices);
            if class_indices.is_empty() { continue; }
            let sym = &self.ir.symbols[sym_idx.val()];
            for ver in &sym.versions {
                // original_type_source holds the table constructor when type_source was overwritten by @type
                let tc_expr = ver.original_type_source.or(ver.type_source);
                let Some(tc_expr) = tc_expr else { continue };
                if tc_expr.is_external() { continue; }
                if let Expr::TableConstructor(ctor_idx) = *self.ir.expr(tc_expr) {
                    if ctor_idx.is_external() { continue; }
                    for &class_idx in &class_indices {
                        tc_pairs.push((class_idx, ctor_idx));
                    }
                }
            }
        }

        // Case 2: Function call arguments
        for resolution in self.ir.call_resolutions.values() {
            for arg in &resolution.expected_args {
                let Some(expected_type) = &arg.expected_type else { continue };
                let mut class_indices = Vec::new();
                extract_all_table_indices_from_type(expected_type, &mut class_indices);
                if class_indices.is_empty() { continue; }
                if arg.arg_expr.is_external() { continue; }
                if let Expr::TableConstructor(ctor_idx) = *self.ir.expr(arg.arg_expr) {
                    if ctor_idx.is_external() { continue; }
                    for &class_idx in &class_indices {
                        tc_pairs.push((class_idx, ctor_idx));
                    }
                }
            }
        }

        // Case 3: Bracket assignments on table<K, V> typed tables
        // e.g. `items[1] = { ... }` where items is `table<integer, SomeClass>`
        let parent_tables: Vec<TableIndex> = self.ir.bracket_key_fields.keys().copied().collect();
        for parent_idx in parent_tables {
            let value_type = self.ir.tables[parent_idx.val()].value_type.clone();
            let mut class_indices = Vec::new();
            if let Some(vt) = value_type.as_ref() {
                extract_all_table_indices_from_type(vt, &mut class_indices);
            }
            if class_indices.is_empty() { continue; }
            // Only if value_type is annotated (authoritative), not inferred
            if !self.ir.tables[parent_idx.val()].value_type_annotated { continue; }
            let Some(bracket_fields) = self.ir.bracket_key_fields.get(&parent_idx).cloned() else { continue };
            for (_key_expr, val_expr) in &bracket_fields {
                if val_expr.is_external() { continue; }
                if let Expr::TableConstructor(ctor_idx) = *self.ir.expr(*val_expr) {
                    if ctor_idx.is_external() { continue; }
                    for &class_idx in &class_indices {
                        tc_pairs.push((class_idx, ctor_idx));
                    }
                }
            }
        }

        // Case 4: Deferred bracket assignments (table resolved in Phase 2)
        // e.g. `local NPCs = private.Data.NPCs; NPCs[1] = { ... }` where
        // NPCs resolves to table<integer, SomeClass> after cross-file resolution.
        // The resolved table may be external (from workspace scanning), so we use
        // self.table() which routes local/external indices correctly.
        for (root_name, scope_idx, val_expr) in std::mem::take(&mut self.ir.pending_bracket_assigns) {
            let mut table_idx_opt = self.ir.find_table_for_symbol(&root_name, scope_idx);
            if table_idx_opt.is_none() {
                // Resolve field chain: for `local NPCs = ns.NPCs`, walk
                // the FieldAccess chain to find the field's annotation type,
                // which preserves Table(Some(idx)) for table<K,V> fields.
                if let Some(sym_idx) = self.ir.get_symbol(&crate::types::SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                    let ver_idx = self.ir.version_for_scope(sym_idx, scope_idx);
                    if let Some(ts) = self.sym(sym_idx).versions[ver_idx].type_source {
                        table_idx_opt = self.resolve_field_access_table(ts);
                    }
                }
            }
            let Some(table_idx) = table_idx_opt else { continue };
            let value_type = self.table(table_idx).value_type.clone();
            let mut class_indices = Vec::new();
            if let Some(vt) = value_type.as_ref() {
                extract_all_table_indices_from_type(vt, &mut class_indices);
            }
            if class_indices.is_empty() { continue; }
            // For same-file tables we check value_type_annotated, but for
            // external (cross-file) tables that flag is not propagated.
            // Instead, verify the class has a name (was declared via @class).
            if !table_idx.is_external() && !self.table(table_idx).value_type_annotated {
                continue;
            }
            // Filter out external classes without a name
            if table_idx.is_external() {
                class_indices.retain(|&idx| self.table(idx).class_name.is_some());
                if class_indices.is_empty() { continue; }
            }
            if val_expr.is_external() { continue; }
            if let Expr::TableConstructor(ctor_idx) = *self.ir.expr(val_expr) {
                if ctor_idx.is_external() { continue; }
                for &class_idx in &class_indices {
                    tc_pairs.push((class_idx, ctor_idx));
                }
            }
        }

        // Case 5: Field assignments with table<K, V> type annotations
        // e.g. `AddOn.RaceIcons = { Human = { ... } }` where RaceIcons has
        // `@type table<string, RaceGender>` annotation.
        for fa in &self.ir.field_assignments {
            if fa.actual_expr.is_external() { continue; }
            let Some(field_info) = self.table(fa.table_idx).fields.get(&fa.field_name) else { continue };
            let Some(ref ann) = field_info.annotation else { continue };
            let mut class_indices = Vec::new();
            extract_all_table_indices_from_type(ann, &mut class_indices);
            if class_indices.is_empty() { continue; }
            if let Expr::TableConstructor(ctor_idx) = *self.ir.expr(fa.actual_expr) {
                if ctor_idx.is_external() { continue; }
                for &class_idx in &class_indices {
                    tc_pairs.push((class_idx, ctor_idx));
                }
            }
        }

        // Case 6: Named fields in table<K, V> typed constructors
        // e.g. `local t: table<string, SomeClass> = { Human = { ... } }`
        // The inner `{ ... }` should be checked against SomeClass.
        // Look at tc_pairs from Cases 1-5 where the "class" is a table<K,V> type
        // (has value_type pointing to a real class). Check the constructor's named
        // field values for inner table constructors.
        // NOTE: Only processes one level of nesting (tc_pairs snapshot from Cases 1-5).
        // Deeper nesting like `table<K, table<K2, SomeClass>>` is not checked.
        let existing_len = tc_pairs.len();
        for i in 0..existing_len {
            let (type_table_idx, ctor_idx) = tc_pairs[i];
            if ctor_idx.is_external() { continue; }
            let type_table = self.table(type_table_idx);
            // For local tables, require value_type_annotated. For external tables,
            // the flag is not serialized — instead verify the value class has a name.
            if !type_table_idx.is_external() && !type_table.value_type_annotated { continue; }
            let vt = type_table.value_type.clone();
            let mut value_class_indices = Vec::new();
            if let Some(vt) = vt.as_ref() {
                extract_all_table_indices_from_type(vt, &mut value_class_indices);
            }
            if value_class_indices.is_empty() { continue; }
            // Filter out external classes without a name
            if type_table_idx.is_external() {
                value_class_indices.retain(|&idx| self.table(idx).class_name.is_some());
                if value_class_indices.is_empty() { continue; }
            }
            let ctor_table = &self.ir.tables[ctor_idx.val()];
            let field_exprs: Vec<ExprId> = ctor_table.fields.values().map(|fi| fi.expr).collect();
            for expr_id in field_exprs {
                if expr_id.is_external() { continue; }
                if let Expr::TableConstructor(inner_ctor_idx) = *self.ir.expr(expr_id) {
                    if inner_ctor_idx.is_external() { continue; }
                    for &class_idx in &value_class_indices {
                        tc_pairs.push((class_idx, inner_ctor_idx));
                    }
                }
            }
        }

        // Record expected classes for each constructor (used by completions and missing-fields)
        for &(class_idx, ctor_idx) in &tc_pairs {
            self.ir.tc_expected_class.entry(ctor_idx).or_default().push(class_idx);
        }
        // Deduplicate class lists (same class can appear from multiple code paths)
        for classes in self.ir.tc_expected_class.values_mut() {
            classes.sort_unstable();
            classes.dedup();
        }

        if tc_pairs.is_empty() { return false; }

        // Phase 2: Collect param type updates
        let mut param_updates: Vec<(SymbolIndex, ValueType)> = Vec::new();

        for (class_idx, ctor_idx) in tc_pairs {
            let ctor_fields: Vec<(String, ExprId)> = self.ir.tables[ctor_idx.val()]
                .fields.iter()
                .map(|(n, fi)| (n.clone(), fi.expr))
                .collect();

            for (field_name, field_expr) in ctor_fields {
                if field_expr.is_external() { continue; }
                let Expr::FunctionDef(inline_func_idx) = *self.ir.expr(field_expr) else { continue };

                // Look up the class field's annotation and extract the function type.
                // Uses get_field (walks parent classes + metatables) so inherited
                // fields are also matched.
                let field_annotation = self.ir.get_field(class_idx, &field_name)
                    .and_then(|fi| fi.annotation.clone());
                let Some(expected_func_idx) = extract_function_idx_from_type(field_annotation.as_ref()) else { continue };

                // Collect expected param types from the annotation function
                let expected_args = self.ir.func(expected_func_idx).args.clone();
                let inline_args = self.ir.func(inline_func_idx).args.clone();

                for (i, &inline_sym_idx) in inline_args.iter().enumerate() {
                    if inline_sym_idx.is_external() { continue; }
                    // Skip if already resolved
                    let already_set = self.ir.symbols[inline_sym_idx.val()].versions.first()
                        .and_then(|v| v.resolved_type.as_ref()).is_some();
                    if already_set { continue; }

                    let Some(&expected_sym_idx) = expected_args.get(i) else { continue };
                    let expected_type = self.sym(expected_sym_idx).versions.first()
                        .and_then(|v| v.resolved_type.clone());
                    let Some(expected_type) = expected_type else { continue };
                    if matches!(expected_type, ValueType::Any | ValueType::Nil) { continue; }

                    param_updates.push((inline_sym_idx, expected_type));
                }
            }
        }

        // Phase 3: Apply updates
        let mut progress = false;
        for (sym_idx, expected_type) in param_updates {
            if let Some(ver) = self.ir.symbols[sym_idx.val()].versions.first_mut()
                && ver.resolved_type.is_none() {
                    ver.resolved_type = Some(expected_type);
                    progress = true;
                }
        }

        progress
    }

    /// Walk a FieldAccess chain to find the terminal field's annotation type.
    /// Returns `Some(table_idx)` when the annotation is `Table(Some(idx))`.
    /// This preserves `table<K,V>` info that `resolve_expr` loses (it returns
    /// `Table(None)` for cross-file field access).
    fn resolve_field_access_table(&mut self, expr_id: ExprId) -> Option<TableIndex> {
        let expr = self.ir.expr(expr_id).clone();
        match expr {
            Expr::FieldAccess { table, ref field, .. } => {
                let base_type = self.resolve_expr(table)?;
                let base_idx = extract_table_idx_from_type(&base_type)?;
                let fi = self.ir.get_field(base_idx, field)?;
                if let Some(ann) = &fi.annotation
                    && let Some(idx) = extract_table_idx_from_type(ann) {
                        return Some(idx);
                }
                // Fallback: resolve the field's expr to find its table index
                self.ir.find_table_index(fi.expr)
            }
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let ver = &self.sym(sym_idx).versions[ver_idx];
                match &ver.resolved_type {
                    Some(ValueType::Table(Some(idx))) => Some(*idx),
                    _ => ver.type_source.and_then(|ts| self.ir.find_table_index(ts)),
                }
            }
            _ => None,
        }
    }

    /// Resolve a backtick string literal to a type. Supports comma-separated
    /// lists (e.g. "Tmpl1, Tmpl2") which produce an intersection type.
    fn resolve_backtick_class_name(&self, class_name: &str) -> ValueType {
        if !class_name.contains(',') {
            return self.resolve_single_class_name(class_name);
        }
        let parts: Vec<ValueType> = class_name.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|name| self.resolve_single_class_name(name))
            // Filter out unknown templates (Any) — a typo in one template
            // shouldn't degrade the type of the others.
            .filter(|t| !matches!(t, ValueType::Any))
            .collect();
        match parts.len() {
            0 => ValueType::Any,
            1 => parts.into_iter().next().unwrap(),
            _ => ValueType::Intersection(parts),
        }
    }

    /// Resolve a single class name string to a type (primitive or class lookup).
    fn resolve_single_class_name(&self, class_name: &str) -> ValueType {
        crate::annotations::resolve_primitive_type_name(class_name)
            .or_else(|| self.ir.classes.get(class_name).copied()
                .or_else(|| self.ir.ext.classes.get(class_name).copied())
                .map(|idx| ValueType::Table(Some(idx))))
            .unwrap_or(ValueType::Any)
    }

    /// Trace through a single SymbolRef expression to find its string literal value.
    /// When a variable like `MAJOR` holds `"SomeLib-1.0"`, its type_source
    /// expression points to the original string literal in `string_literals`.
    ///
    /// Only follows one level of indirection (one SymbolRef hop). Chained
    /// assignments (`local A = "Lib"; local B = A; f(B)`) are not traced.
    fn resolve_string_literal_through_expr(&self, expr_id: &ExprId) -> Option<String> {
        let expr = if expr_id.is_external() {
            self.ir.ext.exprs.get(expr_id.ext_offset())?
        } else {
            self.ir.exprs.get(expr_id.val())?
        };
        match expr {
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let symbol = self.sym(*sym_idx);
                let version = symbol.versions.get(*ver_idx)?;
                let type_source = version.type_source?;
                if type_source.is_external() {
                    self.ir.ext.string_literals.get(&type_source).cloned()
                } else {
                    self.ir.string_literals.get(&type_source).cloned()
                }
            }
            Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::Grouped(inner) => {
                self.resolve_string_literal_through_expr(inner)
            }
            _ => None,
        }
    }

    /// Resolve a backtick generic argument to a type. Three-step resolution:
    /// 1. Direct string literal lookup on the expression
    /// 2. Trace through SymbolRef to find the variable's string literal value
    /// 3. Fall back to `resolve_string_type_as_class`, or `Any` for bare strings
    fn resolve_backtick_arg(&self, arg_expr_id: &ExprId, arg_type: &ValueType) -> ValueType {
        if let Some(class_name) = self.ir.string_literals.get(arg_expr_id) {
            self.resolve_backtick_class_name(class_name)
        } else if let Some(class_name) = self.resolve_string_literal_through_expr(arg_expr_id) {
            self.resolve_backtick_class_name(&class_name)
        } else {
            // For string args that can't be resolved to a class, use Any (the backtick
            // says "type name" — if unknown, Any is better than string). For non-string
            // args (e.g. table variable passed to `T|\`T\``), preserve the original type.
            let fallback = if matches!(arg_type, ValueType::String(_)) {
                ValueType::Any
            } else {
                arg_type.clone()
            };
            self.resolve_string_type_as_class(arg_type).unwrap_or(fallback)
        }
    }

    fn resolve_string_type_as_class(&self, vt: &ValueType) -> Option<ValueType> {
        match vt {
            ValueType::String(Some(val)) => {
                match self.resolve_single_class_name(val) {
                    ValueType::Any => None,
                    resolved => Some(resolved),
                }
            }
            ValueType::Union(members) => {
                let resolved: Vec<ValueType> = members.iter().map(|m| {
                    self.resolve_string_type_as_class(m).unwrap_or_else(|| m.clone())
                }).collect();
                if resolved.iter().zip(members.iter()).all(|(r, m)| r == m) {
                    None
                } else {
                    Some(ValueType::make_union(resolved))
                }
            }
            _ => None,
        }
    }

    /// Look up a global function by name. Returns `ValueType::Function(Some(_))`
    /// if the name resolves to a function in scope 0 (file-level or external stubs).
    fn resolve_global_function_type(&self, name: &str) -> Option<ValueType> {
        let id = SymbolIdentifier::Name(name.to_string());
        let scope0 = ScopeIndex::from(0);
        let sym_idx = self.ir.get_symbol(&id, scope0)?;
        let sym = self.sym(sym_idx);
        let vt = sym.versions.last()?.resolved_type.as_ref()?;
        match vt {
            ValueType::Function(Some(_)) => Some(vt.clone()),
            _ => None,
        }
    }

    /// Resolve the class name from an `expression<C, R>` annotation's first type arg.
    /// `receiver_table_idx` is the table index of the method receiver (for resolving `self`).
    /// Resolve the context type parameter of `expression<C, R>` to table indices.
    /// Supports simple class names, `self`, and intersection types (`C1 & C2`).
    fn resolve_expression_tables(&self, class_ann: &crate::annotations::AnnotationType, receiver_table_idx: Option<TableIndex>) -> Vec<TableIndex> {
        match class_ann {
            crate::annotations::AnnotationType::Simple(name) => {
                if name == "self" {
                    receiver_table_idx.into_iter().collect()
                } else {
                    self.ir.classes.get(name.as_str()).copied()
                        .or_else(|| self.ir.ext.classes.get(name.as_str()).copied())
                        .into_iter().collect()
                }
            }
            crate::annotations::AnnotationType::Intersection(parts) => {
                parts.iter()
                    .flat_map(|part| self.resolve_expression_tables(part, receiver_table_idx))
                    .collect()
            }
            _ => Vec::new(),
        }
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

fn extract_table_idx_from_type(vt: &ValueType) -> Option<TableIndex> {
    match vt {
        ValueType::Table(Some(idx)) => Some(*idx),
        ValueType::Union(parts) => parts.iter().find_map(extract_table_idx_from_type),
        _ => None,
    }
}

fn extract_all_table_indices_from_type(vt: &ValueType, out: &mut Vec<TableIndex>) {
    match vt {
        ValueType::Table(Some(idx)) => out.push(*idx),
        ValueType::Union(parts) => {
            for p in parts {
                extract_all_table_indices_from_type(p, out);
            }
        }
        _ => {}
    }
}

fn extract_function_idx_from_type(vt: Option<&ValueType>) -> Option<FunctionIndex> {
    match vt? {
        ValueType::Function(Some(idx)) => Some(*idx),
        ValueType::Union(parts) => parts.iter().find_map(|p| {
            if let ValueType::Function(Some(idx)) = p { Some(*idx) } else { None }
        }),
        _ => None,
    }
}
