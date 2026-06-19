use super::*;

/// Union the resolved types of every `FunctionRet` symbol in `rets` whose
/// slot index matches `slot`. Returns `None` when no matching symbols exist
/// for that slot or none have resolved yet. When some return branches are
/// resolved and others are not, `Any` is unioned into the result to represent
/// the unknown contribution — this prevents a misleading partial type (e.g.
/// `nil` when the function also has an unresolved non-nil return) while still
/// preserving the resolved branches for callers like `infer_tail_call_returns`.
///
/// Each `return` statement registers its own `FunctionRet` symbol at the
/// scope it lives in, so a function with branched returns has multiple
/// symbols sharing the same `(func_idx, slot)` id. The call-site resolver
/// in `resolve.rs` and `dedup_return_types` (below) both walk `func.rets`
/// to collect every contribution.
pub(crate) fn return_type_at_slot(ir: &Ir, rets: &[SymbolIndex], slot: usize) -> Option<ValueType> {
    let mut acc: Option<ValueType> = None;
    let mut any_unresolved = false;
    for &sym_idx in rets {
        if let SymbolIdentifier::FunctionRet(_, idx) = &ir.sym(sym_idx).id {
            if *idx != slot { continue; }
            if let Some(vt) = ir.sym(sym_idx).versions.first()
                .and_then(|v| v.resolved_type.as_ref())
            {
                acc = Some(match acc.take() {
                    Some(prev) => ir.dedupe_union_tables(ValueType::make_union(vec![prev, vt.clone()])),
                    None => vt.clone(),
                });
            } else {
                any_unresolved = true;
            }
        }
    }
    // Mixed case: some return branches resolved but others are still
    // unresolved. Union Any into the accumulated type to represent the
    // unknown contribution — this avoids silently dropping unresolved
    // branches (which would produce a misleading partial type like `nil`
    // when the function also returns an unresolved value) while still
    // preserving resolved contributions for downstream callers.
    // When ALL returns are unresolved, acc stays None — preserving the
    // "no type info" semantics that unknown-type diagnostics rely on.
    if any_unresolved && let Some(prev) = acc {
        acc = Some(ir.dedupe_union_tables(ValueType::make_union(vec![prev, ValueType::Any])));
    }
    acc
}

/// Query-time variant of `return_type_at_slot` used by `resolve_expr_type_impl`.
/// Unions all resolved FunctionRets at the given slot but does NOT add `Any`
/// for unresolved entries. When there is at least one concrete (non-call,
/// non-Any) return at this slot, `Any`-typed FunctionCall returns are
/// skipped — they are artifacts of circular fixpoint resolution in
/// recursive/mutually-recursive functions. When ALL returns are calls, none
/// are skipped (the `Any` is legitimate, e.g. a wrapper function).
///
/// Not used by `dedup_return_types` (hover/signature formatting) because
/// filtering `Any`-typed call returns there can incorrectly drop legitimate
/// `Any` from non-circular calls (e.g. `return tremove(tbl)`).
fn query_return_type_at_slot(ir: &Ir, rets: &[SymbolIndex], slot: usize) -> Option<ValueType> {
    let slot_rets: Vec<_> = rets.iter().filter_map(|&sym_idx| {
        let SymbolIdentifier::FunctionRet(_, idx) = &ir.sym(sym_idx).id else { return None };
        if *idx != slot { return None; }
        let ver = ir.sym(sym_idx).versions.first()?;
        let is_call = ver.type_source
            .is_some_and(|ts| matches!(ir.expr(ts), Expr::FunctionCall { .. }));
        Some((sym_idx, is_call))
    }).collect();

    let has_concrete_non_call = slot_rets.iter().any(|&(sym_idx, is_call)| {
        !is_call && ir.sym(sym_idx).versions.first()
            .and_then(|v| v.resolved_type.as_ref())
            .is_some_and(|vt| !matches!(vt, ValueType::Any))
    });

    let mut acc: Option<ValueType> = None;
    for &(sym_idx, is_call) in &slot_rets {
        if let Some(vt) = ir.sym(sym_idx).versions.first()
            .and_then(|v| v.resolved_type.as_ref())
        {
            if has_concrete_non_call && is_call && matches!(vt, ValueType::Any) {
                continue;
            }
            acc = Some(match acc.take() {
                Some(prev) => ir.dedupe_union_tables(ValueType::make_union(vec![prev, vt.clone()])),
                None => vt.clone(),
            });
        }
    }
    acc
}

/// Deduplicate `func.rets` by return position and union the resolved types.
/// Multiple `return` statements in different scopes create separate symbols for
/// the same position in `func.rets`. This function groups them by index and
/// returns one type per position (the union of all matching symbols' types).
pub(crate) fn dedup_return_types(ir: &Ir, rets: &[SymbolIndex]) -> Vec<Option<ValueType>> {
    let mut by_index: BTreeMap<usize, Option<ValueType>> = BTreeMap::new();
    for &sym_idx in rets {
        if let SymbolIdentifier::FunctionRet(_, index) = &ir.sym(sym_idx).id {
            by_index.entry(*index).or_insert(None);
        }
    }
    for slot in by_index.keys().cloned().collect::<Vec<_>>() {
        let vt = return_type_at_slot(ir, rets, slot);
        by_index.insert(slot, vt);
    }
    by_index.into_values().collect()
}

/// Maximum recursion depth for read-only expression resolution.
const MAX_QUERY_RESOLVE_DEPTH: usize = 200;

/// Shared implementation for read-only expression type resolution.
/// Both `Analysis::resolve_expr_type` and `AnalysisResult::resolve_expr_type` delegate here.
pub(super) fn resolve_expr_type_impl(
    ir: &Ir,
    resolved_expr_cache: &[Option<ValueType>],
    expr_id: ExprId,
    visited: &mut HashSet<ExprId>,
    depth: usize,
) -> Option<ValueType> {
    // Check Phase 2 resolve cache first — builder chains (@builds-field / @built-name /
    // @return self) are resolved during the fixpoint loop and the result is cached here.
    // The read-only resolver can't replicate the mutable table-cloning logic, so we
    // rely on the cached result for these expressions.
    if let Some(cached) = resolved_expr_cache.get(expr_id.val()).and_then(|v| v.as_ref()) {
        return Some(cached.clone());
    }
    // Depth limit: prevent stack overflow on deeply nested chains
    if depth >= MAX_QUERY_RESOLVE_DEPTH {
        return None;
    }
    // External exprs (>= EXT_BASE) are immutable/shared and can legitimately appear
    // multiple times in method chains (e.g. repeated :AddField() calls on the same class).
    // Only track local exprs for cycle detection.
    if !expr_id.is_external() && !visited.insert(expr_id) {
        return None;
    }
    match ir.expr(expr_id) {
        Expr::Literal(vt) => Some(vt.clone()),
        Expr::SymbolRef(sym_idx, ver_idx) => {
            let sym = ir.sym(*sym_idx);
            sym.versions[*ver_idx].resolved_type.clone()
        }
        Expr::FunctionDef(func_idx) => {
            Some(ValueType::Function(Some(*func_idx)))
        }
        Expr::TableConstructor(table_idx) => {
            Some(ValueType::Table(Some(*table_idx)))
        }
        Expr::Grouped(inner) => resolve_expr_type_impl(ir, resolved_expr_cache, *inner, visited, depth + 1),
        Expr::BinaryOp { op, lhs, rhs } => {
            let (op, lhs, rhs) = (*op, *lhs, *rhs);
            let lhs_type = resolve_expr_type_impl(ir, resolved_expr_cache, lhs, visited, depth + 1);
            let rhs_type = resolve_expr_type_impl(ir, resolved_expr_cache, rhs, visited, depth + 1);
            match (lhs_type, rhs_type) {
                (Some(l), Some(r)) => crate::analysis::resolve::resolve_binary_op_standalone(op, l, r),
                (Some(ValueType::Number), None) | (None, Some(ValueType::Number))
                    if op.is_arithmetic() => Some(ValueType::Number),
                (Some(ref t), None) | (None, Some(ref t))
                    if op == Operator::Concatenate && t.can_concat_to_string() => Some(ValueType::String(None)),
                _ if op.is_comparison() => Some(ValueType::Boolean(None)),
                _ => None,
            }
        }
        Expr::UnaryOp { op, operand } => {
            let (op, operand) = (*op, *operand);
            let operand_type = resolve_expr_type_impl(ir, resolved_expr_cache, operand, visited, depth + 1)?;
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
        Expr::FieldAccess { table, field, .. } => {
            let table = *table;
            let field = field.clone();
            let table_type = resolve_expr_type_impl(ir, resolved_expr_cache, table, visited, depth + 1)?;
            let table_type = table_type.into_strip_opaque();
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
            // Try each table in the union for the field, including parent classes
            let mut field_types: Vec<ValueType> = Vec::new();
            for &idx in &table_indices {
                if let Some(fi) = ir.get_field(idx, &field) {
                    let primary = fi.expr;
                    let extras: Vec<ExprId> = fi.extra_exprs.clone();
                    let annotation = fi.annotation.clone();
                    if let Some(ann) = annotation {
                        if !field_types.contains(&ann) {
                            field_types.push(ann);
                        }
                    } else {
                        // Skip nil primary when there are reassignments
                        let skip_primary = !extras.is_empty()
                            && matches!(resolve_expr_type_impl(ir, resolved_expr_cache, primary, visited, depth + 1), Some(ValueType::Nil));
                        let all_exprs: Vec<ExprId> = if skip_primary {
                            extras
                        } else {
                            std::iter::once(primary).chain(extras).collect()
                        };
                        let mut has_unresolvable = false;
                        for eid in all_exprs {
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, eid, visited, depth + 1) {
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
                    // If own field resolved only to Table(None) placeholders and the
                    // table is a class, fall through to parent class check for a better type.
                    // (Mirrors the same guard in resolve.rs FieldAccess and
                    // queries.rs resolve_field_or_g_env.)
                    if !field_types.is_empty()
                        && (!field_types.iter().all(|vt| matches!(vt, ValueType::Table(None)))
                            || ir.table(idx).class_name.is_none())
                    {
                        continue;
                    }
                }
                // Check parent classes
                for &parent_idx in &ir.table(idx).parent_classes {
                    if let Some(fi) = ir.get_field(parent_idx, &field) {
                        if let Some(ref ann) = fi.annotation {
                            if !matches!(ann, ValueType::Any | ValueType::Table(None))
                                && !field_types.contains(ann) {
                                field_types.push(ann.clone());
                            }
                        } else {
                            let expr = fi.expr;
                            if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, expr, visited, depth + 1)
                                && !matches!(vt, ValueType::Any | ValueType::Table(None))
                                && !field_types.contains(&vt) {
                                    field_types.push(vt);
                                }
                        }
                        break;
                    }
                }
            }
            if field_types.is_empty() {
                return ir.explicit_map_value_type(&table_indices);
            }
            Some(ValueType::make_union(field_types))
        }
        Expr::FunctionCall { func, ret_index, .. } => {
            let func = *func;
            let ret_index = *ret_index;
            let func_type = resolve_expr_type_impl(ir, resolved_expr_cache, func, visited, depth + 1)?;
            let func_type = func_type.into_strip_opaque();
            let func_idx = match func_type {
                ValueType::Function(Some(idx)) => idx,
                ValueType::Table(Some(table_idx)) => {
                    ir.table(table_idx).call_func?
                }
                _ => return None,
            };
            let func_info = ir.func(func_idx);
            // Handle @return self
            if func_info.returns_self && ret_index == 0
                && let Expr::FieldAccess { table: receiver_expr, .. } = ir.expr(func).clone()
                    && let Some(rt) = resolve_expr_type_impl(ir, resolved_expr_cache, receiver_expr, visited, depth + 1) {
                        return Some(rt);
                    }
            // Handle @return built: return the accumulated built_table from the receiver
            if func_info.returns_built && ret_index == 0
                && let Expr::FieldAccess { table: receiver_expr, .. } = ir.expr(func).clone()
                    && let Some(ValueType::Table(Some(recv_idx))) = resolve_expr_type_impl(ir, resolved_expr_cache, receiver_expr, visited, depth + 1) {
                        if let Some(built_idx) = ir.table(recv_idx).built_table {
                            return Some(ValueType::Table(Some(built_idx)));
                        }
                        return Some(ValueType::Table(None));
                    }
            query_return_type_at_slot(ir, &func_info.rets, ret_index)
        }
        Expr::BracketIndex { table, .. } => {
            let table = *table;
            let table_type = resolve_expr_type_impl(ir, resolved_expr_cache, table, visited, depth + 1)?;
            let table_type = table_type.into_strip_opaque();
            match &table_type {
                ValueType::Table(Some(idx)) => ir.table(*idx).value_type.clone(),
                ValueType::Union(types) => {
                    if types.iter().any(|t| matches!(t, ValueType::Table(None))) {
                        return Some(ValueType::Any);
                    }
                    let mut vts: Vec<ValueType> = Vec::new();
                    for t in types {
                        if let ValueType::Table(Some(idx)) = t
                            && let Some(vt) = &ir.table(*idx).value_type
                                && !vts.contains(vt) { vts.push(vt.clone()); }
                    }
                    if vts.is_empty() { None } else { Some(ValueType::make_union(vts)) }
                }
                ValueType::Table(None) => Some(ValueType::Any),
                _ => None,
            }
        }
        Expr::VarArgs(ret_index, file_level) => {
            if *file_level {
                match ret_index {
                    0 => Some(ValueType::String(None)),
                    1 => {
                        ir.addon_table_idx().map(|idx| ValueType::Table(Some(idx)))
                    }
                    _ => Some(ValueType::Nil),
                }
            } else {
                None
            }
        }
        Expr::BranchMerge(exprs) => {
            let exprs = exprs.clone();
            let mut types: Vec<ValueType> = Vec::new();
            for eid in exprs {
                if let Some(vt) = resolve_expr_type_impl(ir, resolved_expr_cache, eid, visited, depth + 1) {
                    types.push(vt);
                }
            }
            if types.is_empty() { None } else { Some(ValueType::make_union(types)) }
        }
        Expr::StripNil(inner) => {
            let inner = *inner;
            match resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1).map(|vt| vt.strip_nil()) {
                Some(ValueType::Union(ref members)) if members.is_empty() => None,
                other => other,
            }
        }
        Expr::AssignNarrow { inner, rhs } => {
            let inner = *inner;
            let rhs = *rhs;
            let inner_ty = resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1);
            let rhs_ty = resolve_expr_type_impl(ir, resolved_expr_cache, rhs, visited, depth + 1);
            let strip = match rhs_ty {
                Some(ref t) => !t.contains_nil(),
                None => false,
            };
            if strip {
                match inner_ty.map(|vt| vt.strip_nil()) {
                    Some(ValueType::Union(ref members)) if members.is_empty() => None,
                    other => other,
                }
            } else {
                inner_ty
            }
        }
        Expr::StripFalsy(inner) => {
            let inner = *inner;
            match resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1).map(|vt| vt.strip_falsy()) {
                Some(ValueType::Union(ref members)) if members.is_empty() => None,
                other => other,
            }
        }
        Expr::CastAdd(inner, cast_type) => {
            let inner = *inner;
            let cast_type = cast_type.clone();
            resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1)
                .map(|vt| ValueType::union(vt, cast_type))
        }
        Expr::CastRemove(inner, cast_type) => {
            let inner = *inner;
            let cast_type = cast_type.clone();
            resolve_expr_type_impl(ir, resolved_expr_cache, inner, visited, depth + 1)
                .map(|vt| vt.strip_type_with(&cast_type, &|idx| ir.table(idx).enum_kind))
        }
        _ => None,
    }
}

