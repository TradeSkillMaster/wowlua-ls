use std::collections::HashMap;

use crate::ast::*;
use crate::types::*;
use super::Analysis;

// ── Type Resolution (Phase 2) ──────────────────────────────────────────────────

impl Analysis {
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
        loop {
            let prev_len = pending.len();
            pending.retain(|&(si, vi)| {
                let expr_id = self.ir.symbols[si].versions[vi].type_source.unwrap();
                if let Some(resolved) = self.resolve_expr(expr_id) {
                    self.ir.symbols[si].versions[vi].resolved_type = Some(resolved);
                    false
                } else {
                    true
                }
            });
            if pending.len() == prev_len {
                break;
            }
        }

        // Resolve function call exprs that weren't already resolved through symbols
        let resolved_exprs: std::collections::HashSet<ExprId> = self.ir.symbols.iter()
            .flat_map(|s| s.versions.iter())
            .filter(|v| v.resolved_type.is_some())
            .filter_map(|v| v.type_source)
            .collect();
        let call_exprs = self.deferred.call_exprs.clone();
        for expr_id in call_exprs {
            if !resolved_exprs.contains(&expr_id) {
                self.resolve_expr(expr_id);
            }
        }

        self.check_return_type_diagnostics();
        self.check_field_type_diagnostics();
        self.check_assign_type_diagnostics();
        self.check_access_diagnostics();
        self.check_nil_diagnostics();
        self.check_undefined_global_diagnostics();
        self.check_unused_local_diagnostics();
        self.check_duplicate_set_field_diagnostics();
        self.check_missing_return_diagnostics();
        self.check_diagnostic_codes();