/// Format a single return annotation, prefixing `...` if it's the last entry and vararg.
pub(crate) fn format_vararg_return(formatted: String, index: usize, func: &Function) -> String {
    if index == func.return_annotations.len() - 1 && func.has_vararg_return {
        if formatted.starts_with("...") {
            formatted
        } else {
            format!("...{}", formatted)
        }
    } else if is_intersection_of_varargs_raw(func, index) {
        format!("& {}", formatted)
    } else {
        formatted
    }
}

/// Join multiple return values, parenthesizing any element that itself contains
/// a top-level comma.  Such an element is a nested multi-return function value
/// (e.g. `fun(): number, string`); without grouping, its returns blur into the
/// sibling returns — `fun(): number, string, Foo` reads ambiguously.  Wrapping
/// gives `(fun(): number, string), Foo`.  A single return (no sibling to confuse
/// with) and elements without a top-level comma pass through unchanged.
pub(super) fn join_returns(rets: &[String]) -> String {
    if rets.len() <= 1 {
        return rets.join(", ");
    }
    rets.iter()
        .map(|r| {
            if has_top_level_comma(r) {
                format!("({r})")
            } else {
                r.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether `s` contains a comma that is not nested inside any bracket pair
/// (`()`, `<>`, `[]`, `{}`).  Used to detect a rendered type whose top-level
/// shape is itself a comma-separated tuple (i.e. a multi-return function value).
///
/// Assumes well-formed input (balanced brackets) since `s` is always produced
/// by the type formatter.  Depth is clamped to 0 on close-brackets so a stray
/// unmatched closer cannot drive depth negative and mask later commas.
fn has_top_level_comma(s: &str) -> bool {
    let mut depth = 0i32;
    for b in s.bytes() {
        match b {
            b'(' | b'<' | b'[' | b'{' => depth += 1,
            b')' | b'>' | b']' | b'}' => { depth = (depth - 1).max(0); }
            b',' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

/// Check whether a return annotation at `index` was written as `& ...M`
/// (intersection-of-varargs).  The raw annotation is `Intersection([VarArgs(_)])`
/// — a single-element intersection wrapping a VarArgs.
pub(super) fn is_intersection_of_varargs_raw(func: &Function, index: usize) -> bool {
    func.return_annotations_raw.get(index).is_some_and(|raw| {
        matches!(raw, crate::annotations::AnnotationType::Intersection(parts) if parts.len() == 1 && matches!(&parts[0], crate::annotations::AnnotationType::VarArgs(_)))
    })
}

/// Format a vararg parameter for display.  When the type annotation already
/// starts with `...` (e.g. `...M` from a variadic generic), the name `...` is
/// redundant so we show just the type.  Otherwise show `...: type`.
pub(crate) fn format_vararg_param(ann: &crate::annotations::AnnotationType) -> String {
    let type_text = crate::annotations::format_annotation_type(ann);
    if type_text.starts_with("...") {
        type_text
    } else {
        format!("...: {}", type_text)
    }
}

impl AnalysisResult {
    /// Get the string literal value for a symbol, checking both local and external sources.
    pub(super) fn get_string_value(&self, symbol_idx: SymbolIndex, token_start: u32) -> Option<&str> {
        // External symbol: look up in PreResolvedGlobals string_values
        if symbol_idx.is_external() {
            return self.ir.ext.string_values.get(&symbol_idx).map(|s| s.as_str());
        }
        // Local symbol: find the version's type_source and check string_literals
        let symbol = self.sym(symbol_idx);
        let version = if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
            symbol.versions.get(ver_idx)
        } else {
            self.version_at_def_site(symbol, token_start)
                .or_else(|| symbol.versions.last())
        };
        version
            .and_then(|v| v.type_source)
            .and_then(|expr_id| self.ir.string_literals.get(&expr_id))
            .map(|s| s.as_str())
    }

    /// Get the number literal value for a symbol, checking both local and external sources.
    pub(super) fn get_number_value(&self, symbol_idx: SymbolIndex, token_start: u32) -> Option<&str> {
        if symbol_idx.is_external() {
            return self.ir.ext.number_values.get(&symbol_idx).map(|s| s.as_str());
        }
        let symbol = self.sym(symbol_idx);
        let version = if let Some(&ver_idx) = self.symbol_version_at.get(&token_start) {
            symbol.versions.get(ver_idx)
        } else {
            self.version_at_def_site(symbol, token_start)
                .or_else(|| symbol.versions.last())
        };
        version
            .and_then(|v| v.type_source)
            .and_then(|expr_id| self.ir.number_literals.get(&expr_id))
            .map(|s| s.as_str())
    }

    /// Get the literal display value for a field's expression (number or quoted string),
    /// checking both local and external sources.
    pub(super) fn get_field_literal_value(&self, field_info: &FieldInfo) -> Option<String> {
        let (num_map, str_map) = if field_info.expr.is_external() {
            (&self.ir.ext.number_literals, &self.ir.ext.string_literals)
        } else {
            (&self.ir.number_literals, &self.ir.string_literals)
        };
        if let Some(val) = num_map.get(&field_info.expr) {
            return Some(val.clone());
        }
        if let Some(val) = str_map.get(&field_info.expr) {
            return Some(format!("\"{}\"", val));
        }
        None
    }

    /// Format a single field line for enum or regular table display.
    /// Enum fields show `name = value`, non-enum fields show `name: type`.
    pub(super) fn format_enum_field_line(&self, indent: &str, name: &str, field_info: &FieldInfo, is_enum: bool, depth: usize) -> String {
        if is_enum
            && let Some(val) = self.get_field_literal_value(field_info)
        {
            return format!("{}{} = {}", indent, name, val);
        }
        let type_str = self.format_field_type(field_info, depth);
        format!("{}{}: {}", indent, name, type_str)
    }

    pub(super) fn narrow_type_for_display(&self, resolved: &ValueType, symbol_idx: SymbolIndex, offset: u32) -> Option<ValueType> {
        let scope_idx = self.scope_at_offset(offset)?;
        // If the symbol was reassigned in this scope, narrowing no longer applies.
        let narrowing_active = !self.is_narrowing_overridden_at(symbol_idx, scope_idx, offset);
        // Start from a type-narrowed base if one exists (e.g. type(x) == "string")
        let base = if narrowing_active {
            if let Some(narrowed_vt) = self.get_type_narrowing(symbol_idx, scope_idx) {
                Some(narrowed_vt.clone())
            } else if let Some(guard_vt) = self.get_type_filtering(symbol_idx, scope_idx) {
                Some(resolved.filter_type_with(guard_vt, &|idx| self.table(idx).enum_kind))
            } else {
                self.get_type_stripping(symbol_idx, scope_idx).map(|stripped_vt| {
                    resolved.strip_type_with(stripped_vt, &|idx| self.table(idx).enum_kind)
                })
            }
        } else {
            None
        };
        // Apply falsy/nil narrowing on top (inner scope `if x then` further narrows)
        let strip_falsy = narrowing_active && self.is_symbol_falsy_narrowed(symbol_idx, scope_idx);
        let strip_nil = strip_falsy || (narrowing_active && self.is_symbol_narrowed(symbol_idx, scope_idx));
        if !strip_nil {
            return base;
        }
        let target = base.as_ref().unwrap_or(resolved);
        // Strip Nil (and optionally false) from union types
        if let ValueType::Union(types) = target {
            let filtered: Vec<_> = types.iter()
                .filter(|t| {
                    if **t == ValueType::Nil { return false; }
                    if strip_falsy && **t == ValueType::Boolean(Some(false)) { return false; }
                    true
                })
                .cloned()
                .collect();
            if filtered.len() == types.len() {
                // Nil stripping didn't change the union; return base if type-filtering
                // or type-narrowing was applied (otherwise None = no change).
                return base;
            }
            if filtered.len() == 1 {
                return Some(filtered.into_iter().next().unwrap());
            }
            if !filtered.is_empty() {
                return Some(ValueType::Union(filtered));
            }
        }
        // Non-union: nil stripping is a no-op. Return base if type-filtering
        // or type-narrowing was applied, otherwise None.
        base
    }

    /// Look up a global symbol by name in scope0 (local and external).
    /// Returns the symbol's resolved type. Used for `_G.field` redirect.
    pub(super) fn resolve_global_symbol_type(&self, name: &str) -> Option<ValueType> {
        let sym_id = SymbolIdentifier::Name(name.to_string());
        let sym_idx = self.ir.scopes[0].symbols.get(&sym_id).copied()
            .or_else(|| self.ir.ext.scope0_symbols.get(&sym_id).copied());
        let si = sym_idx?;
        let sym = self.sym(si);
        sym.versions.last().and_then(|v| v.resolved_type.clone())
    }

    pub(super) fn doc_for_type(&self, st: &ValueType) -> Option<String> {
        match st {
            ValueType::Function(Some(func_idx)) => {
                self.format_function_doc(*func_idx)
            }
            ValueType::Table(Some(table_idx)) => {
                self.format_see_doc(&self.table(*table_idx).see)
            }
            _ => None,
        }
    }

    /// Render `@see` targets as hover doc lines (one per entry).
    pub(crate) fn format_see_doc(&self, see: &[String]) -> Option<String> {
        if see.is_empty() {
            None
        } else {
            Some(see.iter().map(|t| format!("@*see* {}", t)).collect::<Vec<_>>().join("\n\n"))
        }
    }

    /// Build a rich doc string for a function, including its doc comment and @param descriptions.
    pub(super) fn format_function_doc(&self, func_idx: FunctionIndex) -> Option<String> {
        let func = self.func(func_idx);
        let has_descriptions = func.param_descriptions.iter().any(|d| d.is_some());
        let flavors_mask = func.flavors;
        if func.doc.is_none() && !has_descriptions && func.see.is_empty() && flavors_mask == 0 {
            return None;
        }
        let mut parts = Vec::new();
        if let Some(ref doc) = func.doc {
            parts.push(doc.clone());
        }
        if has_descriptions {
            let mut param_lines = Vec::new();
            for (i, &sym_idx) in func.args.iter().enumerate() {
                if let Some(Some(desc)) = func.param_descriptions.get(i) {
                    let name = match &self.sym(sym_idx).id {
                        SymbolIdentifier::Name(n) => n.clone(),
                        _ => continue,
                    };
                    let optional = func.param_optional.get(i).copied().unwrap_or(false);
                    let ann_has_nil = func.param_annotations.get(i)
                        .is_some_and(crate::annotations::annotation_type_is_nullable);
                    let suffix = if optional && !ann_has_nil { "?" } else { "" };
                    param_lines.push(format!("@*param* `{}{}` — {}", name, suffix, desc));
                }
            }
            if !param_lines.is_empty() {
                parts.push(param_lines.join("\n\n"));
            }
        }
        if let Some(see_block) = self.format_see_doc(&func.see) {
            parts.push(see_block);
        }
        // Low-key flavor info for APIs with known availability data.
        if flavors_mask != 0 {
            parts.push(format!("Flavors: {}", crate::flavor::format_flavor_list(flavors_mask)));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }

    pub(crate) fn resolve_expr_type(&self, expr_id: ExprId) -> Option<ValueType> {
        let mut visited = HashSet::new();
        resolve_expr_type_impl(&self.ir, &self.resolved_expr_cache, expr_id, &mut visited, 0)
    }

    /// Resolve a field's type considering annotation, primary expr, and extra_exprs.
    /// Skips nil primary when extras exist (matches reassignment semantics).
    pub(super) fn resolve_field_type(&self, fi: &FieldInfo) -> Option<ValueType> {
        if let Some(ref ann) = fi.annotation {
            return Some(ann.clone());
        }
        let mut types: Vec<ValueType> = Vec::new();
        let skip_primary = !fi.extra_exprs.is_empty()
            && matches!(self.resolve_expr_type(fi.expr), Some(ValueType::Nil));
        let exprs: Vec<ExprId> = if skip_primary {
            fi.extra_exprs.clone()
        } else {
            std::iter::once(fi.expr).chain(fi.extra_exprs.clone()).collect()
        };
        for eid in exprs {
            if let Some(vt) = self.resolve_expr_type(eid)
                && !types.contains(&vt) { types.push(vt); }
        }
        if types.is_empty() { None } else { Some(ValueType::make_union(types)) }
    }

    pub(crate) fn format_type(&self, vt: &ValueType) -> String {
        self.format_type_depth(vt, 0)
    }

    pub(super) fn get_type_args_for_expr(&self, expr_id: ExprId) -> Vec<ValueType> {
        if let Some(args) = self.call_type_args.get(&expr_id) {
            return args.clone();
        }
        let expr = self.expr(expr_id).clone();
        match expr {
            Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::Grouped(inner) => {
                self.get_type_args_for_expr(inner)
            }
            Expr::AssignNarrow { inner, .. } => self.get_type_args_for_expr(inner),
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
                let table_idx = match self.resolve_expr_type(table) {
                    Some(ValueType::Table(Some(idx))) => idx,
                    _ => return Vec::new(),
                };
                if let Some(cached) = self.field_type_args_cache.get(&(table_idx, field.clone())) {
                    return cached.clone();
                }
                if let Some(fi) = self.table(table_idx).fields.get(&field) {
                    if let Some(args) = self.call_type_args.get(&fi.expr) {
                        return args.clone();
                    }
                    for &extra in &fi.extra_exprs {
                        if let Some(args) = self.call_type_args.get(&extra) {
                            return args.clone();
                        }
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn get_symbol_type_args(&self, sym_idx: SymbolIndex, token_start: u32) -> Vec<ValueType> {
        let ver_idx = self.symbol_version_at.get(&token_start).copied().unwrap_or(0);
        let sym = self.sym(sym_idx);
        if let Some(version) = sym.versions.get(ver_idx) {
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

    pub(super) fn format_type_args(&self, type_args: &[ValueType]) -> String {
        type_args.iter()
            .map(|a| self.format_type_depth(a, 1))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(super) fn append_type_args_to_class(&self, formatted: &str, vt: &ValueType, type_args: &[ValueType]) -> String {
        if type_args.is_empty() {
            return formatted.to_string();
        }
        // Don't display unresolved generic type variables (e.g. "R" from @generic R).
        // Show just the class name without type args instead of "ClassName<R>".
        if type_args.iter().any(|a| matches!(a, ValueType::TypeVariable(_))) {
            return formatted.to_string();
        }
        if let ValueType::Table(Some(idx)) = vt
            && let Some(ref class_name) = self.table(*idx).class_name
            && formatted.starts_with(class_name.as_str())
        {
            let args_str = self.format_type_args(type_args);
            return format!("{}<{}>", class_name, args_str);
        }
        // Handle nullable parameterized class: Union([Table(class), Nil]) formatted as "ClassName?"
        if let ValueType::Union(members) = vt
            && members.len() == 2
            && members.iter().any(|t| matches!(t, ValueType::Nil))
            && let Some(ValueType::Table(Some(idx))) = members.iter().find(|t| !matches!(t, ValueType::Nil))
            && let Some(ref class_name) = self.table(*idx).class_name
            && formatted.starts_with(class_name.as_str())
        {
            let args_str = self.format_type_args(type_args);
            return format!("{}<{}>?", class_name, args_str);
        }
        formatted.to_string()
    }

    /// Collect accessible fields from one or more tables, deduplicating by name.
    /// Returns sorted, formatted field lines (e.g. `"  name: type"`).
    pub(super) fn collect_accessible_fields(
        &self,
        table_indices: &[TableIndex],
        enclosing_class: Option<TableIndex>,
    ) -> Vec<String> {
        let indent = "  ";
        let mut seen: HashSet<&str> = HashSet::new();
        let mut fields: Vec<String> = Vec::new();
        for &table_idx in table_indices {
            let table = self.table(table_idx);
            let overlay = self.ir.overlay_fields.get(&table_idx);
            let is_enum = table.enum_kind.is_enum();
            let is_accessible = |fi: &FieldInfo| -> bool {
                match fi.visibility {
                    crate::annotations::Visibility::Public => true,
                    crate::annotations::Visibility::Private => {
                        enclosing_class.is_some_and(|ec| self.same_class(ec, table_idx))
                    }
                    crate::annotations::Visibility::Protected => {
                        enclosing_class.is_some_and(|ec| self.is_subclass_of(ec, table_idx))
                    }
                }
            };
            for (name, field_info) in &table.fields {
                if seen.insert(name.as_str()) && is_accessible(field_info) {
                    fields.push(self.format_enum_field_line(indent, name, field_info, is_enum, 0));
                }
            }
            if let Some(ov) = overlay {
                for (name, field_info) in ov.iter() {
                    if seen.insert(name.as_str()) && is_accessible(field_info) {
                        fields.push(self.format_enum_field_line(indent, name, field_info, is_enum, 0));
                    }
                }
            }
            for &parent_idx in &table.parent_classes {
                let parent_table = self.table(parent_idx);
                for (name, field_info) in &parent_table.fields {
                    if seen.insert(name.as_str()) && is_accessible(field_info) {
                        fields.push(self.format_enum_field_line(indent, name, field_info, is_enum, 0));
                    }
                }
            }
        }
        fields.sort();
        fields
    }

    /// Format a type for hover display, filtering out inaccessible private/protected fields.
    pub(super) fn format_type_accessible(&self, vt: &ValueType, enclosing_class: Option<TableIndex>) -> String {
        if let ValueType::Table(Some(table_idx)) = vt {
            let table = self.table(*table_idx);
            let overlay = self.ir.overlay_fields.get(table_idx);
            let has_fields = !table.fields.is_empty() || overlay.is_some_and(|o| !o.is_empty());
            let has_parents = !table.parent_classes.is_empty();
            if let Some(ref class_name) = table.class_name {
                if !has_fields && !has_parents {
                    return class_name.clone();
                }
                let fields = self.collect_accessible_fields(&[*table_idx], enclosing_class);
                if fields.is_empty() {
                    return class_name.clone();
                }
                return format!("{} {{\n{}\n}}", class_name, fields.join(",\n"));
            }
        }
        if let ValueType::Intersection(types) = vt {
            // Flatten nested intersections.
            let flat = Self::flatten_intersection(types);

            // Collect table indices from members.
            let table_indices: Vec<TableIndex> = flat.iter()
                .filter_map(|t| if let ValueType::Table(Some(idx)) = t { Some(*idx) } else { None })
                .collect();

            if !table_indices.is_empty() {
                // Dedup: remove members that are ancestors of another member.
                // e.g. if MixinClass : Frame, then Frame & MixinClass → MixinClass
                let deduped: Vec<&ValueType> = flat.iter().copied().filter(|t| {
                    if let ValueType::Table(Some(idx)) = t {
                        // Drop this member if some OTHER table member is a subclass of it
                        !table_indices.iter().any(|&other| other != *idx && self.is_subclass_of(other, *idx))
                    } else {
                        true
                    }
                }).collect();

                // Build header line with deduped member names (depth 1 → class names only).
                // Skip anonymous tables with fields — they'd expand inline in the header
                // but their fields are already shown in the vertical block below.
                let header_parts: Vec<String> = deduped.iter()
                    .filter(|t| {
                        if let ValueType::Table(Some(idx)) = t {
                            let tbl = self.table(*idx);
                            // Keep: named classes, array/map tables. Skip: anonymous field tables.
                            tbl.class_name.is_some() || tbl.value_type.is_some() || tbl.fields.is_empty()
                        } else {
                            true
                        }
                    })
                    .map(|t| self.format_value_type_depth(t, 1))
                    .collect();
                let header = header_parts.join(" & ");

                // If all members were filtered from the header, fall back to compact format.
                if header.is_empty() {
                    return self.format_type(vt);
                }

                let fields = self.collect_accessible_fields(&table_indices, enclosing_class);
                if fields.is_empty() {
                    return header;
                }
                return format!("{} {{\n{}\n}}", header, fields.join(",\n"));
            }
        }
        self.format_type(vt)
    }

    /// Flatten nested `Intersection` types into a single flat list.
    pub(super) fn flatten_intersection(types: &[ValueType]) -> Vec<&ValueType> {
        let mut flat = Vec::new();
        for t in types {
            if let ValueType::Intersection(inner) = t {
                flat.extend(Self::flatten_intersection(inner));
            } else {
                flat.push(t);
            }
        }
        flat
    }

    pub(crate) fn format_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        self.format_value_type_depth(vt, depth)
    }

    /// Format a type for inlay hints: anonymous shape tables (no class name,
    /// no key/value type) collapse to `table` instead of listing fields inline.
    pub(super) fn format_type_for_hint(&self, vt: &ValueType) -> String {
        if self.is_anon_shape_table(vt) {
            return "table".to_string();
        }
        if let ValueType::Union(members) = vt
            && members.iter().any(|m| self.is_anon_shape_table(m))
        {
            // Re-format with anonymous tables collapsed
            let collapsed: Vec<String> = members.iter().map(|m| {
                if self.is_anon_shape_table(m) {
                    "table".to_string()
                } else {
                    self.format_type_depth(m, 1)
                }
            }).collect();
            // Apply T? shorthand for two-member unions with nil
            if collapsed.len() == 2 && members.iter().any(|t| matches!(t, ValueType::Nil)) {
                let other = collapsed.iter().find(|s| s.as_str() != "nil").unwrap();
                return format!("{}?", other);
            }
            return collapsed.join(" | ");
        }
        self.format_type_depth(vt, 1)
    }

    pub(super) fn is_anon_shape_table(&self, vt: &ValueType) -> bool {
        if let ValueType::Table(Some(table_idx)) = vt {
            let table = self.table(*table_idx);
            table.class_name.is_none() && table.value_type.is_none() && table.key_type.is_none()
                && !table.fields.is_empty()
        } else {
            false
        }
    }

    /// For tables whose constructor had array elements that were later mutated
    /// via bracket assignment (e.g. `{strsplit(","  , s)}` then `tbl[i] = tonumber(tbl[i])`),
    /// return the initial element type for display purposes.
    pub(super) fn initial_array_display(&self, vt: &ValueType) -> Option<String> {
        let ValueType::Table(Some(table_idx)) = vt else { return None };
        let table = self.table(*table_idx);
        let ivt = table.initial_value_type.as_ref()?;
        // Only use initial type when it actually differs from the resolved value_type
        if table.value_type.as_ref() == Some(ivt) { return None; }
        let val_str = self.format_value_type_depth(ivt, 1);
        Some(if matches!(ivt, ValueType::Union(_) | ValueType::Intersection(_)) {
            format!("({})[]", val_str)
        } else {
            format!("{}[]", val_str)
        })
    }

    pub(super) fn format_field_type(&self, field_info: &FieldInfo, depth: usize) -> String {
        if let Some(ref text) = field_info.annotation_text {
            // annotation_text from format_annotation_type already includes ! for NonNil
            return text.clone();
        }
        if let Some(ref ann) = field_info.annotation {
            let base = self.format_type_depth(ann, depth + 1);
            return if field_info.lateinit { format!("{}!", base) } else { base };
        }
        // Union original expr with any reassignment exprs.
        // If there are reassignments and the initial value is nil,
        // skip the nil — it's just a placeholder initializer.
        let skip_primary = !field_info.extra_exprs.is_empty()
            && matches!(self.resolve_expr_type(field_info.expr), Some(ValueType::Nil));
        let mut types: Vec<ValueType> = Vec::new();
        let exprs: Vec<ExprId> = if skip_primary {
            field_info.extra_exprs.clone()
        } else {
            std::iter::once(field_info.expr).chain(field_info.extra_exprs.iter().copied()).collect()
        };
        for expr_id in exprs {
            if let Some(vt) = self.resolve_expr_type(expr_id)
                && !types.contains(&vt) {
                    types.push(vt);
                }
        }
        if types.is_empty() {
            return "?".to_string();
        }
        let unified = ValueType::make_union(types);
        self.format_type_depth(&unified, depth + 1)
    }

    /// Format the `__call` metamethod signature for a callable table.
    /// Returns `None` if the table has no metamethod-based `call_func`.
    pub(super) fn format_call_signature(&self, table_idx: TableIndex) -> Option<String> {
        let table = self.table(table_idx);
        let func_idx = table.call_func?;
        if !table.call_func_is_metamethod { return None; }
        let func = self.func(func_idx);
        // The first parameter of a __call metamethod always receives the table
        // being called (the implicit receiver), regardless of its name — skip it
        // so the hover shows only the user-facing parameters.
        let skip = if !func.args.is_empty() { 1 } else { 0 };
        let args: Vec<String> = func.args.iter().enumerate().skip(skip).map(|(i, &sym_idx)| {
            let name = match &self.sym(sym_idx).id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => "?".to_string(),
            };
            let optional = func.param_optional.get(i).copied().unwrap_or(false);
            let ann_has_nil = func.param_annotations.get(i)
                .is_some_and(crate::annotations::annotation_type_is_nullable);
            let suffix = if optional && !ann_has_nil { "?" } else { "" };
            let type_str = self.param_annotation_text(func, i)
                .or_else(|| {
                    self.sym(sym_idx).versions.first()
                        .and_then(|v| v.resolved_type.as_ref())
                        .map(|rt| {
                            let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                            self.format_type_depth(&display_type, 1)
                        })
                });
            match type_str {
                Some(t) => format!("{}{}: {}", name, suffix, t),
                None => format!("{}{}", name, suffix),
            }
        }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let no_subs = HashMap::new();
        let rets: Vec<String> = if func.returns_self {
            vec![self.self_return_text(func, &no_subs)]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                let formatted = self.format_value_type_depth(vt, 1);
                format_vararg_return(formatted, i, func)
            }).collect()
        } else {
            self.format_inferred_returns(func, 1)
        };
        if rets.is_empty() {
            Some(format!("__call({})", all_args.join(", ")))
        } else if rets.len() == 1 {
            Some(format!("__call({}): {}", all_args.join(", "), rets[0]))
        } else {
            Some(format!("__call({}): {}", all_args.join(", "), join_returns(&rets)))
        }
    }

    /// Append `__call` signature to `type_str` and merge `__call` doc with existing doc.
    pub(super) fn append_call_hover(&self, table_idx: TableIndex, type_str: &mut String, base_doc: Option<String>) -> Option<String> {
        if let Some(call_sig) = self.format_call_signature(table_idx) {
            *type_str = format!("{}\n\n{}", type_str, call_sig);
        }
        let table = self.table(table_idx);
        if let Some(func_idx) = table.call_func.filter(|_| table.call_func_is_metamethod) {
            let call_doc = self.format_function_doc(func_idx);
            match (base_doc, call_doc) {
                (Some(td), Some(cd)) => Some(format!("{}\n\n{}", td, cd)),
                (Some(d), None) | (None, Some(d)) => Some(d),
                (None, None) => None,
            }
        } else {
            base_doc
        }
    }

    pub(crate) fn format_value_type_depth(&self, vt: &ValueType, depth: usize) -> String {
        // Safety net: prevent stack overflow from recursive types (e.g. table
        // whose value_type contains the same table via a recursive function).
        if depth > 8 { return "?".to_string(); }
        match vt {
            ValueType::Any => "any".to_string(),
            ValueType::Nil => "nil".to_string(),
            ValueType::Boolean(Some(true)) => "true".to_string(),
            ValueType::Boolean(Some(false)) => "false".to_string(),
            ValueType::Boolean(None) => "boolean".to_string(),
            ValueType::Number => "number".to_string(),
            ValueType::NumberLiteral(val) => val.clone(),
            ValueType::String(Some(val)) => format!("\"{}\"", val),
            ValueType::String(None) => "string".to_string(),
            ValueType::Function(Some(func_idx)) => {
                let primary = self.format_function_value(*func_idx, depth, None);
                let func = self.func(*func_idx);
                if func.overloads.is_empty() || depth > 0 {
                    primary
                } else {
                    let mut lines = vec![primary];
                    for overload in &func.overloads {
                        lines.push(self.format_overload(overload));
                    }
                    lines.join("\n")
                }
            }
            ValueType::Function(None) => "function".to_string(),
            ValueType::FunctionSig(shape) => self.format_function_shape(shape, depth),
            ValueType::Table(Some(table_idx)) => {
                let table = self.table(*table_idx);
                let overlay = self.ir.overlay_fields.get(table_idx);
                let has_fields = !table.fields.is_empty() || overlay.is_some_and(|o| !o.is_empty());
                // Array/map types: table has value_type and no class_name
                if table.class_name.is_none()
                    && let Some(ref val_vt) = table.value_type {
                        // Tighter limit than the outer depth > 8 guard: recursive
                        // functions (e.g. deep-copy) commonly produce tables whose
                        // value_type contains the same table, so cap early to avoid
                        // deep expansion before the general safety net kicks in.
                        if depth > 4 { return "table".to_string(); }
                        let val_str = self.format_value_type_depth(val_vt, depth + 1);
                        return match &table.key_type {
                            Some(ValueType::Number) | None if !table.is_explicit_map => {
                                if matches!(val_vt, ValueType::Union(_) | ValueType::Intersection(_)) {
                                    format!("({})[]", val_str)
                                } else {
                                    format!("{}[]", val_str)
                                }
                            }
                            Some(key_vt) => {
                                let key_str = self.format_value_type_depth(key_vt, depth + 1);
                                format!("table<{}, {}>", key_str, val_str)
                            }
                            // Defensive: explicit-map tables always have Some(key_type)
                            None => format!("{}[]", val_str),
                        };
                    }
                if let Some(ref class_name) = table.class_name {
                    let has_parents = !table.parent_classes.is_empty();
                    if (!has_fields && !has_parents) || depth > 0 {
                        return class_name.clone();
                    }
                    let indent = "  ".repeat(depth + 1);
                    let is_enum = table.enum_kind.is_enum();
                    let mut seen: HashSet<&str> = HashSet::new();
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        seen.insert(name.as_str());
                        self.format_enum_field_line(&indent, name, field_info, is_enum, depth)
                    }).collect();
                    if let Some(ov) = overlay {
                        for (name, field_info) in ov.iter() {
                            if seen.insert(name.as_str()) {
                                fields.push(self.format_enum_field_line(&indent, name, field_info, is_enum, depth));
                            }
                        }
                    }
                    // Include inherited fields from parent classes
                    for &parent_idx in &table.parent_classes {
                        let parent_table = self.table(parent_idx);
                        for (name, field_info) in &parent_table.fields {
                            if seen.insert(name.as_str()) {
                                fields.push(self.format_enum_field_line(&indent, name, field_info, is_enum, depth));
                            }
                        }
                    }
                    fields.sort();
                    if fields.is_empty() {
                        return class_name.clone();
                    }
                    return format!("{} {{\n{}\n}}", class_name, fields.join(",\n"));
                }
                if !has_fields || depth > 4 {
                    "table".to_string()
                } else if depth > 0 {
                    // Collapse sub-tables that contain methods or have many fields
                    // to keep hover readable (e.g. Auctionator.AH with 25 methods).
                    let has_methods = table.fields.values().any(|fi| {
                        matches!(self.expr(fi.expr), Expr::FunctionDef(_))
                    });
                    if (has_methods && table.fields.len() > 2) || table.fields.len() > 4 {
                        return format!("{{... {} fields}}", table.fields.len());
                    }
                    // Compact inline format for small nested anonymous tables
                    // (e.g. value_type in arrays: `{id: number, name: string}[]`)
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        let type_str = self.format_field_type(field_info, depth);
                        format!("{}: {}", name, type_str)
                    }).collect();
                    fields.sort();
                    format!("{{{}}}", fields.join(", "))
                } else {
                    let indent = "  ".repeat(depth + 1);
                    let mut fields: Vec<String> = table.fields.iter().map(|(name, field_info)| {
                        let type_str = self.format_field_type(field_info, depth);
                        format!("{}{}: {}", indent, name, type_str)
                    }).collect();
                    if let Some(ov) = overlay {
                        for (name, field_info) in ov.iter() {
                            if !table.fields.contains_key(name) {
                                let type_str = self.format_field_type(field_info, depth);
                                fields.push(format!("{}{}: {}", indent, name, type_str));
                            }
                        }
                    }
                    fields.sort();
                    format!("{{\n{}\n}}", fields.join(",\n"))
                }
            }
            ValueType::Table(None) => "table".to_string(),
            ValueType::Union(types) if types.is_empty() => "never".to_string(),
            ValueType::Union(types) if types.len() == 2
                && types.iter().any(|t| matches!(t, ValueType::Nil))
                && types.iter().any(|t| !matches!(t, ValueType::Nil)) =>
            {
                let other = types.iter().find(|t| !matches!(t, ValueType::Nil)).unwrap();
                let formatted = self.format_value_type_depth(other, depth + 1);
                if matches!(other, ValueType::Function(Some(_)) | ValueType::FunctionSig(_)) {
                    format!("({})?", formatted)
                } else {
                    format!("{}?", formatted)
                }
            }
            ValueType::Union(types) => {
                const MAX_STRING_LITERALS: usize = 3;
                let string_literal_count = types.iter().filter(|t| matches!(t, ValueType::String(Some(_)))).count();
                if string_literal_count > MAX_STRING_LITERALS {
                    let mut parts: Vec<String> = Vec::new();
                    let mut shown_strings = 0;
                    let mut total_unique_strings = 0;
                    for t in types {
                        if matches!(t, ValueType::String(Some(_))) {
                            let s = self.format_value_type_depth(t, depth + 1);
                            if !parts.contains(&s) {
                                total_unique_strings += 1;
                                if shown_strings < MAX_STRING_LITERALS {
                                    parts.push(s);
                                    shown_strings += 1;
                                }
                            }
                        } else {
                            let s = self.format_value_type_depth(t, depth + 1);
                            if !parts.contains(&s) { parts.push(s); }
                        }
                    }
                    let remaining = total_unique_strings - shown_strings;
                    if remaining > 0 {
                        parts.push(format!("({} more)", remaining));
                    }
                    parts.join(" | ")
                } else {
                    let mut parts: Vec<String> = Vec::new();
                    for t in types {
                        let s = self.format_value_type_depth(t, depth + 1);
                        if !parts.contains(&s) { parts.push(s); }
                    }
                    parts.join(" | ")
                }
            }
            ValueType::Intersection(types) => {
                let parts: Vec<String> = types.iter().map(|t| self.format_value_type_depth(t, depth + 1)).collect();
                parts.join(" & ")
            }
            ValueType::TypeVariable(name) => name.clone(),
            ValueType::Userdata => "userdata".to_string(),
            ValueType::Thread => "thread".to_string(),
            ValueType::OpaqueAlias(name, _) => name.clone(),
        }
    }

    /// Like `format_value_type_depth`, but substitutes class-level type
    /// variables (e.g. `T → string`) using `subs` before formatting. Used by
    /// hover on a method of a parameterized-class receiver so the displayed
    /// signature shows the concrete bound types instead of bare `T`. Falls back
    /// to the plain formatter whenever there's nothing to substitute, so it
    /// stays byte-for-byte identical to existing output in the common case.
    pub(super) fn format_type_subst(
        &self,
        vt: &ValueType,
        depth: usize,
        subs: &HashMap<String, ValueType>,
    ) -> String {
        if depth > 8 {
            return "?".to_string();
        }
        if subs.is_empty() || !self.type_contains_type_variable_deep(vt) {
            return self.format_value_type_depth(vt, depth);
        }
        match vt {
            ValueType::TypeVariable(name) => match subs.get(name) {
                Some(t) => self.format_value_type_depth(t, depth),
                None => name.clone(),
            },
            ValueType::Function(Some(func_idx)) => {
                self.format_function_value(*func_idx, depth, Some(subs))
            }
            ValueType::Table(Some(table_idx)) => {
                let table = self.table(*table_idx);
                // Array/map types (no class name, has value_type): recurse into
                // the element/key types so nested `T` is substituted.
                if table.class_name.is_none()
                    && let Some(ref val_vt) = table.value_type
                {
                    if depth > 4 {
                        return "table".to_string();
                    }
                    let val_str = self.format_type_subst(val_vt, depth + 1, subs);
                    return match &table.key_type {
                        Some(ValueType::Number) | None if !table.is_explicit_map => {
                            if matches!(val_vt, ValueType::Union(_) | ValueType::Intersection(_)) {
                                format!("({})[]", val_str)
                            } else {
                                format!("{}[]", val_str)
                            }
                        }
                        Some(key_vt) => {
                            let key_str = self.format_type_subst(key_vt, depth + 1, subs);
                            format!("table<{}, {}>", key_str, val_str)
                        }
                        None => format!("{}[]", val_str),
                    };
                }
                // Named class tables collapse to their name at depth > 0, so the
                // plain formatter is sufficient for nested class references.
                self.format_value_type_depth(vt, depth)
            }
            ValueType::Union(types)
                if types.len() == 2
                    && types.iter().any(|t| matches!(t, ValueType::Nil))
                    && types.iter().any(|t| !matches!(t, ValueType::Nil)) =>
            {
                let other = types.iter().find(|t| !matches!(t, ValueType::Nil)).unwrap();
                let formatted = self.format_type_subst(other, depth + 1, subs);
                if matches!(other, ValueType::Function(Some(_))) {
                    format!("({})?", formatted)
                } else {
                    format!("{}?", formatted)
                }
            }
            ValueType::Union(types) => {
                let parts: Vec<String> = types.iter()
                    .map(|t| self.format_type_subst(t, depth + 1, subs))
                    .collect();
                parts.join(" | ")
            }
            ValueType::Intersection(types) => {
                let parts: Vec<String> = types.iter()
                    .map(|t| self.format_type_subst(t, depth + 1, subs))
                    .collect();
                parts.join(" & ")
            }
            // OpaqueAlias always displays its alias name; the inner type is
            // not shown, so no substitution is needed. Explicit arm avoids
            // the `type_contains_type_variable_deep` guard triggering for an
            // inner TypeVariable and then falling through without effect.
            ValueType::OpaqueAlias(name, _) => name.clone(),
            _ => self.format_value_type_depth(vt, depth),
        }
    }

    /// Format an inline [`crate::types::FunctionShape`] (carried by
    /// `ValueType::FunctionSig`) as `fun(params): returns`. Mirrors the tail of
    /// `format_function_value` but reads the self-contained shape instead of an
    /// arena `Function`.
    pub(super) fn format_function_shape(&self, shape: &crate::types::FunctionShape, depth: usize) -> String {
        let mut all_args: Vec<String> = shape.params.iter().map(|p| {
            let suffix = if p.optional { "?" } else { "" };
            let type_str = self.format_value_type_depth(&p.ty, depth + 1);
            format!("{}{}: {}", p.name, suffix, type_str)
        }).collect();
        if shape.is_vararg {
            all_args.push("...".to_string());
        }
        let rets: Vec<String> = shape.returns.iter()
            .map(|vt| self.format_value_type_depth(vt, depth + 1))
            .collect();
        if rets.is_empty() {
            format!("fun({})", all_args.join(", "))
        } else if rets.len() == 1 {
            format!("fun({}): {}", all_args.join(", "), rets[0])
        } else {
            format!("fun({}): {}", all_args.join(", "), join_returns(&rets))
        }
    }

    /// Format a `fun(...)` value, optionally substituting class type variables
    /// via `subs`. Shared implementation used by both `format_value_type_depth`
    /// (no subs) and `format_type_subst` (with subs).
    pub(super) fn format_function_value(
        &self,
        func_idx: FunctionIndex,
        depth: usize,
        subs: Option<&HashMap<String, ValueType>>,
    ) -> String {
        let func = self.func(func_idx);
        let args: Vec<String> = func.args.iter().enumerate().map(|(i, &sym_idx)| {
            let name = match &self.sym(sym_idx).id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => "?".to_string(),
            };
            let optional = func.param_optional.get(i).copied().unwrap_or(false);
            let ann_has_nil = func.param_annotations.get(i)
                .is_some_and(crate::annotations::annotation_type_is_nullable);
            let suffix = if optional && !ann_has_nil { "?" } else { "" };
            let type_str = if let Some(s) = subs {
                self.param_annotation_text_subst(func, i, s)
            } else {
                self.param_annotation_text(func, i)
            }.or_else(|| {
                self.sym(sym_idx).versions.first()
                    .and_then(|v| v.resolved_type.as_ref())
                    .map(|rt| {
                        let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                        let effective_depth = if name == "self" && depth > 0 {
                            depth.max(5)
                        } else {
                            depth + 1
                        };
                        if let Some(s) = subs {
                            self.format_type_subst(&display_type, effective_depth, s)
                        } else {
                            self.format_type_depth(&display_type, effective_depth)
                        }
                    })
            });
            match type_str {
                Some(t) => format!("{}{}: {}", name, suffix, t),
                None => format!("{}{}", name, suffix),
            }
        }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let no_subs = HashMap::new();
        let effective_subs = subs.unwrap_or(&no_subs);
        // `func` already reflects any precise cross-file return types via the
        // per-file overlay (`func()` consults it for deferred external functions).
        let rets: Vec<String> = if func.returns_self {
            vec![self.self_return_text(func, effective_subs)]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                let formatted = if let Some(s) = subs {
                    self.format_type_subst(vt, depth + 1, s)
                } else {
                    self.format_value_type_depth(vt, depth + 1)
                };
                format_vararg_return(formatted, i, func)
            }).collect()
        } else {
            self.format_inferred_returns(func, depth + 1)
        };
        if rets.is_empty() {
            format!("fun({})", all_args.join(", "))
        } else if rets.len() == 1 {
            format!("fun({}): {}", all_args.join(", "), rets[0])
        } else {
            format!("fun({}): {}", all_args.join(", "), join_returns(&rets))
        }
    }

    /// Check if a symbol is a function parameter.
    pub(crate) fn is_param_symbol(&self, symbol_idx: SymbolIndex) -> bool {
        if symbol_idx.is_external() {
            return false;
        }
        self.ir.functions.iter().any(|f| f.args.contains(&symbol_idx))
    }

    /// Whether an EXT-space symbol came from the precomputed WoW API stubs
    /// (vs. being discovered by the workspace scan of user code).
    pub(crate) fn is_stub_symbol(&self, symbol_idx: SymbolIndex) -> bool {
        symbol_idx.is_external() && (symbol_idx.ext_offset()) < self.ir.ext.stub_symbols_end
    }

    pub(super) fn is_param_optional(&self, symbol_idx: SymbolIndex) -> bool {
        if symbol_idx.is_external() {
            return false;
        }
        for f in &self.ir.functions {
            if let Some(pos) = f.args.iter().position(|&s| s == symbol_idx) {
                return f.param_optional.get(pos).copied().unwrap_or(false);
            }
        }
        false
    }

    /// Find the raw `AnnotationType` for a param symbol by locating its function.
    pub(super) fn find_param_annotation_raw(&self, symbol_idx: SymbolIndex) -> Option<&crate::annotations::AnnotationType> {
        if symbol_idx.is_external() {
            return None;
        }
        for func in &self.ir.functions {
            if let Some(pos) = func.args.iter().position(|&s| s == symbol_idx) {
                return func.param_annotations.get(pos);
            }
        }
        None
    }

    /// If `ann` reduces to a single reference to a function-typed alias (optionally
    /// wrapped in `NonNil` or `Union(T, nil)`, and possibly chained through other
    /// aliases like `@alias A = B` where `B = fun(...)`), return the expanded
    /// `fun(...)` signature. Returns `None` for non-alias annotations, non-function
    /// aliases, or composite types like unions/intersections with multiple members.
    pub(super) fn expand_alias_fun_signature(&self, ann: &crate::annotations::AnnotationType) -> Option<String> {
        let (fun_ann, _) = crate::annotations::reduce_to_fun_alias(
            ann, &self.ir.alias_fun_types, &self.ir.ext.alias_fun_types,
        )?;
        Some(crate::annotations::format_annotation_type(fun_ann))
    }

    /// Find the annotation text for a param symbol by locating its function.
    /// Returns the formatted annotation with nil members stripped (since the
    /// caller adds `?` for optional/nil-containing types).
    pub(super) fn find_param_annotation_text(&self, symbol_idx: SymbolIndex) -> Option<String> {
        if symbol_idx.is_external() {
            return None;
        }
        for func in &self.ir.functions {
            if let Some(pos) = func.args.iter().position(|&s| s == symbol_idx) {
                let ann = func.param_annotations.get(pos)?;
                if matches!(ann, crate::annotations::AnnotationType::Simple(s) if s.is_empty()) {
                    return None;
                }
                if self.annotation_has_unresolvable(ann, &func.generics) {
                    return None;
                }
                // Strip nil from union annotations (added by `?` suffix syntax)
                return Some(Self::format_annotation_stripping_nil(ann));
            }
        }
        None
    }

    /// Format an annotation type, removing nil from union members.
    pub(super) fn format_annotation_stripping_nil(ann: &crate::annotations::AnnotationType) -> String {
        use crate::annotations::AnnotationType;
        if let AnnotationType::Union(parts) = ann {
            let non_nil: Vec<_> = parts.iter()
                .filter(|p| !matches!(p, AnnotationType::Simple(s) if s == "nil"))
                .collect();
            if non_nil.len() < parts.len() {
                // Had nil — format without it
                return non_nil.iter()
                    .map(|p| crate::annotations::format_annotation_type(p))
                    .collect::<Vec<_>>()
                    .join(" | ");
            }
        }
        crate::annotations::format_annotation_type(ann)
    }

    /// Get the formatted annotation text for a function parameter, if it has
    /// a non-empty annotation. This preserves alias names like `ThemeColorKey`
    /// instead of expanding them to their underlying union.
    /// Skips annotations containing unresolvable names (e.g. generic type
    /// variables from a parent scope like `T`), so the resolved concrete type
    /// is shown instead.
    pub(super) fn param_annotation_text(&self, func: &Function, param_idx: usize) -> Option<String> {
        let ann = func.param_annotations.get(param_idx)?;
        match ann {
            crate::annotations::AnnotationType::Simple(s) if s.is_empty() => None,
            _ => {
                if self.annotation_has_unresolvable(ann, &func.generics) {
                    return None;
                }
                // For named params, VarArgs(...) doesn't make sense — unwrap to
                // just the inner type (e.g. `...any?` → `any?`).
                let effective = match ann {
                    crate::annotations::AnnotationType::VarArgs(inner) => inner.as_ref(),
                    other => other,
                };
                let formatted = crate::annotations::format_annotation_type(effective);
                // If the formatted result is empty or just "?" (from VarArgs
                // wrapping an empty base type in old precomputed stubs), normalize
                // to "any?" since the annotation intent is "optional any value".
                if formatted.is_empty() {
                    return None;
                }
                if formatted == "?" {
                    return Some("any?".to_string());
                }
                Some(formatted)
            }
        }
    }

    /// Like `param_annotation_text` but substitutes class type variables (e.g.
    /// `T → string`) from `subs` into the raw annotation before formatting. This
    /// is what lets hover show concrete bound types for a method called on a
    /// parameterized-class receiver — the raw `@param func fun(value: T)` would
    /// otherwise short-circuit before any resolved-type substitution applied.
    pub(super) fn param_annotation_text_subst(
        &self,
        func: &Function,
        param_idx: usize,
        subs: &HashMap<String, ValueType>,
    ) -> Option<String> {
        if subs.is_empty() {
            return self.param_annotation_text(func, param_idx);
        }
        let ann = func.param_annotations.get(param_idx)?;
        let ann = self.substitute_annotation_type_vars(ann, subs);
        match &ann {
            crate::annotations::AnnotationType::Simple(s) if s.is_empty() => None,
            _ => {
                if self.annotation_has_unresolvable(&ann, &func.generics) {
                    return None;
                }
                let effective = match &ann {
                    crate::annotations::AnnotationType::VarArgs(inner) => inner.as_ref(),
                    other => other,
                };
                let formatted = crate::annotations::format_annotation_type(effective);
                if formatted.is_empty() {
                    return None;
                }
                if formatted == "?" {
                    return Some("any?".to_string());
                }
                Some(formatted)
            }
        }
    }

    /// Format the `@return self` text for a method, expanding to `self<X>` when
    /// the method has `@return self<X>` re-parameterization args. When `subs`
    /// contains bindings for type variables in the args (e.g. `T → string?`),
    /// the concrete types are shown instead of the raw annotation variables.
    pub(super) fn self_return_text(&self, func: &Function, subs: &HashMap<String, ValueType>) -> String {
        match &func.returns_self_type_args {
            Some(args) if !args.is_empty() => {
                let inner = args.iter()
                    .map(|arg| self.format_self_return_arg(arg, subs))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("self<{inner}>")
            }
            _ => "self".to_string(),
        }
    }

    /// Format a single `@return self<X>` type argument, substituting type
    /// variables from `subs`. Handles `Simple("T")` (bare type variables),
    /// `NonNil(Simple("T"))` (stripping nil from the substituted type), and
    /// `Union(members)` (recursively formatting each member with dedup).
    /// More complex annotation types containing type variables
    /// (e.g. `Parameterized("Foo", [Simple("T")])`) fall through to raw
    /// formatting without substitution.
    pub(super) fn format_self_return_arg(&self, arg: &crate::annotations::AnnotationType, subs: &HashMap<String, ValueType>) -> String {
        use crate::annotations::AnnotationType;
        match arg {
            AnnotationType::Simple(name) => {
                if let Some(vt) = subs.get(name.as_str()) {
                    self.format_type_depth(vt, 1)
                } else {
                    crate::annotations::format_annotation_type(arg)
                }
            }
            AnnotationType::NonNil(inner) => {
                if let AnnotationType::Simple(name) = inner.as_ref()
                    && let Some(vt) = subs.get(name.as_str())
                {
                    return self.format_type_depth(&vt.strip_nil(), 1);
                }
                crate::annotations::format_annotation_type(arg)
            }
            AnnotationType::Union(members) => {
                let mut parts: Vec<String> = Vec::new();
                for member in members {
                    let formatted = self.format_self_return_arg(member, subs);
                    if !parts.contains(&formatted) {
                        parts.push(formatted);
                    }
                }
                parts.join(" | ")
            }
            _ => crate::annotations::format_annotation_type(arg),
        }
    }

    /// Format the raw `@return` annotation at `index` for a parameterized class
    /// type (e.g. `Schema<T>` / `Schema<string?>`), applying any class type-var
    /// substitution from the receiver. The resolved `return_annotations` drop
    /// class type args (they're tracked out-of-band), so a return like
    /// `Schema<string?>` would otherwise display as the bare `Schema`; the raw
    /// annotation is the only place the `<...>` survives. Returns None when
    /// there is no raw annotation, when it is not a parameterized class type, or
    /// when it references an unresolvable type — the caller then falls back to
    /// the resolved return type formatting (preserving all other shapes such as
    /// aliases, `fun()`, plain classes, and primitives).
    pub(super) fn return_annotation_text_subst(
        &self,
        func: &Function,
        index: usize,
        subs: &HashMap<String, ValueType>,
    ) -> Option<String> {
        let raw = func.return_annotations_raw.get(index)?;
        let effective = match raw {
            crate::annotations::AnnotationType::VarArgs(inner) => inner.as_ref(),
            other => other,
        };
        // Only override the resolved formatting for parameterized class types,
        // whose type args the resolved return type discards. All other shapes
        // keep their existing resolved formatting.
        if !matches!(effective, crate::annotations::AnnotationType::Parameterized(..)) {
            return None;
        }
        // Gate on the *raw* annotation's type references, treating the
        // substitution keys (the class type vars being bound) as resolvable.
        // We can't gate on the post-substitution annotation because that
        // formats concrete types back into `Simple` leaves (e.g. `string?`),
        // which aren't valid type names and would be wrongly rejected.
        let mut gen_ctx: Vec<(String, Option<ValueType>)> = func.generics.clone();
        gen_ctx.extend(subs.keys().map(|k| (k.clone(), None)));
        if self.annotation_has_unresolvable(effective, &gen_ctx) {
            return None;
        }
        let ann = self.substitute_annotation_type_vars(effective, subs);
        let formatted = crate::annotations::format_annotation_type(&ann);
        if formatted.is_empty() || formatted == "?" {
            return None;
        }
        Some(formatted)
    }

    /// Recursively replace `Simple(name)` annotation leaves whose name is a key in
    /// `subs` with the formatted concrete type. Used by `param_annotation_text_subst`
    /// to render bound class type variables.
    pub(super) fn substitute_annotation_type_vars(
        &self,
        ann: &crate::annotations::AnnotationType,
        subs: &HashMap<String, ValueType>,
    ) -> crate::annotations::AnnotationType {
        use crate::annotations::{AnnotationType as AT, ParamInfo, TuplePosition};
        match ann {
            AT::Simple(s) => match subs.get(s) {
                Some(vt) => AT::Simple(self.format_value_type_depth(vt, 1)),
                None => ann.clone(),
            },
            AT::Union(parts) => AT::Union(parts.iter().map(|p| self.substitute_annotation_type_vars(p, subs)).collect()),
            AT::Intersection(parts) => AT::Intersection(parts.iter().map(|p| self.substitute_annotation_type_vars(p, subs)).collect()),
            AT::Array(inner) => AT::Array(Box::new(self.substitute_annotation_type_vars(inner, subs))),
            AT::Parameterized(base, args) => AT::Parameterized(base.clone(), args.iter().map(|a| self.substitute_annotation_type_vars(a, subs)).collect()),
            AT::Backtick(inner) => AT::Backtick(Box::new(self.substitute_annotation_type_vars(inner, subs))),
            AT::NonNil(inner) => AT::NonNil(Box::new(self.substitute_annotation_type_vars(inner, subs))),
            AT::VarArgs(inner) => AT::VarArgs(Box::new(self.substitute_annotation_type_vars(inner, subs))),
            AT::Fun(params, returns, is_vararg) => AT::Fun(
                params.iter().map(|p| ParamInfo {
                    name: p.name.clone(),
                    typ: self.substitute_annotation_type_vars(&p.typ, subs),
                    optional: p.optional,
                    description: p.description.clone(),
                }).collect(),
                returns.iter().map(|r| self.substitute_annotation_type_vars(r, subs)).collect(),
                *is_vararg,
            ),
            AT::TableLiteral(fields) => AT::TableLiteral(
                fields.iter().map(|(n, ft)| (n.clone(), self.substitute_annotation_type_vars(ft, subs))).collect(),
            ),
            AT::IndexedAccess(base, key) => {
                let substituted_base = subs.get(base)
                    .map(|vt| self.format_value_type_depth(vt, 1))
                    .unwrap_or_else(|| base.clone());
                AT::IndexedAccess(
                    substituted_base,
                    Box::new(self.substitute_annotation_type_vars(key, subs)),
                )
            }
            AT::Tuple(positions, desc) => AT::Tuple(
                positions.iter().map(|p| TuplePosition {
                    typ: self.substitute_annotation_type_vars(&p.typ, subs),
                    name: p.name.clone(),
                }).collect(),
                desc.clone(),
            ),
        }
    }

    /// Check if an annotation type contains any Simple names that can't be
    /// resolved to a known type, class, or alias. This detects stale generic
    /// type variables (e.g. `T` from a parent scope) that were substituted
    /// during resolution but remain in the raw annotation.
    pub(super) fn annotation_has_unresolvable(
        &self, ann: &crate::annotations::AnnotationType,
        generics: &[(String, Option<ValueType>)],
    ) -> bool {
        use crate::annotations::AnnotationType;
        match ann {
            AnnotationType::Simple(s) => {
                match s.as_str() {
                    "" | "nil" | "boolean" | "bool" | "true" | "false"
                    | "number" | "integer" | "string" | "table"
                    | "function" | "fun" | "any" | "self" => false,
                    _ if s.starts_with('"') || s.starts_with('\'') => false,
                    _ if s.starts_with("fun(") => false,
                    _ if generics.iter().any(|(g, _)| g == s) => false,
                    _ if self.ir.classes.contains_key(s) => false,
                    _ if self.ir.aliases.contains_key(s) => false,
                    _ if self.ir.ext.classes.contains_key(s.as_str()) => false,
                    _ if self.ir.ext.aliases.contains_key(s.as_str()) => false,
                    _ => true,
                }
            }
            AnnotationType::Union(parts) => parts.iter().any(|p| self.annotation_has_unresolvable(p, generics)),
            AnnotationType::Intersection(parts) => parts.iter().any(|p| self.annotation_has_unresolvable(p, generics)),
            AnnotationType::Array(inner) => self.annotation_has_unresolvable(inner, generics),
            AnnotationType::Parameterized(base, args) => {
                self.annotation_has_unresolvable(&AnnotationType::Simple(base.clone()), generics)
                    || args.iter().any(|a| self.annotation_has_unresolvable(a, generics))
            }
            AnnotationType::Backtick(inner) => self.annotation_has_unresolvable(inner, generics),
            AnnotationType::NonNil(inner) => self.annotation_has_unresolvable(inner, generics),
            AnnotationType::Fun(params, returns, _) => {
                params.iter().any(|p| self.annotation_has_unresolvable(&p.typ, generics))
                    || returns.iter().any(|r| self.annotation_has_unresolvable(r, generics))
            }
            AnnotationType::TableLiteral(fields) => {
                fields.iter().any(|(_, ft)| self.annotation_has_unresolvable(ft, generics))
            }
            AnnotationType::VarArgs(inner) => self.annotation_has_unresolvable(inner, generics),
            AnnotationType::IndexedAccess(base, key) => {
                (!generics.iter().any(|(g, _)| g == base)
                    && self.annotation_has_unresolvable(&AnnotationType::Simple(base.clone()), generics))
                || self.annotation_has_unresolvable(key, generics)
            }
            AnnotationType::Tuple(positions, _) => positions.iter().any(|p| self.annotation_has_unresolvable(&p.typ, generics)),
        }
    }

    /// Format a function in declaration style for hover: `function name(params)\n  -> ret`
    /// If `skip_self` is true, the first "self" parameter is omitted (colon-style methods).
    /// Format inferred return types (no `@return` annotation case). Returns
    /// empty when there are no value-returning return statements (void).
    /// When there are inferred returns and the function has an implicit nil
    /// return, nil is unioned into each resolved position.
    pub(crate) fn format_inferred_returns(&self, func: &Function, depth: usize) -> Vec<String> {
        // When synthesized return-only overloads exist, derive the summary type
        // per position by unioning across the overloads. This is more accurate
        // than reading FunctionRet symbols which may hold stale placeholder types
        // (e.g. `Any` from before the overload refinement fixpoint settles).
        let return_only: Vec<&ResolvedOverload> = func.overloads.iter()
            .filter(|o| o.is_return_only).collect();
        if !return_only.is_empty() {
            return Self::return_only_overload_summary(&return_only)
                .iter()
                .map(|t| self.format_type_depth(t, depth))
                .collect();
        }
        let inferred = dedup_return_types(&self.ir, &func.rets);
        let implicit_nil = func.implicit_nil_return;
        if inferred.is_empty() {
            return vec![];
        }
        inferred.iter().map(|rt| match rt.as_ref() {
            Some(rt) => {
                let display = if implicit_nil && !rt.contains_nil() && !matches!(rt, ValueType::Any) {
                    ValueType::make_union(vec![rt.clone(), ValueType::Nil])
                } else {
                    rt.clone()
                };
                self.format_type_depth(&display, depth)
            }
            // Unresolved position: leave as `?` — we don't know the type,
            // and artificially narrowing to `nil` would be misleading.
            None => "?".to_string(),
        }).collect()
    }

    /// Per-position summary `ValueType`s across the given return-only overloads:
    /// the union of each overload's slot type. Mirrors the overload branch of
    /// `format_inferred_returns`, but returns types rather than formatted strings
    /// so the cross-file deferred harvest can store the same per-slot summary the
    /// definition site displays (e.g. correlated `(number,number)|(nil,nil)` →
    /// `number?, number?`).
    pub(crate) fn return_only_overload_summary(return_only: &[&ResolvedOverload]) -> Vec<ValueType> {
        let max_arity = return_only.iter().map(|o| o.returns.len()).max().unwrap_or(0);
        (0..max_arity)
            .map(|pos| {
                let mut types: Vec<ValueType> = Vec::new();
                for o in return_only {
                    let vt = o.return_type_at(pos);
                    if !types.contains(&vt) {
                        types.push(vt);
                    }
                }
                ValueType::make_union(types)
            })
            .collect()
    }

    /// Per-slot inferred return `ValueType`s for a function with no `@return`,
    /// matching `format_inferred_returns` but yielding types (unresolved slots →
    /// `Any`). Used by the cross-file deferred harvest so a cross-file caller's
    /// base return slots equal the definition-site summary.
    pub(crate) fn inferred_return_types(&self, func: &Function) -> Vec<ValueType> {
        let return_only: Vec<&ResolvedOverload> = func.overloads.iter()
            .filter(|o| o.is_return_only).collect();
        if !return_only.is_empty() {
            return Self::return_only_overload_summary(&return_only);
        }
        let inferred = dedup_return_types(&self.ir, &func.rets);
        let implicit_nil = func.implicit_nil_return;
        inferred.into_iter().map(|rt| match rt {
            Some(rt) => {
                if implicit_nil && !rt.contains_nil() && !matches!(rt, ValueType::Any) {
                    ValueType::make_union(vec![rt, ValueType::Nil])
                } else {
                    rt
                }
            }
            None => ValueType::Any,
        }).collect()
    }

    /// Like `format_inferred_returns` but collapses anonymous shape tables for inlay hints.
    pub(super) fn format_inferred_returns_for_hint(&self, func: &Function) -> Vec<String> {
        // Same overload-based summary as format_inferred_returns.
        let return_only: Vec<&ResolvedOverload> = func.overloads.iter()
            .filter(|o| o.is_return_only).collect();
        if !return_only.is_empty() {
            let max_arity = return_only.iter().map(|o| o.returns.len()).max().unwrap_or(0);
            let mut result = Vec::new();
            for pos in 0..max_arity {
                let mut types: Vec<ValueType> = Vec::new();
                for o in &return_only {
                    let vt = o.return_type_at(pos);
                    if !types.contains(&vt) {
                        types.push(vt);
                    }
                }
                let merged = ValueType::make_union(types);
                result.push(self.format_type_for_hint(&merged));
            }
            return result;
        }
        let inferred = dedup_return_types(&self.ir, &func.rets);
        let implicit_nil = func.implicit_nil_return;
        if inferred.is_empty() {
            return vec![];
        }
        inferred.iter().map(|rt| match rt.as_ref() {
            Some(rt) => {
                let display = if implicit_nil && !rt.contains_nil() && !matches!(rt, ValueType::Any) {
                    ValueType::make_union(vec![rt.clone(), ValueType::Nil])
                } else {
                    rt.clone()
                };
                self.format_type_for_hint(&display)
            }
            None => "?".to_string(),
        }).collect()
    }

    pub(super) fn format_function_decl(
        &self,
        func_idx: FunctionIndex,
        name: &str,
        skip_self: bool,
        subs: Option<&HashMap<String, ValueType>>,
    ) -> String {
        let empty = HashMap::new();
        let subs = subs.unwrap_or(&empty);
        let func = self.func(func_idx);
        let args: Vec<String> = func.args.iter().enumerate()
            .filter(|&(i, &sym_idx)| {
                if skip_self && i == 0
                    && let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id {
                        return n != "self";
                    }
                true
            })
            .map(|(i, &sym_idx)| {
                let param_name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let optional = func.param_optional.get(i).copied().unwrap_or(false);
                let ann_has_nil = func.param_annotations.get(i)
                    .is_some_and(crate::annotations::annotation_type_is_nullable);
                let suffix = if optional && !ann_has_nil { "?" } else { "" };
                // Prefer raw annotation text (preserves alias names) over resolved type
                let type_str = self.param_annotation_text_subst(func, i, subs)
                    .or_else(|| {
                        // Use version 0 only (declaration type from @param), not a
                        // later version from type-guard narrowing in the body.
                        self.sym(sym_idx).versions.first()
                            .and_then(|v| v.resolved_type.as_ref())
                            .map(|rt| {
                                let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                                self.format_type_subst(&display_type, 1, subs)
                            })
                    });
                match type_str {
                    Some(t) => format!("{}{}: {}", param_name, suffix, t),
                    None => format!("{}{}", param_name, suffix),
                }
            }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        // `func` already reflects any precise cross-file return types and
        // correlated overloads via the per-file overlay (`func()` consults it for
        // deferred external functions), so plain `func.*` reads are precise.
        let rets: Vec<String> = if func.returns_self {
            vec![self.self_return_text(func, subs)]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                // Prefer the raw annotation (preserves `Parameterized` class
                // type args like `Schema<T>`) with the receiver's class type-var
                // substitution applied, so a method on a `Stream<string>` shows
                // `-> Schema<string>` instead of the bare resolved `Schema`.
                let formatted = self.return_annotation_text_subst(func, i, subs)
                    .unwrap_or_else(|| self.format_type_subst(vt, 1, subs));
                let with_vararg = format_vararg_return(formatted, i, func);
                // Prepend `label: ` if a label exists for this position
                match func.return_labels.get(i).and_then(|n| n.as_ref()) {
                    Some(label) => format!("{}: {}", label, with_vararg),
                    None => with_vararg,
                }
            }).collect()
        } else {
            self.format_inferred_returns(func, 1)
        };
        let args_joined = all_args.join(", ");
        let single_line = format!("function {}({})", name, args_joined);
        let mut result = if single_line.len() > 80 && all_args.len() > 1 {
            format!("function {}(\n  {}\n)", name, all_args.join(",\n  "))
        } else {
            single_line
        };
        if !rets.is_empty() {
            result.push_str(&format!("\n  -> {}", join_returns(&rets)));
        }
        // Partition overloads: return-only overloads render as a "cases:" table
        // below the primary signature (they don't vary the param list, so stacking
        // them as separate `function name(...)` blocks would be visual noise).
        // Regular overloads continue to stack above as before.
        if !func.overloads.is_empty() {
            for overload in &func.overloads {
                if overload.is_return_only { continue; }
                let ov_args: Vec<String> = overload.params.iter()
                    .filter(|p| !(skip_self && p.name == "self"))
                    .map(|p| {
                        let opt = if p.optional { "?" } else { "" };
                        match &p.typ {
                            Some(vt) => format!("{}{}: {}", p.name, opt, self.format_type_subst(vt, 1, subs)),
                            None => format!("{}{}", p.name, opt),
                        }
                    }).collect();
                let ov_rets: Vec<String> = if let Some(ref self_args) = overload.returns_self_type_args {
                    if self_args.is_empty() {
                        vec!["self".to_string()]
                    } else {
                        let inner = self_args.iter()
                            .map(|arg| self.format_self_return_arg(arg, subs))
                            .collect::<Vec<_>>()
                            .join(", ");
                        vec![format!("self<{inner}>")]
                    }
                } else {
                    overload.returns.iter()
                        .map(|vt| self.format_type_subst(vt, 1, subs))
                        .collect()
                };
                let ov_args_joined = ov_args.join(", ");
                let ov_single = format!("\nfunction {}({})", name, ov_args_joined);
                let mut ov_line = if ov_single.len() > 81 && ov_args.len() > 1 {
                    format!("\nfunction {}(\n  {}\n)", name, ov_args.join(",\n  "))
                } else {
                    ov_single
                };
                if !ov_rets.is_empty() {
                    ov_line.push_str(&format!("\n  -> {}", join_returns(&ov_rets)));
                }
                result.push_str(&ov_line);
            }

            // Return-only overloads → cases table. Synthesized cases (from
            // `synthesize_correlated_return_overloads`) have no `@return` source
            // and no descriptions — mark them as inferred so hover doesn't imply
            // the author wrote them.
            let return_only: Vec<&ResolvedOverload> = func.overloads.iter()
                .filter(|o| o.is_return_only).collect();
            if !return_only.is_empty() {
                let mut rows: Vec<(String, Option<String>)> = return_only.iter().map(|ovl| {
                    let parts: Vec<String> = ovl.returns.iter()
                        .map(|vt| self.format_type_subst(vt, 1, subs))
                        .collect();
                    (format!("({})", parts.join(", ")), ovl.description.clone())
                }).collect();
                // Deduplicate identical formatted tuples (can arise when
                // different annotation representations resolve to the same type).
                rows.dedup_by(|a, b| a.0 == b.0);
                let widest = rows.iter().map(|(t, _)| t.len()).max().unwrap_or(0);
                let synthesized = func.return_annotations.is_empty();
                result.push_str(if synthesized { "\n  cases (inferred):" } else { "\n  cases:" });
                for (tuple_str, desc) in rows {
                    match desc {
                        Some(d) => result.push_str(&format!("\n    {:<width$}  -- {}", tuple_str, d, width = widest)),
                        None => result.push_str(&format!("\n    {}", tuple_str)),
                    }
                }
            }
        }
        result
    }

    pub(super) fn format_overload(&self, overload: &ResolvedOverload) -> String {
        let args: Vec<String> = overload.params.iter().map(|p| {
            let opt = if p.optional { "?" } else { "" };
            match &p.typ {
                Some(vt) => format!("{}{}: {}", p.name, opt, self.format_value_type_depth(vt, 1)),
                None => format!("{}{}", p.name, opt),
            }
        }).collect();
        let rets: Vec<String> = overload.returns.iter()
            .map(|vt| self.format_value_type_depth(vt, 1))
            .collect();
        if rets.is_empty() {
            format!("fun({})", args.join(", "))
        } else if rets.len() == 1 {
            format!("fun({}): {}", args.join(", "), rets[0])
        } else {
            format!("fun({}): {}", args.join(", "), join_returns(&rets))
        }
    }
}