        // Deduplicate diagnostics (resolve loop may emit the same diagnostic multiple times)
        {
            let mut seen = std::collections::HashSet::new();
            self.diagnostics.retain(|d| seen.insert((d.code, d.start, d.end)));
        }
    }

    pub(super) fn resolve_expr(&mut self, expr_id: ExprId) -> Option<ValueType> {
        // Cycle detection: if we're already resolving this expr, break the cycle
        if !self.resolving_exprs.insert(expr_id) {
            return None;
        }
        let result = self.resolve_expr_inner(expr_id);
        self.resolving_exprs.remove(&expr_id);
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
                        if op == Operator::Concatenate && t.can_concat_to_string() => Some(ValueType::String),
                    // Comparisons always yield boolean
                    _ if op.is_comparison() => Some(ValueType::Boolean(None)),
                    // `unknown and rhs` → rhs | false | nil (unknown could be truthy → rhs,
                    // or falsy → false/nil, the only two falsy values in Lua)
                    (None, Some(r)) if op == Operator::And => {
                        Some(ValueType::make_union(vec![r, ValueType::Boolean(Some(false)), ValueType::Nil]))
                    }
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
                            _ => None,
                        }
                    }
                    Operator::ArrayLength => Some(ValueType::Number),
                    _ => None,
                }
            }

            Expr::Grouped(inner) => self.resolve_expr(*inner),

            Expr::FunctionCall { func, args, arg_ranges, ret_index, call_range, discarded } => {
                let call_range = *call_range;
                let discarded = *discarded;
                let arg_ranges = arg_ranges.clone();
                // Resolve the function expression to get its type
                let func_type = self.resolve_expr(*func)?;
                let ValueType::Function(Some(func_idx)) = func_type else { return None };

                // Extract scalar fields without cloning the full Function struct
                let deprecated = self.func(func_idx).deprecated;
                let nodiscard = self.func(func_idx).nodiscard;
                let is_vararg = self.func(func_idx).is_vararg;
                let func_scope = self.func(func_idx).scope;
                let has_generics = !self.func(func_idx).generics.is_empty();
                let has_overloads = !self.func(func_idx).overloads.is_empty();
                // Clone only the Vecs we need unconditionally
                let func_args = self.func(func_idx).args.clone();
                // Defer conditional clones
                let overloads = if has_overloads { self.func(func_idx).overloads.clone() } else { Vec::new() };
                let param_optional = self.func(func_idx).param_optional.clone();
                let generics = if has_generics { self.func(func_idx).generics.clone() } else { Vec::new() };
                let return_annotations = if has_generics { self.func(func_idx).return_annotations.clone() } else { Vec::new() };
                let param_annotations = if has_generics { self.func(func_idx).param_annotations.clone() } else { Vec::new() };

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

                // For colon method calls, self is implicit — func_args includes it but args doesn't
                let has_self = func_args.first().is_some_and(|&sym| {
                    matches!(&self.sym(sym).id, SymbolIdentifier::Name(n) if n == "self")
                });
                let self_offset = if has_self { 1 } else { 0 };

                // Emit redundant-parameter / missing-parameter diagnostics
                {
                    let actual_count = args.len();
                    let expected_count = func_args.len() - self_offset;

                    // Redundant: more args than params, and function is not vararg
                    if actual_count > expected_count && !is_vararg {
                        // Check overloads: if any overload accepts this many args, skip
                        let overload_accepts = overloads.iter().any(|o| {
                            o.params.len() >= actual_count
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
                    if actual_count < expected_count {
                        // Count required params (non-optional, excluding trailing optional)
                        let required_count = {
                            let mut count = expected_count;
                            // Walk backwards from the end, skipping optional params (use self_offset to skip self)
                            for i in (self_offset..func_args.len()).rev() {
                                if param_optional.get(i).copied().unwrap_or(false) {
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

                // Propagate call-site arg types to parameter symbols (local only)
                for (i, arg_expr_id) in args.iter().enumerate() {
                    if let Some(&param_sym_idx) = func_args.get(i + self_offset) {
                        if param_sym_idx >= EXT_BASE { continue; }
                        if let Some(ver) = self.ir.symbols[param_sym_idx].versions.first() {
                            if ver.resolved_type.is_none() {
                                if let Some(arg_type) = self.resolve_expr(*arg_expr_id) {
                                    // Widen boolean literals to boolean when inferring param types
                                    let arg_type = match arg_type {
                                        ValueType::Boolean(Some(_)) => ValueType::Boolean(None),
                                        other => other,
                                    };
                                    self.ir.symbols[param_sym_idx].versions[0].resolved_type = Some(arg_type);
                                }
                            }
                        }
                    }
                }

                // Build generic substitution map from call-site arg types
                let mut generic_subs: HashMap<String, ValueType> = HashMap::new();
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
                                generic_subs.insert(name.clone(), arg_type.clone());
                            }
                            // Infer generics from structured param annotations (T[], table<K,V>)
                            if let Some(annotation) = param_annotations.get(i + self_offset) {
                                self.infer_generics_from_annotation(annotation, &generic_names, *arg_expr_id, &mut generic_subs);
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

                // Find the matching overload (if any) — used for both diagnostics and return type
                let matching_overload = if !overloads.is_empty() {
                    let n_args = args.len();
                    overloads.iter()
                        .find(|o| o.params.len() == n_args)
                        .or(overloads.first())
                } else {
                    None
                };

                // Emit type mismatch diagnostics
                for (i, arg_expr_id) in args.iter().enumerate() {
                    let Some(arg_type) = self.resolve_expr(*arg_expr_id) else { continue };
                    // Get expected parameter type (last version = the function param, not outer scope)
                    let expected_type = if let Some(overload) = matching_overload {
                        overload.params.get(i).and_then(|(_, t)| t.clone())
                    } else if let Some(&param_sym_idx) = func_args.get(i + self_offset) {
                        self.sym(param_sym_idx).versions.last()
                            .and_then(|ver| ver.resolved_type.clone())
                    } else {
                        None
                    };
                    let Some(expected_type) = expected_type else { continue };
                    // Skip type-mismatch for generic type variables
                    if matches!(expected_type, ValueType::TypeVariable(_)) { continue; }
                    // Check assignability (structural + table subclass)
                    if !arg_type.is_assignable_to(&expected_type) && !self.is_table_subtype(&arg_type, &expected_type) {
                        let param_name: String = if let Some(overload) = matching_overload {
                            overload.params.get(i).map(|(n, _)| n.clone()).unwrap_or_else(|| "?".to_string())
                        } else if let Some(&param_sym_idx) = func_args.get(i + self_offset) {
                            if let SymbolIdentifier::Name(n) = &self.sym(param_sym_idx).id { n.clone() } else { "?".to_string() }
                        } else {
                            "?".to_string()
                        };
                        let expected_str = self.format_value_type_depth(&expected_type, 0);
                        let actual_str = self.format_value_type_depth(&arg_type, 0);
                        if let Some(&(start, end)) = arg_ranges.get(i) {
                            crate::diagnostics::type_mismatch::check(
                                &mut self.diagnostics, &param_name,
                                &expected_str, &actual_str,
                                start as usize, end as usize,
                            );
                        }
                    }
                }

                // Pick the matching overload signature for return types
                let ret_index = *ret_index;
                let return_type = matching_overload
                    .and_then(|o| o.returns.get(ret_index))
                    .map(|vt| {
                        if generic_subs.is_empty() {
                            vt.clone()
                        } else {
                            vt.substitute_generics(&generic_subs)
                        }
                    });
                if let Some(rt) = return_type {
                    return Some(rt);
                }

                // Generic substitution for non-overload return types
                if !generic_subs.is_empty() {
                    if let Some(ret_vt) = return_annotations.get(ret_index) {
                        let substituted = ret_vt.substitute_generics(&generic_subs);
                        if !matches!(substituted, ValueType::TypeVariable(_)) {
                            return Some(substituted);
                        }
                    }
                }

                // Non-overload: look up the return symbol
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_scope)?;
                self.sym(ret_sym_idx).versions.first()?.resolved_type.clone()
            }

            Expr::FieldAccess { table, field, field_range } => {
                let field_range = *field_range;
                let table_type = self.resolve_expr(*table)?;
                let table_indices: Vec<TableIndex> = match &table_type {
                    ValueType::Table(Some(idx)) => vec![*idx],
                    ValueType::Union(types) => types.iter().filter_map(|t| match t {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }).collect(),
                    _ => return None,
                };
                if table_indices.is_empty() { return None; }

                // Try each table in the union for the field, collecting types
                // Prefer @type annotation when available, else use expr + extra_exprs
                let mut field_types: Vec<ValueType> = Vec::new();
                for &idx in &table_indices {
                    if let Some(fi) = self.table(idx).fields.get(field) {
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

                // Field not found — check for undefined-field diagnostic on the first @class table
                let first_idx = table_indices[0];
                if self.table(first_idx).class_name.is_some() {
                    // Check parent classes across all tables in the union
                    let mut found = false;
                    for &idx in &table_indices {
                        let parents = self.table(idx).parent_classes.clone();
                        for &parent_idx in &parents {
                            if self.table(parent_idx).fields.contains_key(field) {
                                found = true;
                                break;
                            }
                        }
                        if found { break; }
                    }
                    if !found {
                        if let Some((start, end)) = field_range {
                            let class_name = self.table(first_idx).class_name.clone().unwrap_or_default();
                            crate::diagnostics::undefined_field::check(
                                &mut self.diagnostics,
                                field, &class_name,
                                start as usize, end as usize,
                            );
                        }
                    }
                }
                None
            }
            Expr::VarArgs(ret_index) => {
                // WoW passes (addonName: string, addonTable: table) to each file
                match ret_index {
                    0 => Some(ValueType::String),
                    1 => {
                        if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                            Some(ValueType::Table(Some(addon_idx)))
                        } else {
                            let table_idx = self.ir.tables.len();
                            self.ir.tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), value_type: None });
                            Some(ValueType::Table(Some(table_idx)))
                        }
                    }
                    _ => Some(ValueType::Nil),
                }
            }
            Expr::BracketIndex { table, key: _ } => {
                let table_type = self.resolve_expr(*table)?;
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
            _ => None,
        }
    }

    pub(super) fn resolve_binary_op(&self, op: Operator, lhs_type: ValueType, rhs_type: ValueType) -> Option<ValueType> {
        match op {
            Operator::Or => {
                match (&lhs_type, &rhs_type) {
                    (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(Some(true)), _) => {
                        Some(lhs_type)
                    },
                    (ValueType::Boolean(None), ValueType::Boolean(_)) => Some(lhs_type),
                    (ValueType::Boolean(None), _) => {
                        Some(ValueType::union(
                            ValueType::Boolean(Some(true)),
                            rhs_type.clone(),
                        ))
                    },
                    (ValueType::Union(types), _) => {
                        let has_falsy = types.iter().any(|t| matches!(t, ValueType::Nil | ValueType::Boolean(Some(false))));
                        if has_falsy {
                            let mut remaining: Vec<ValueType> = types.iter()
                                .filter(|t| !matches!(t, ValueType::Nil | ValueType::Boolean(Some(false))))
                                .cloned()
                                .collect();
                            remaining.push(rhs_type.clone());
                            Some(ValueType::make_union(remaining))
                        } else {
                            Some(lhs_type)
                        }
                    },
                    (ValueType::Number | ValueType::String | ValueType::Function(_) | ValueType::Table(_) | ValueType::TypeVariable(_), _) => {
                        Some(lhs_type)
                    },
                }
            },
            Operator::And => {
                match (&lhs_type, &rhs_type) {
                    (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => {
                        Some(lhs_type)
                    },
                    (ValueType::Boolean(Some(true)) | ValueType::Number | ValueType::String | ValueType::Function(_) | ValueType::Table(_) | ValueType::Union(_) | ValueType::TypeVariable(_), _) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(None), ValueType::Boolean(Some(true))) => {
                        Some(lhs_type)
                    },
                    (_, ValueType::Boolean(Some(false)) | ValueType::Nil) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(None), _) => {
                        Some(ValueType::union(
                            ValueType::Boolean(Some(false)),
                            rhs_type.clone(),
                        ))
                    },
                }
            },
            Operator::LessThan | Operator::GreaterThan | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals => {
                Some(ValueType::Boolean(None))
            },
            Operator::NotEquals | Operator::Equals => {
                Some(ValueType::Boolean(None))
            },
            Operator::Concatenate => {
                if lhs_type.can_concat_to_string() && rhs_type.can_concat_to_string() {
                    Some(ValueType::String)
                } else {
                    None
                }
            },
            Operator::Add | Operator::Subtract | Operator::Divide | Operator::Multiply | Operator::Modulo | Operator::Hat => {
                match (&lhs_type, &rhs_type) {
                    (ValueType::Number, ValueType::Number) => Some(ValueType::Number),
                    (ValueType::Table(_), _) | (_, ValueType::Table(_)) => None, // TODO: metamethods
                    _ => None,
                }
            },
            _ => None,
        }
    }
}

