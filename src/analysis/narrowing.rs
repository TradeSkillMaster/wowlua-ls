use crate::ast::*;
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::*;
use super::{Analysis, Ir};
use super::build_ir::OverloadCheck;

enum OrTermEffect {
    /// `x == nil` — value is nil
    IsNil,
    /// `type(x) == "number"` — value is a specific type
    TypeIs(ValueType),
}

/// How an `and`/`or` LHS guard narrows a symbol for the RHS.
#[derive(Clone)]
pub(super) enum GuardNarrow {
    /// Nil comparison (`x ~= nil and ...`): strip only nil
    StripNil,
    /// Bare truthiness (`x and ...`): strip nil and false
    StripFalsy,
    /// Type guard (`type(x) == "string" and ...`): filter union to matching types
    FilterTo(ValueType),
}

impl<'a> Analysis<'a> {
    /// Detect flavor-narrowing conditions and update scope_flavors accordingly.
    /// Handles:
    ///   `WOW_PROJECT_ID == WOW_PROJECT_<const>` (equality and negation)
    ///   A call to a function annotated with `@flavor-narrows`.
    /// Returns whether anything was narrowed.
    fn try_flavor_narrow(&mut self, cond: &Expression<'_>, parent_scope: ScopeIndex, target_scope: ScopeIndex, is_then_branch: bool) -> bool {
        if self.project_flavors == 0 { return false; }
        match cond {
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return false; }
                let terms = bin.get_terms();
                let [lhs, rhs] = match terms.as_slice() {
                    [a, b] => [a, b],
                    _ => return false,
                };
                // Match `WOW_PROJECT_ID == WOW_PROJECT_<const>` in either order.
                let const_name = Self::extract_wow_project_comparison(lhs, rhs);
                if let Some(ref name) = const_name {
                    let Some(const_bit) = crate::flavor::wow_project_constant_flavor(name) else { return false };
                    // Both equality and negation contribute flavor narrowing: the
                    // then-branch of `==` narrows to `const_bit`, the else-branch
                    // excludes it. `~=` flips the sense.
                    let narrow_to_bit = (is_eq && is_then_branch) || (is_neq && !is_then_branch);
                    if narrow_to_bit {
                        self.narrow_scope_flavors(target_scope, const_bit);
                    } else {
                        self.exclude_scope_flavors(target_scope, const_bit);
                    }
                    return true;
                }
                false
            }
            // Call to a flavor-guard function — narrow in then-branch, exclude in else-branch.
            Expression::FunctionCall(call) => {
                let Some(mask) = self.flavor_guard_mask_for_call(call, parent_scope) else { return false };
                if is_then_branch {
                    self.narrow_scope_flavors(target_scope, mask);
                } else {
                    self.exclude_scope_flavors(target_scope, mask);
                }
                true
            }
            // Boolean variable or field annotated with `@flavor-narrows`.
            Expression::Identifier(ident) => {
                let Some(mask) = self.flavor_guard_mask_for_ident(ident, parent_scope) else { return false };
                if is_then_branch {
                    self.narrow_scope_flavors(target_scope, mask);
                } else {
                    self.exclude_scope_flavors(target_scope, mask);
                }
                true
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    return self.try_flavor_narrow(&inner, parent_scope, target_scope, is_then_branch);
                }
                false
            }
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                if let Some(inner) = u.get_terms().into_iter().next() {
                    return self.try_flavor_narrow(&inner, parent_scope, target_scope, !is_then_branch);
                }
                false
            }
            _ => false,
        }
    }

    /// If `lhs/rhs` is `WOW_PROJECT_ID` compared against a `WOW_PROJECT_*`
    /// constant name in either order, return the constant name.
    fn extract_wow_project_comparison(lhs: &Expression<'_>, rhs: &Expression<'_>) -> Option<String> {
        let is_project_id = |e: &Expression<'_>| -> bool {
            if let Expression::Identifier(ident) = e {
                let names = ident.names();
                names.len() == 1 && names[0] == "WOW_PROJECT_ID"
            } else { false }
        };
        let project_constant = |e: &Expression<'_>| -> Option<String> {
            if let Expression::Identifier(ident) = e {
                let names = ident.names();
                if names.len() == 1 && names[0].starts_with("WOW_PROJECT_") && names[0] != "WOW_PROJECT_ID" {
                    return Some(names[0].clone());
                }
            }
            None
        };
        if is_project_id(lhs) {
            return project_constant(rhs);
        }
        if is_project_id(rhs) {
            return project_constant(lhs);
        }
        None
    }

    /// If `call` resolves to a function annotated with `@flavor-narrows`,
    /// return the guard mask. During build_ir, symbol `resolved_type` is not
    /// yet populated for local symbols, so walk `type_source` to find the
    /// FunctionDef / Table referenced by each name in the dotted chain.
    fn flavor_guard_mask_for_call(&self, call: &crate::ast::FunctionCall<'_>, parent_scope: ScopeIndex) -> Option<u8> {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.is_empty() { return None; }

        let sym_id = SymbolIdentifier::Name(names[0].clone());
        let sym_idx = self.get_symbol(&sym_id, parent_scope)?;

        if names.len() == 1 {
            // Single-name call: resolve the symbol to a function.
            let func_idx = self.find_function_for_symbol(sym_idx, parent_scope)?;
            let g = self.func(func_idx).flavor_guard;
            if g != 0 { return Some(g); }
            return None;
        }

        // Dotted path: resolve root symbol to a table, then walk fields.
        let mut table_idx = self.find_table_for_symbol_phase1(sym_idx, parent_scope)?;
        for name in &names[1..names.len() - 1] {
            let fi = self.ir.get_field(table_idx, name)?;
            match self.ir.expr(fi.expr) {
                Expr::TableConstructor(i) => table_idx = *i,
                Expr::Literal(ValueType::Table(Some(i))) => table_idx = *i,
                _ => return None,
            }
        }
        let final_name = names.last()?;
        let fi = self.ir.get_field(table_idx, final_name)?;
        match self.ir.expr(fi.expr) {
            Expr::FunctionDef(func_idx) => {
                let g = self.func(*func_idx).flavor_guard;
                if g != 0 { return Some(g); }
            }
            Expr::Literal(ValueType::Function(Some(func_idx))) => {
                let g = self.func(*func_idx).flavor_guard;
                if g != 0 { return Some(g); }
            }
            _ => {}
        }
        None
    }

    /// If `ident` resolves to a symbol or field annotated with `@flavor-narrows`,
    /// return the guard mask.
    fn flavor_guard_mask_for_ident(&self, ident: &crate::ast::Identifier<'_>, parent_scope: ScopeIndex) -> Option<u8> {
        let names = ident.names();
        if names.is_empty() { return None; }

        let sym_id = SymbolIdentifier::Name(names[0].clone());
        let sym_idx = self.get_symbol(&sym_id, parent_scope)?;

        if names.len() == 1 {
            let g = self.sym(sym_idx).flavor_guard;
            if g != 0 { return Some(g); }
            return None;
        }

        let mut table_idx = self.find_table_for_symbol_phase1(sym_idx, parent_scope)?;
        for name in &names[1..names.len() - 1] {
            let fi = self.ir.get_field(table_idx, name)?;
            match self.ir.expr(fi.expr) {
                Expr::TableConstructor(i) => table_idx = *i,
                Expr::Literal(ValueType::Table(Some(i))) => table_idx = *i,
                _ => return None,
            }
        }
        let final_name = names.last()?;
        let fi = self.ir.get_field(table_idx, final_name)?;
        if fi.flavor_guard != 0 { return Some(fi.flavor_guard); }
        None
    }

    /// Walk a symbol's `type_source` to find a FunctionDef. Handles both
    /// external symbols (read via resolved_type) and local ones (read via
    /// type_source, since resolved_type is only populated in Phase 2).
    fn find_function_for_symbol(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<FunctionIndex> {
        let ver_idx = self.ir.version_for_scope(sym_idx, scope_idx);
        if sym_idx.is_external() {
            let rt = self.sym(sym_idx).versions.get(ver_idx)?.resolved_type.as_ref()?;
            if let ValueType::Function(Some(func_idx)) = rt {
                return Some(*func_idx);
            }
            return None;
        }
        let type_source = self.sym(sym_idx).versions.get(ver_idx)?.type_source?;
        self.find_function_def(type_source)
    }

    /// Walk a symbol's `type_source` to find a TableIndex, phase-1 compatible
    /// (doesn't rely on `resolved_type` being populated).
    fn find_table_for_symbol_phase1(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<TableIndex> {
        let ver_idx = self.ir.version_for_scope(sym_idx, scope_idx);
        if sym_idx.is_external() {
            let rt = self.sym(sym_idx).versions.get(ver_idx)?.resolved_type.as_ref()?;
            if let ValueType::Table(Some(idx)) = rt {
                return Some(*idx);
            }
            return None;
        }
        let type_source = self.sym(sym_idx).versions.get(ver_idx)?.type_source?;
        self.ir.find_table_index(type_source)
    }

    /// Walk an expression ID to find a FunctionDef at the end (follows SymbolRef /
    /// Literal(Function(_)) / FunctionDef / Grouped chains). Used during build_ir
    /// before types are fully resolved.
    fn find_function_def(&self, expr_id: ExprId) -> Option<FunctionIndex> {
        match self.ir.expr(expr_id) {
            Expr::FunctionDef(idx) => Some(*idx),
            Expr::Literal(ValueType::Function(Some(idx))) => Some(*idx),
            Expr::Grouped(inner) => self.find_function_def(*inner),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let ts = self.sym(*sym_idx).versions.get(*ver_idx)?.type_source?;
                self.find_function_def(ts)
            }
            _ => None,
        }
    }

    pub(super) fn analyze_nil_guard(&mut self, cond: &Expression<'_>, parent_scope: ScopeIndex, target_scope: ScopeIndex, is_then_branch: bool) {
        // Flavor narrowing (project-flavors-aware). Returns whether anything
        // matched — but we still fall through so the usual nil/type-guard
        // logic also runs for unrelated conditions.
        self.try_flavor_narrow(cond, parent_scope, target_scope, is_then_branch);
        self.analyze_nil_guard_inner(cond, parent_scope, target_scope, is_then_branch);
    }

    fn analyze_nil_guard_inner(&mut self, cond: &Expression<'_>, parent_scope: ScopeIndex, target_scope: ScopeIndex, is_then_branch: bool) {
        match cond {
            // `if x then` or `if self.field then` — bare truthiness guard.
            // Also handles falsy-direction via recursion from `UnaryExpression(Not)` and
            // from the `else` branch of explicit `if/else` chains.
            Expression::Identifier(ident) => {
                if is_then_branch {
                    let names = ident.names_with_brackets();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                            self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                            self.falsy_narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                            self.narrow_siblings(sym_idx, target_scope);
                            self.narrow_correlated_locals(sym_idx, target_scope, true);
                            self.narrow_or_coalesce_derived(sym_idx, target_scope, true);
                        }
                    } else if !ident.has_any_dynamic_bracket() {
                        self.try_narrow_field_falsy(&names, target_scope);
                    }
                } else {
                    // Falsy-direction: bare `x` with is_then_branch=false means we're in the
                    // else-branch of `if x then` or the then-branch of `if not x then`.
                    // Mark x as falsy-narrowed so multi-return siblings can be filtered by
                    // return-only overloads whose position at x is truthy-only.
                    let names = ident.names();
                    if names.len() == 1
                        && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                            self.truthy_narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                            self.narrow_siblings(sym_idx, target_scope);
                        }
                }
            }
            // `if x ~= nil then` or `if x == nil then`
            // `if type(x) == "string" then` (any non-nil type literal)
            // `if a and b then` — recurse into both sides
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                // `a and b` — both conditions hold in the then-branch.
                // Also handle Operator::None which the parser produces for the outer
                // grouping node of chained binary expressions like `a == b and c == d`.
                if matches!(op, Operator::And | Operator::None) && is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.try_flavor_narrow(term, parent_scope, target_scope, true);
                            self.analyze_nil_guard_inner(term, parent_scope, target_scope, true);
                        }
                        return;
                    }
                }
                // `a or b` in then-branch: at least one is true.
                // If all terms narrow the same symbol, the result is the union of
                // what each term narrows to. E.g. `x == nil or type(x) == "number"`
                // narrows x to `nil | number`.
                if matches!(op, Operator::Or) && is_then_branch {
                    let terms = Self::flatten_or_terms(&Expression::BinaryExpression(*bin));
                    if terms.len() >= 2 {
                        self.try_or_then_narrowing(&terms, parent_scope, target_scope);
                        return;
                    }
                }
                // `a or b` in else-branch: NOT (a OR b) = NOT a AND NOT b
                // Both conditions are false, so apply inverse narrowing to both.
                if matches!(op, Operator::Or) && !is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.try_flavor_narrow(term, parent_scope, target_scope, false);
                            self.analyze_nil_guard_inner(term, parent_scope, target_scope, false);
                        }
                        return;
                    }
                }
                let is_neq = matches!(op, Operator::NotEquals);
                let is_eq = matches!(op, Operator::Equals);
                if !is_neq && !is_eq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Check for nil comparison: `x ~= nil` / `x == nil`
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names_with_brackets();
                        let should_narrow = (is_neq && is_then_branch) || (is_eq && !is_then_branch);
                        if should_narrow {
                            if names.len() == 1 {
                                if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                                    self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                    self.narrow_siblings(sym_idx, target_scope);
                                    self.narrow_correlated_locals(sym_idx, target_scope, false);
                                    self.narrow_or_coalesce_derived(sym_idx, target_scope, false);
                                }
                            } else if !ident.has_any_dynamic_bracket() {
                                self.try_narrow_field(&names, target_scope);
                            }
                        }
                    }
                    // Check for type() guard: `type(x) == "string"` etc.
                    // Also handles cached pattern: `local t = type(x); if t == "string"`
                    let is_positive_type_guard = (is_eq && is_then_branch) || (is_neq && !is_then_branch);
                    let is_inverse_type_guard = (is_eq && !is_then_branch) || (is_neq && is_then_branch);
                    if is_positive_type_guard || is_inverse_type_guard {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, parent_scope)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, parent_scope));
                        if let Some(sym_idx) = guard_sym {
                            if let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if type_name == "nil" {
                                    // `type(x) == "nil"` → positive means x IS nil (no narrowing needed),
                                    // inverse means x is NOT nil (strip nil)
                                    if is_inverse_type_guard {
                                        self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                        self.narrow_siblings(sym_idx, target_scope);
                                        self.narrow_or_coalesce_derived(sym_idx, target_scope, false);
                                    }
                                } else if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    if is_positive_type_guard {
                                        self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                        self.narrow_siblings(sym_idx, target_scope);
                                        self.type_filtered_symbols.entry(target_scope).or_default()
                                            .insert(sym_idx, vt);
                                        self.narrow_or_coalesce_derived(sym_idx, target_scope, false);
                                    } else {
                                        self.add_type_stripped(target_scope, sym_idx, vt.clone());
                                        self.push_strip_type_version(sym_idx, vt, target_scope, false);
                                    }
                                }
                            } else if is_positive_type_guard {
                                // No type name literal but still a type guard (shouldn't happen, but keep existing behavior)
                                self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                self.narrow_siblings(sym_idx, target_scope);
                                self.narrow_or_coalesce_derived(sym_idx, target_scope, false);
                            }
                        }
                        // Field type guard: `type(obj.field) == "string"`
                        if guard_sym.is_none()
                            && let Some((sym_idx, chain)) = self.extract_type_guard_field(lhs, rhs, parent_scope)
                                && let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                    if type_name == "nil" {
                                        // `type(obj.f) ~= "nil"` → strip nil
                                        if is_inverse_type_guard {
                                            self.narrowed_fields.entry(target_scope).or_default()
                                                .insert((sym_idx, chain));
                                        }
                                    } else if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                        if is_positive_type_guard {
                                            self.narrowed_fields.entry(target_scope).or_default()
                                                .insert((sym_idx, chain.clone()));
                                            self.type_narrowed_fields.entry(target_scope).or_default()
                                                .insert((sym_idx, chain), vt);
                                        } else {
                                            // Inverse: strip the guarded type from the field's union
                                            self.type_stripped_fields.entry(target_scope).or_default()
                                                .insert((sym_idx, chain), vt);
                                        }
                                    }
                                }
                    }
                    // Class-equality narrowing: `x == Foo.Bar` where `Foo.Bar` is class-typed.
                    // Only positive then-branch (or negative else-branch) is useful;
                    // the opposite direction doesn't produce a clean subtraction on a class.
                    let is_positive_class_eq = (is_eq && is_then_branch) || (is_neq && !is_then_branch);
                    if is_positive_class_eq
                        && Self::extract_type_name_literal(lhs, rhs).is_none()
                        && !Self::is_nil_literal(lhs) && !Self::is_nil_literal(rhs)
                    {
                        self.record_class_eq_deferral(lhs, rhs, parent_scope, target_scope);
                    }
                    // Event-param narrowing: `event == "EVENT_NAME"` where event is
                    // the event param of a function with event_params annotation.
                    if is_positive_class_eq {
                        self.try_event_param_narrowing(lhs, rhs, parent_scope, target_scope);
                    }
                }
            }
            // Unwrap grouping: `if (x) then`
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.analyze_nil_guard_inner(&inner, parent_scope, target_scope, is_then_branch);
                }
            }
            // Custom type guard: `if IsType(x, "Foo") then`
            // Also handles literal-bool union discrimination: `if x:IsSubRow() then`
            Expression::FunctionCall(call) => {
                if let Some((sym_idx, class_name)) = self.extract_type_narrows_guard(call, parent_scope) {
                    // @type-narrows only narrows in then-branch (no else-branch semantic)
                    if is_then_branch {
                        self.apply_type_narrows(sym_idx, &class_name, target_scope);
                    }
                } else if let Some((sym_idx, true_type, false_type)) = self.extract_bool_discriminator(call, parent_scope) {
                    let narrowed = if is_then_branch { true_type } else { false_type };
                    self.type_narrowed_symbols.entry(target_scope).or_default()
                        .insert(sym_idx, narrowed);
                } else if let Some((sym_idx, chain, true_type, false_type)) = self.extract_bool_discriminator_field(call, parent_scope) {
                    let narrowed = if is_then_branch { true_type } else { false_type };
                    self.type_narrowed_fields.entry(target_scope).or_default()
                        .insert((sym_idx, chain), narrowed);
                }
            }
            // `not expr` flips the branch sense
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                if let Some(inner) = u.get_terms().into_iter().next() {
                    self.analyze_nil_guard_inner(&inner, parent_scope, target_scope, !is_then_branch);
                }
            }
            _ => {}
        }
    }

    /// For `a or b` in then-branch, try to narrow if all terms constrain the same
    /// symbol. The narrowed type is the union of each term's effect.
    fn try_or_then_narrowing(&mut self, terms: &[Expression<'_>], parent_scope: ScopeIndex, target_scope: ScopeIndex) {
        // Collect what each term narrows
        let mut effects: Vec<(SymbolIndex, OrTermEffect)> = Vec::new();
        for term in terms {
            if let Some(effect) = self.extract_or_term_effect(term, parent_scope) {
                effects.push(effect);
            } else {
                return; // A term doesn't narrow any symbol — can't narrow overall
            }
        }
        // Check all terms narrow the same symbol
        let target_sym = effects[0].0;
        if !effects.iter().all(|(s, _)| *s == target_sym) {
            return;
        }
        // Build union of narrowed types
        let mut union_types: Vec<ValueType> = Vec::new();
        for (_, effect) in &effects {
            match effect {
                OrTermEffect::IsNil => {
                    if !union_types.contains(&ValueType::Nil) {
                        union_types.push(ValueType::Nil);
                    }
                }
                OrTermEffect::TypeIs(vt) => {
                    if !union_types.contains(vt) {
                        union_types.push(vt.clone());
                    }
                }
            }
        }
        if union_types.is_empty() { return; }
        let combined = if union_types.len() == 1 {
            union_types.into_iter().next().unwrap()
        } else {
            ValueType::Union(union_types)
        };
        let has_nil = matches!(&combined, ValueType::Nil)
            || matches!(&combined, ValueType::Union(ts) if ts.contains(&ValueType::Nil));
        self.type_narrowed_symbols.entry(target_scope).or_default()
            .insert(target_sym, combined);
        if !has_nil {
            self.narrowed_symbols.entry(target_scope).or_default().insert(target_sym);
            self.narrow_or_coalesce_derived(target_sym, target_scope, false);
        }
        self.narrow_siblings(target_sym, target_scope);
    }

    /// Extract the narrowing effect of a single comparison term in an `or` chain
    /// (then-branch context). Returns the symbol and what it's narrowed to.
    fn extract_or_term_effect(&self, term: &Expression<'_>, parent_scope: ScopeIndex) -> Option<(SymbolIndex, OrTermEffect)> {
        match term {
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return None; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // `x == nil` → IsNil
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1
                            && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                                if is_eq {
                                    return Some((sym_idx, OrTermEffect::IsNil));
                                }
                                // x ~= nil in an or-then context doesn't produce a useful positive constraint
                                return None;
                            }
                    }
                    // `type(x) == "number"` → TypeIs(Number)
                    if is_eq {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, parent_scope)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, parent_scope));
                        if let Some(sym_idx) = guard_sym
                            && let Some(type_name) = Self::extract_type_name_literal(lhs, rhs)
                                && let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    return Some((sym_idx, OrTermEffect::TypeIs(vt)));
                                }
                    }
                }
                None
            }
            Expression::GroupedExpression(g) => {
                g.get_expression().and_then(|inner| self.extract_or_term_effect(&inner, parent_scope))
            }
            _ => None,
        }
    }

    /// Flatten nested `or` binary expressions into a flat list of leaf terms.
    /// `(a or b) or c` → `[a, b, c]`
    fn flatten_or_terms<'b>(expr: &Expression<'b>) -> Vec<Expression<'b>> {
        match expr {
            Expression::BinaryExpression(bin) if matches!(bin.kind(), Operator::Or) => {
                bin.get_terms().iter().flat_map(|t| Self::flatten_or_terms(t)).collect()
            }
            other => {
                vec![Expression::cast(other.syntax()).unwrap()]
            }
        }
    }

    /// Early-exit narrowing: if the then-branch always exits and the condition
    /// implies the variable is nil/falsy, narrow it as non-nil in the parent scope.
    /// Patterns: `if not x then error() end`, `if x == nil then return end`
    pub(super) fn analyze_early_exit_guard(&mut self, cond: &Expression<'_>, scope_idx: ScopeIndex) {
        // If the exit condition is a flavor check (e.g. `if WOW_PROJECT_ID ==
        // WOW_PROJECT_MAINLINE then return end`), exclude that flavor from the
        // active set after the guard — i.e. treat it as the else-branch narrowing.
        self.try_flavor_narrow(cond, scope_idx, scope_idx, false);
        match cond {
            // `if x then return end` → x is falsy in the outer scope after.
            // Mainly useful for multi-return sibling narrowing on return-only overloads.
            Expression::Identifier(ident) => {
                let names = ident.names();
                if names.len() == 1
                    && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                        self.truthy_narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
                        self.narrow_siblings(sym_idx, scope_idx);
                    }
            }
            // `if not x then error()/return end` → x is truthy after (strip nil + false)
            // `if not IsType(x, "Foo") then return end` → x IS Foo after
            Expression::UnaryExpression(unary) => {
                if !matches!(unary.kind(), Operator::Not) { return; }
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names_with_brackets();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                            self.narrow_symbol_strip_falsy(sym_idx, scope_idx);
                        }
                    } else if !ident.has_any_dynamic_bracket() {
                        self.try_narrow_field_falsy(&names, scope_idx);
                    }
                } else if let Some(Expression::FunctionCall(call)) = terms.first() {
                    if let Some((sym_idx, class_name)) = self.extract_type_narrows_guard(call, scope_idx) {
                        self.apply_type_narrows(sym_idx, &class_name, scope_idx);
                    } else if let Some((sym_idx, true_type, _)) = self.extract_bool_discriminator(call, scope_idx) {
                        // `if not x:IsSubRow() then return end` → x is the true-branch after
                        self.type_narrowed_symbols.entry(scope_idx).or_default()
                            .insert(sym_idx, true_type);
                    } else if let Some((sym_idx, chain, true_type, _)) = self.extract_bool_discriminator_field(call, scope_idx) {
                        self.type_narrowed_fields.entry(scope_idx).or_default()
                            .insert((sym_idx, chain), true_type);
                    }
                }
            }
            // `if x == nil then error()/return end` → x is non-nil after
            // `if type(x) == "boolean" then return end` → x has boolean stripped after
            // `if a or b then return end` → both a and b are false after
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                // `a or b` in early-exit: NOT (a OR b) = NOT a AND NOT b
                if matches!(op, Operator::Or) {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.analyze_early_exit_guard(term, scope_idx);
                        }
                        return;
                    }
                }
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Nil comparison: `x == nil then return end` → strip nil
                    if is_eq {
                        let ident_expr = if Self::is_nil_literal(rhs) {
                            Some(lhs)
                        } else if Self::is_nil_literal(lhs) {
                            Some(rhs)
                        } else {
                            None
                        };
                        if let Some(Expression::Identifier(ident)) = ident_expr {
                            let names = ident.names_with_brackets();
                            if names.len() == 1 {
                                if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                                    self.narrow_symbol_strip_nil(sym_idx, scope_idx);
                                }
                            } else if !ident.has_any_dynamic_bracket() {
                                self.try_narrow_field(&names, scope_idx);
                            }
                        }
                    }
                    // Type guard early exit: `if type(x) == "boolean" then return end`
                    // → strip boolean from x in parent scope (inverse of then-branch)
                    let strip_type_guard = is_eq;
                    let narrow_type_guard = is_neq;
                    if strip_type_guard || narrow_type_guard {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, scope_idx)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, scope_idx));
                        if let Some(sym_idx) = guard_sym
                            && let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if type_name == "nil" {
                                    // `if type(x) == "nil" then return end` → x is NOT nil after
                                    if strip_type_guard {
                                        self.narrow_symbol_strip_nil(sym_idx, scope_idx);
                                    }
                                    // `if type(x) ~= "nil" then return end` → x IS nil after (no useful narrowing)
                                } else if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    if strip_type_guard {
                                        self.add_type_stripped(scope_idx, sym_idx, vt.clone());
                                        // Use ancestors-only lookup to avoid picking up
                                        // then-branch versions that would corrupt the result.
                                        self.push_strip_type_version(sym_idx, vt.clone(), scope_idx, true);
                                    } else {
                                        self.type_filtered_symbols.entry(scope_idx).or_default()
                                            .insert(sym_idx, vt.clone());
                                        // Use ancestors-only lookup to avoid picking up
                                        // then-branch versions that would corrupt the result.
                                        self.push_type_filter_version(sym_idx, vt, scope_idx, true);
                                    }
                                }
                            }
                        // Field type guard early exit: `if type(obj.field) == "table" then return end`
                        if guard_sym.is_none()
                            && let Some((sym_idx, chain)) = self.extract_type_guard_field(lhs, rhs, scope_idx)
                                && let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                    if type_name == "nil" {
                                        // `if type(obj.f) == "nil" then return end` → obj.f is NOT nil after
                                        if strip_type_guard {
                                            self.narrowed_fields.entry(scope_idx).or_default()
                                                .insert((sym_idx, chain));
                                        }
                                    } else if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                        if strip_type_guard {
                                            // `if type(obj.f) == "table" then return end`
                                            // → obj.f is NOT table after, strip that type
                                            self.type_stripped_fields.entry(scope_idx).or_default()
                                                .insert((sym_idx, chain), vt);
                                        } else {
                                            // `if type(obj.f) ~= "table" then return end`
                                            // → obj.f IS table after
                                            self.narrowed_fields.entry(scope_idx).or_default()
                                                .insert((sym_idx, chain.clone()));
                                            self.type_narrowed_fields.entry(scope_idx).or_default()
                                                .insert((sym_idx, chain), vt);
                                        }
                                    }
                                }
                    }
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.analyze_early_exit_guard(&inner, scope_idx);
                }
            }
            _ => {}
        }
    }

    /// Ensure-initialized narrowing: detects `if not FIELD then FIELD = val end`
    /// and narrows FIELD as non-nil in the parent scope.
    /// Also handles `if FIELD == nil then FIELD = val end`.
    pub(super) fn analyze_ensure_initialized(&mut self, cond: &Expression<'_>, block: &Block<'_>, scope_idx: ScopeIndex) {
        let guarded_names = self.extract_nil_guard_field(cond);
        if guarded_names.len() < 2 { return; }
        // Check if the then-block assigns to the same field
        if Self::block_assigns_field(block, &guarded_names) {
            self.try_narrow_field(&guarded_names, scope_idx);
        }
    }

    /// Extract symbols from a nil-guard condition that would be non-nil when the
    /// condition is false. Returns `(SymbolIndex, strip_falsy, var_name)` tuples.
    ///
    /// - `not x` → (x, strip_falsy=true, "x") because `not x` false means x is truthy
    /// - `x == nil` → (x, strip_falsy=false, "x") because `x == nil` false means x is non-nil
    ///
    /// Static method to avoid borrow conflicts during if-statement processing.
    pub(super) fn extract_nil_guard_symbols(cond: &Expression<'_>, out: &mut Vec<(SymbolIndex, bool, String)>, ir: &Ir, scope_idx: ScopeIndex) {
        match cond {
            // `not x` → x is truthy (strip falsy) when condition is false
            Expression::UnaryExpression(unary) if unary.kind() == Operator::Not => {
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1
                        && let Some(sym_idx) = ir.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                            out.push((sym_idx, true, names[0].clone()));
                        }
                }
            }
            // `x == nil` → x is non-nil (strip nil) when condition is false
            Expression::BinaryExpression(bin) if bin.kind() == Operator::Equals => {
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1
                            && let Some(sym_idx) = ir.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                                out.push((sym_idx, false, names[0].clone()));
                            }
                    }
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    Self::extract_nil_guard_symbols(&inner, out, ir, scope_idx);
                }
            }
            _ => {}
        }
    }

    /// Check whether every path through `block` either assigns the named variable
    /// or exits (return/break/error). Used to verify that a nil-guard's then-block
    /// eliminates the nil case before applying post-merge StripNil.
    pub(super) fn block_ensures_assigned_or_exits(block: &Block<'_>, var_name: &str) -> bool {
        let stmts = block.statements();
        // Check if any top-level statement assigns the variable directly
        for stmt in &stmts {
            if Self::stmt_directly_assigns_var(stmt, var_name) {
                return true;
            }
        }
        // If not assigned at top level, check if the block always exits
        if Self::block_always_exits(block) {
            return true;
        }
        // Check last statement: if it's an if/else chain where all branches
        // ensure assigned-or-exit, the block is covered.
        if let Some(Statement::If(if_chain)) = stmts.last() {
            let branches = if_chain.if_branches();
            if let Some(else_branch) = if_chain.else_branch() {
                let all_if_ok = branches.iter().all(|b| {
                    b.block().is_some_and(|bl| Self::block_ensures_assigned_or_exits(&bl, var_name))
                });
                let else_ok = else_branch.block().is_some_and(|bl| {
                    Self::block_ensures_assigned_or_exits(&bl, var_name)
                });
                if all_if_ok && else_ok {
                    return true;
                }
            }
        }
        false
    }

    /// Check if a statement directly assigns to a variable by name.
    fn stmt_directly_assigns_var(stmt: &Statement<'_>, var_name: &str) -> bool {
        if let Statement::Assign(assign) = stmt
            && let Some(var_list) = assign.variable_list() {
                for ident in var_list.identifiers() {
                    let names = ident.names();
                    if names.len() == 1 && names[0] == var_name {
                        return true;
                    }
                }
            }
        false
    }

    /// Extract the field chain from a negated nil-guard condition.
    /// Returns the names for `not self.field` or `self.field == nil`, empty vec otherwise.
    fn extract_nil_guard_field(&self, cond: &Expression<'_>) -> Vec<String> {
        match cond {
            // `not self.field`
            Expression::UnaryExpression(unary) => {
                if !matches!(unary.kind(), Operator::Not) { return vec![]; }
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names_with_brackets();
                    if names.len() >= 2 && !ident.has_any_dynamic_bracket() {
                        return names;
                    }
                }
                vec![]
            }
            // `self.field == nil`
            Expression::BinaryExpression(bin) => {
                if !matches!(bin.kind(), Operator::Equals) { return vec![]; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names_with_brackets();
                        if names.len() >= 2 && !ident.has_any_dynamic_bracket() {
                            return names;
                        }
                    }
                }
                vec![]
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    return self.extract_nil_guard_field(&inner);
                }
                vec![]
            }
            _ => vec![],
        }
    }

    /// Check if a block contains an assignment to the given field chain.
    /// Only checks top-level statements (not nested blocks).
    fn block_assigns_field(block: &Block<'_>, target_names: &[String]) -> bool {
        for stmt in block.statements() {
            if let Statement::Assign(assign) = &stmt
                && let Some(var_list) = assign.variable_list() {
                    for ident in var_list.identifiers() {
                        if ident.names_with_brackets() == target_names && !ident.has_any_dynamic_bracket() {
                            return true;
                        }
                    }
                }
        }
        false
    }

    /// Mark a symbol as narrowed (non-nil) in the given scope, and create a new
    /// symbol version with nil stripped so type-mismatch checks see the narrowed type.
    fn narrow_symbol_strip_nil(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.push_strip_nil_version(sym_idx, scope_idx);
        self.narrow_siblings(sym_idx, scope_idx);
        self.narrow_correlated_locals(sym_idx, scope_idx, false);
        self.narrow_or_coalesce_derived(sym_idx, scope_idx, false);
    }

    /// Like narrow_symbol_strip_nil but also strips false (truthiness narrowing).
    fn narrow_symbol_strip_falsy(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.falsy_narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.push_strip_falsy_version(sym_idx, scope_idx);
        self.narrow_siblings(sym_idx, scope_idx);
        self.narrow_correlated_locals(sym_idx, scope_idx, true);
        self.narrow_or_coalesce_derived(sym_idx, scope_idx, true);
    }

    /// Narrow the expression passed to `assert()`. Decomposes `and` chains so that
    /// `assert(a and b and c)` narrows all three identifiers.
    pub(super) fn narrow_assert_expr(&mut self, expr: &Expression<'_>, scope_idx: ScopeIndex) {
        self.try_flavor_narrow(expr, scope_idx, scope_idx, true);
        match expr {
            Expression::Identifier(ident) => {
                let names = ident.names_with_brackets();
                if names.len() == 1 {
                    if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                        self.narrow_symbol_strip_falsy(sym_idx, scope_idx);
                    }
                } else if !ident.has_any_dynamic_bracket() {
                    self.try_narrow_field_falsy(&names, scope_idx);
                }
            }
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                if matches!(op, Operator::And | Operator::None) {
                    for term in &bin.get_terms() {
                        self.narrow_assert_expr(term, scope_idx);
                    }
                    return;
                }
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // assert(x ~= nil) — strip nil
                    if is_neq {
                        let ident_expr = if Self::is_nil_literal(rhs) {
                            Some(lhs)
                        } else if Self::is_nil_literal(lhs) {
                            Some(rhs)
                        } else {
                            None
                        };
                        if let Some(Expression::Identifier(ident)) = ident_expr {
                            let names = ident.names_with_brackets();
                            if names.len() == 1 {
                                if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                                    self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
                                    self.narrow_siblings(sym_idx, scope_idx);
                                    self.narrow_correlated_locals(sym_idx, scope_idx, false);
                                    self.narrow_or_coalesce_derived(sym_idx, scope_idx, false);
                                }
                            } else if !ident.has_any_dynamic_bracket() {
                                self.try_narrow_field(&names, scope_idx);
                            }
                        }
                    }
                    // assert(type(x) == "string") — type guard (positive for ==, inverse for ~=)
                    let guard_sym = self.extract_type_guard_symbol(lhs, rhs, scope_idx)
                        .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, scope_idx));
                    if let Some(sym_idx) = guard_sym
                        && let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                            if type_name == "nil" {
                                // assert(type(x) ~= "nil") → x is NOT nil
                                if is_neq {
                                    self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
                                    self.narrow_siblings(sym_idx, scope_idx);
                                    self.narrow_or_coalesce_derived(sym_idx, scope_idx, false);
                                }
                                // assert(type(x) == "nil") → x IS nil (no useful narrowing in assert)
                            } else if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                if is_eq {
                                    self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
                                    self.narrow_siblings(sym_idx, scope_idx);
                                    self.type_filtered_symbols.entry(scope_idx).or_default()
                                        .insert(sym_idx, vt);
                                    self.narrow_or_coalesce_derived(sym_idx, scope_idx, false);
                                } else {
                                    self.add_type_stripped(scope_idx, sym_idx, vt.clone());
                                    self.push_strip_type_version(sym_idx, vt, scope_idx, false);
                                }
                            }
                        }
                    // assert(type(obj.field) == "table") — field type guard
                    if guard_sym.is_none()
                        && let Some((sym_idx, chain)) = self.extract_type_guard_field(lhs, rhs, scope_idx)
                            && let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if type_name == "nil" {
                                    // assert(type(obj.f) ~= "nil") → strip nil
                                    if is_neq {
                                        self.narrowed_fields.entry(scope_idx).or_default()
                                            .insert((sym_idx, chain));
                                    }
                                } else if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    if is_eq {
                                        self.narrowed_fields.entry(scope_idx).or_default()
                                            .insert((sym_idx, chain.clone()));
                                        self.type_narrowed_fields.entry(scope_idx).or_default()
                                            .insert((sym_idx, chain), vt);
                                    } else {
                                        // assert(type(obj.field) ~= "table") — strip that type
                                        self.type_stripped_fields.entry(scope_idx).or_default()
                                            .insert((sym_idx, chain), vt);
                                    }
                                }
                            }
                }
            }
            Expression::FunctionCall(call) => {
                // assert(obj:IsCat()) — type-narrows guard inside assert
                if let Some((sym_idx, class_name)) = self.extract_type_narrows_guard(call, scope_idx) {
                    self.apply_type_narrows(sym_idx, &class_name, scope_idx);
                } else if let Some((sym_idx, true_type, _)) = self.extract_bool_discriminator(call, scope_idx) {
                    // assert(x:IsSubRow()) — literal-bool union discrimination
                    self.type_narrowed_symbols.entry(scope_idx).or_default()
                        .insert(sym_idx, true_type);
                } else if let Some((sym_idx, chain, true_type, _)) = self.extract_bool_discriminator_field(call, scope_idx) {
                    self.type_narrowed_fields.entry(scope_idx).or_default()
                        .insert((sym_idx, chain), true_type);
                }
            }
            Expression::GroupedExpression(group) => {
                if let Some(inner) = group.get_expression() {
                    self.narrow_assert_expr(&inner, scope_idx);
                }
            }
            _ => {}
        }
    }

    /// Synthesize correlated return-only overloads for a function whose body has
    /// just finished walking. Triggered when:
    ///   * `inference.correlated_return_overloads` is enabled
    ///   * the function has no `@return` / return-only overload annotations
    ///   * its return statements yield ≥ 2 distinct per-position signatures
    ///     (matching arity ≥ 2)
    ///
    /// Emits one return-only `ResolvedOverload` per unique tuple with literal-derived
    /// per-position types (string/number/boolean literals normalize to their generic types;
    /// non-literal expressions default to `Any`; nil literals stay `Nil`). Duplicate
    /// overloads (same `returns` vector) are collapsed.
    ///
    /// The old "every nil-containing tuple must be all-nil" footgun guard and the
    /// "≥ 1 all-nil tuple" requirement are intentionally dropped: mixed tuples
    /// (e.g. `return true, nil, nil` alongside `return true, AST_VARIANT, 3`) are a
    /// common real-world pattern, and the overload filter correctly keeps the
    /// "no-nil-here" positions non-nil across every emitted case.
    ///
    /// These overloads serve two purposes downstream:
    ///   1. Sibling narrowing: `narrow_siblings` picks them up via `is_return_only` and
    ///      creates `OverloadNarrow` versions for the call's other return values.
    ///      Positions that are non-nil in every synthesized case simply stay non-nil
    ///      (they don't drive narrowing but don't break it either).
    ///   2. Base return type fallback: `resolve_function_call` uses their type union at
    ///      each ret position when no `FunctionRet` symbol exists at the function-body
    ///      scope (the typical case when every return is inside a nested if-branch).
    pub(super) fn synthesize_correlated_return_overloads(&mut self, func_id: FunctionIndex) {
        if !self.correlated_return_overloads { return; }
        if func_id.is_external() { return; }
        {
            let func = &self.ir.functions[func_id.val()];
            if !func.return_annotations.is_empty() { return; }
            if func.has_vararg_return { return; }
            if func.explicit_void_return { return; }
            if func.overloads.iter().any(|o| o.is_return_only) { return; }
        }

        // Group ret-symbol versions by (def_node.start, def_node.end). Each group
        // is one return statement; the SymbolIdentifier::FunctionRet's index gives
        // the position within that statement's tuple.
        use std::collections::BTreeMap;
        let rets = self.ir.functions[func_id.val()].rets.clone();
        let mut groups: BTreeMap<(u32, u32), Vec<(usize, ExprId)>> = BTreeMap::new();
        for sym_idx in rets {
            if sym_idx.is_external() { continue; }
            let sym = &self.ir.symbols[sym_idx.val()];
            let SymbolIdentifier::FunctionRet(_, ret_index) = sym.id else { continue };
            for ver in &sym.versions {
                let Some(expr_id) = ver.type_source else { continue };
                let key = (ver.def_node.start, ver.def_node.end);
                groups.entry(key).or_default().push((ret_index, expr_id));
            }
        }
        let implicit_nil = self.ir.functions[func_id.val()].implicit_nil_return;
        // A bare `return` / fall-through counts as one additional "signature"
        // (an implicit all-nil tuple) at caller side, so it can contribute to
        // the ≥ 2 group minimum even when there's only a single explicit return.
        if groups.len() + if implicit_nil { 1 } else { 0 } < 2 { return; }

        // Validate matching arity ≥ 2 and compute per-position synthesized types.
        // `tuples` carries both the coarse build-time type (what enters dedup) and
        // the source ExprId for non-literal positions (candidates for resolve-time
        // refinement). Literal positions have `None` — their type is final.
        let mut arity: Option<usize> = None;
        let mut tuples: Vec<Vec<(ValueType, Option<ExprId>)>> = Vec::new();
        for (_, mut entries) in groups {
            entries.sort_by_key(|(idx, _)| *idx);
            // Positions must be contiguous 0..len (no gaps).
            for (i, (idx, _)) in entries.iter().enumerate() {
                if *idx != i { return; }
            }
            match arity {
                None => arity = Some(entries.len()),
                Some(a) if a == entries.len() => {}
                _ => return,
            }
            let returns: Vec<(ValueType, Option<ExprId>)> = entries.iter().map(|(_, expr_id)| {
                Self::synthesized_return_type(self.ir.expr(*expr_id), *expr_id)
            }).collect();
            tuples.push(returns);
        }
        let arity = arity.unwrap_or(0);
        if arity < 2 { return; }

        // Bare `return` / fall-through at the end of the body is observationally
        // identical to `return nil, nil, ..., nil` from the caller's side. Fold
        // that into the signature set so patterns like
        //   if cond then return items, groups, n end
        //   return  -- bare early-out
        // correlate cleanly under sibling narrowing.
        if implicit_nil {
            tuples.push(vec![(ValueType::Nil, None); arity]);
        }

        // Dedupe by the full per-position build-time tuple (literal bools distinct,
        // non-literal positions collapse to Any). Merge candidate ExprIds across
        // returns that land in the same bucket so refinement sees every source.
        // Require ≥ 2 distinct signatures — a single signature gives no sibling
        // narrowing benefit over the plain return-type fallback.
        struct Emitted {
            returns: Vec<ValueType>,
            candidates: Vec<Vec<ExprId>>,
        }
        let mut emitted: Vec<Emitted> = Vec::new();
        for returns in tuples {
            let shape: Vec<ValueType> = returns.iter().map(|(v, _)| v.clone()).collect();
            if let Some(slot) = emitted.iter_mut().find(|e| e.returns == shape) {
                for (pos, (_, expr_id)) in returns.iter().enumerate() {
                    if let Some(eid) = expr_id
                        && !slot.candidates[pos].contains(eid) {
                            slot.candidates[pos].push(*eid);
                        }
                }
                continue;
            }
            let candidates: Vec<Vec<ExprId>> = returns.iter()
                .map(|(_, e)| e.iter().copied().collect())
                .collect();
            emitted.push(Emitted { returns: shape, candidates });
        }
        if emitted.len() < 2 { return; }

        for Emitted { returns, candidates } in emitted {
            let overload_idx = self.ir.functions[func_id.val()].overloads.len();
            self.ir.functions[func_id.val()].overloads.push(ResolvedOverload {
                params: Vec::new(),
                returns,
                is_return_only: true,
                description: None,
                has_vararg_tail: false,
                is_vararg: false,
            });
            // Queue non-literal positions for refinement at resolve time.
            for (pos, cands) in candidates.into_iter().enumerate() {
                if cands.is_empty() { continue; }
                self.synth_return_overload_refinements.push(
                    crate::analysis::SynthOverloadRefinement {
                        function_idx: func_id,
                        overload_idx,
                        ret_pos: pos,
                        candidates: cands,
                        resolved: Vec::new(),
                    },
                );
            }
        }
    }

    /// Map a return expression to a build-time ValueType for synthesized overload
    /// positions, plus an optional ExprId carried when the type is a placeholder.
    /// Literal booleans are preserved as `Boolean(Some(b))` so that the synthesized
    /// overloads can discriminate `true` vs `false` cases under sibling narrowing
    /// (same machinery as hand-written `@return true` / `@return false`).
    /// String and number literals still normalize to their generic types to avoid
    /// ugly literal unions across branches.
    /// Non-literal expressions land on `Any` with the source `ExprId`; resolve-time
    /// refinement replaces the placeholder with the resolved type.
    fn synthesized_return_type(expr: &Expr, expr_id: ExprId) -> (ValueType, Option<ExprId>) {
        match expr {
            Expr::Literal(ValueType::Nil) => (ValueType::Nil, None),
            Expr::Literal(ValueType::String(_)) => (ValueType::String(None), None),
            Expr::Literal(ValueType::Number) => (ValueType::Number, None),
            Expr::Literal(ValueType::Boolean(Some(b))) => (ValueType::Boolean(Some(*b)), None),
            Expr::Literal(ValueType::Boolean(None)) => (ValueType::Boolean(None), None),
            _ => (ValueType::Any, Some(expr_id)),
        }
    }

    /// Narrow multi-return siblings when a symbol from a return-only overload group is narrowed.
    /// Uses OverloadNarrow expressions to filter return-only overloads and compute precise
    /// union types for each sibling, propagating narrowing to ALL return siblings.
    pub(super) fn narrow_siblings(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        let Some(siblings) = self.multi_return_siblings.get(&sym_idx).cloned() else { return };
        // Check that the function has return-only overloads by tracing from any sibling's
        // type_source (a FunctionCall expr) → func expr → symbol → FunctionDef → overloads
        let func_expr = match self.check_return_only_overloads_from_siblings(&siblings) {
            OverloadCheck::HasOverloads(fe) => fe,
            OverloadCheck::NoOverloads => return,
            OverloadCheck::Deferred(func_expr) => {
                // Can't resolve at build time (cross-file FieldAccess) — defer to resolve phase
                let narrowed_info = self.collect_narrowed_sibling_info(&siblings, scope_idx);
                self.deferred_sibling_narrowings.push(DeferredSiblingNarrowing {
                    func_expr, siblings, scope: scope_idx, narrowed: narrowed_info,
                });
                return;
            }
        };
        // Collect ALL narrowed siblings in this scope (including sym_idx which was just narrowed)
        let narrowed_info = self.collect_narrowed_sibling_info(&siblings, scope_idx);
        if narrowed_info.is_empty() { return; }
        // Create OverloadNarrow versions for all non-guarded siblings.
        // Do NOT add siblings to narrowed_symbols — the OverloadNarrow expression
        // already computes the correct type (which may still include nil).
        // Adding to narrowed_symbols would cause narrow_type_for_display to strip
        // nil again, producing incorrect results.
        for &(ret_index, sibling_idx) in &siblings {
            if sibling_idx == sym_idx { continue; }
            self.ir.push_overload_narrow_version(
                sibling_idx, scope_idx, func_expr, ret_index, narrowed_info.clone(),
            );
        }
    }

    /// Collect (ret_index, NarrowKind) for every sibling in `siblings` that has a
    /// narrowing recorded in `scope_idx`. The just-narrowed trigger is included
    /// naturally because the caller inserts it into a tracking map before invoking
    /// `narrow_siblings`, and `narrow_kind_for` reads from all tracking maps.
    /// The `OverloadNarrow` filter uses this to pick overloads compatible with every guard.
    pub(crate) fn collect_narrowed_sibling_info(&self, siblings: &[(usize, SymbolIndex)], scope_idx: ScopeIndex) -> Vec<(usize, NarrowKind)> {
        let mut info = Vec::new();
        for &(ret_index, sibling_idx) in siblings {
            if let Some(k) = self.narrow_kind_for(sibling_idx, scope_idx) {
                info.push((ret_index, k));
            }
        }
        info
    }

    /// Detect `x == EXPR` (or `EXPR == x`) where `x` is a bare single-name symbol
    /// and `EXPR` is an identifier chain (dot access) whose eventual type may be a
    /// class. Lowers `EXPR` and queues a deferred class-equality narrowing that
    /// resolve picks up once `EXPR`'s type is known.
    ///
    /// Restricted to pure identifier chains (no function calls) so re-lowering
    /// doesn't create a second binding for embedded name references that would
    /// overwrite the original `symbol_version_at` entries.
    fn record_class_eq_deferral(
        &mut self,
        lhs: &Expression<'_>,
        rhs: &Expression<'_>,
        parent_scope: ScopeIndex,
        target_scope: ScopeIndex,
    ) {
        let (sym_side, other_side) = match (
            Self::as_bare_single_name(lhs, self, parent_scope),
            Self::as_bare_single_name(rhs, self, parent_scope),
        ) {
            (Some(sym), _) if Self::is_pure_identifier_chain(rhs) => (sym, rhs),
            (None, Some(sym)) if Self::is_pure_identifier_chain(lhs) => (sym, lhs),
            _ => return,
        };
        let expr_id = self.lower_expression(other_side, parent_scope);
        self.deferred_class_eq_narrowings.push((sym_side, expr_id, target_scope));
    }

    /// Return the SymbolIndex if `expr` is a single-name identifier resolving in `scope`.
    fn as_bare_single_name(expr: &Expression<'_>, analysis: &Analysis<'a>, scope: ScopeIndex) -> Option<SymbolIndex> {
        if let Expression::Identifier(ident) = expr {
            let names = ident.names();
            if names.len() == 1 {
                return analysis.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope);
            }
        }
        None
    }

    /// True iff `expr` is a pure identifier chain — either a bare `NameRef` or a
    /// `DotAccess` path like `Foo.Bar.Baz`. The parser normalizes both into
    /// `Expression::Identifier` (see *Expression lowering — split identifier nodes*
    /// in CLAUDE.md); `BracketAccess`, `MethodCall`, and `FunctionCall` are separate
    /// variants and are correctly rejected here.
    ///
    /// Used by `record_class_eq_deferral` to guard against re-lowering a subexpression
    /// that contains references to the narrowed sibling (e.g. `strlower(name)` where
    /// `name` was already narrowed by an enclosing `and` chain), which would clobber
    /// the original `symbol_version_at` binding.
    fn is_pure_identifier_chain(expr: &Expression<'_>) -> bool {
        matches!(expr, Expression::Identifier(_))
    }

    /// Resolve the narrowing kind (if any) for a symbol in a given scope.
    /// Checks class_narrowed_symbols first (most specific), then truthy/falsy/nil narrowing.
    pub(crate) fn narrow_kind_for(&self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) -> Option<NarrowKind> {
        if let Some(class_name) = self.class_narrowed_symbols.get(&scope_idx)
            .and_then(|m| m.get(&sym_idx))
        {
            return Some(NarrowKind::ClassEq(class_name.clone()));
        }
        if self.truthy_narrowed_symbols.get(&scope_idx).is_some_and(|s| s.contains(&sym_idx)) {
            return Some(NarrowKind::StripTruthy);
        }
        if self.narrowed_symbols.get(&scope_idx).is_some_and(|s| s.contains(&sym_idx)) {
            if self.falsy_narrowed_symbols.get(&scope_idx).is_some_and(|s| s.contains(&sym_idx)) {
                return Some(NarrowKind::StripFalsy);
            }
            return Some(NarrowKind::StripNil);
        }
        None
    }

    /// When a local variable from a correlated-local group is narrowed (nil stripped),
    /// also narrow all sibling locals in the same group. This handles the pattern where
    /// multiple locals are always assigned together in every branch of an if/elseif chain
    /// (without else), so guarding one implies all are non-nil.
    fn narrow_correlated_locals(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex, falsy: bool) {
        // Find all groups containing sym_idx and collect sibling indices.
        let mut siblings: Vec<SymbolIndex> = Vec::new();
        for group in &self.correlated_locals {
            if group.contains(&sym_idx) {
                for &sibling in group {
                    if sibling != sym_idx && !siblings.contains(&sibling) {
                        siblings.push(sibling);
                    }
                }
            }
        }
        for sibling in siblings {
            self.narrowed_symbols.entry(scope_idx).or_default().insert(sibling);
            if falsy {
                self.falsy_narrowed_symbols.entry(scope_idx).or_default().insert(sibling);
                self.push_strip_falsy_version(sibling, scope_idx);
            } else {
                self.push_strip_nil_version(sibling, scope_idx);
            }
            // A correlated sibling is itself a valid narrowing source for any
            // `x = x or sibling` coalesce derivations.
            self.narrow_or_coalesce_derived(sibling, scope_idx, falsy);
        }
    }

    /// Collect symbols derived from `source` via `x = x or source` assignments.
    /// Each derived symbol is non-nil whenever `source` is known non-nil.
    pub(super) fn or_coalesce_derived(&self, source: SymbolIndex) -> Vec<SymbolIndex> {
        self.or_coalesce_derivations.get(&source).cloned().unwrap_or_default()
    }

    /// When `source` is narrowed (non-nil), also narrow all symbols derived from
    /// it via `x = x or source`. See `or_coalesce_derivations` for the pattern.
    fn narrow_or_coalesce_derived(&mut self, source: SymbolIndex, scope_idx: ScopeIndex, falsy: bool) {
        for derived in self.or_coalesce_derived(source) {
            if self.narrowed_symbols.get(&scope_idx).is_some_and(|s| s.contains(&derived)) {
                // Already narrowed in this scope; skip to avoid redundant versions.
                continue;
            }
            self.narrowed_symbols.entry(scope_idx).or_default().insert(derived);
            if falsy {
                self.falsy_narrowed_symbols.entry(scope_idx).or_default().insert(derived);
                self.push_strip_falsy_version(derived, scope_idx);
            } else {
                self.push_strip_nil_version(derived, scope_idx);
            }
        }
    }

    /// Detect two `or`-coalesce patterns and register narrowing derivations.
    ///
    /// Pattern 1 (`x = x or y`): narrowing `y` narrows `x`. Only fires on reassignments,
    /// not local declarations — the LHS of `or` must already refer to the symbol being
    /// assigned, which can only happen when re-assigning an existing binding.
    ///
    /// Pattern 2 (`y = (x and _) or nil`): narrowing `y` non-nil narrows `x`. The
    /// trailing `or nil` forces `y` to be nil whenever `x` is falsy, so a non-nil `y`
    /// guarantees `x` was truthy. Fires on both local decls and reassignments.
    ///
    /// Sources differ between the patterns: pattern 1's source is the bare RHS
    /// identifier (`y`) and the assignment LHS is the derived; pattern 2's source
    /// is the assignment LHS (`y`) and the derived is the LHS of the inner `and`
    /// (`x`). The map key is always the source.
    ///
    /// Invalidation: any assignment to `x_sym` clears prior derivations where it
    /// appeared as either source or derived. The new RHS then re-registers whatever
    /// pattern still holds.
    pub(super) fn maybe_register_or_coalesce(
        &mut self,
        x_sym: SymbolIndex,
        x_name: &str,
        expression: Option<&Expression<'_>>,
        scope_idx: ScopeIndex,
        is_local_decl: bool,
    ) {
        // Pattern 1: x_sym is the derived. Source is the bare RHS identifier.
        // Skipped for local decls: the LHS of `or` would resolve to the freshly-
        // inserted inner symbol rather than the outer shadowed one the programmer
        // actually wrote, and the existing test suite assumes local decls don't
        // register this pattern.
        let pattern1_source: Option<SymbolIndex> = if is_local_decl {
            None
        } else {
            (|| -> Option<SymbolIndex> {
                let expr = expression?;
                let bin = match expr {
                    Expression::BinaryExpression(b) => b,
                    _ => return None,
                };
                if !matches!(bin.kind(), Operator::Or) { return None; }
                let terms = bin.get_terms();
                if terms.len() != 2 { return None; }
                let lhs_ident = match &terms[0] {
                    Expression::Identifier(id) => id,
                    _ => return None,
                };
                let lhs_names = lhs_ident.names();
                if lhs_names.len() != 1 || lhs_names[0] != x_name { return None; }
                let lhs_sym = self.get_symbol(&SymbolIdentifier::Name(lhs_names[0].clone()), scope_idx)?;
                if lhs_sym != x_sym { return None; }
                let rhs_ident = match &terms[1] {
                    Expression::Identifier(id) => id,
                    _ => return None,
                };
                let rhs_names = rhs_ident.names();
                if rhs_names.len() != 1 { return None; }
                if rhs_names[0] == x_name { return None; }
                self.get_symbol(&SymbolIdentifier::Name(rhs_names[0].clone()), scope_idx)
            })()
        };

        // Pattern 2: x_sym is the source. Derived is the LHS of the inner `and`.
        let pattern2_derived: Option<SymbolIndex> = (|| -> Option<SymbolIndex> {
            let expr = expression?;
            let or_bin = match expr {
                Expression::BinaryExpression(b) => b,
                _ => return None,
            };
            if !matches!(or_bin.kind(), Operator::Or) { return None; }
            let or_terms = or_bin.get_terms();
            if or_terms.len() != 2 { return None; }
            if !Self::is_nil_literal(&or_terms[1]) { return None; }
            let and_bin = match &or_terms[0] {
                Expression::BinaryExpression(b) => b,
                _ => return None,
            };
            if !matches!(and_bin.kind(), Operator::And) { return None; }
            let and_terms = and_bin.get_terms();
            if and_terms.len() != 2 { return None; }
            let lhs_ident = match &and_terms[0] {
                Expression::Identifier(id) => id,
                _ => return None,
            };
            let lhs_names = lhs_ident.names();
            if lhs_names.len() != 1 { return None; }
            if lhs_names[0] == x_name { return None; }
            let derived = self.get_symbol(&SymbolIdentifier::Name(lhs_names[0].clone()), scope_idx)?;
            if derived == x_sym { return None; }
            Some(derived)
        })();

        // Invalidate entries involving x_sym (as derived or as source) before
        // registering the new relationship.
        for derived_list in self.or_coalesce_derivations.values_mut() {
            derived_list.retain(|&d| d != x_sym);
        }
        self.or_coalesce_derivations.remove(&x_sym);
        self.or_coalesce_derivations.retain(|_, v| !v.is_empty());

        if let Some(y_sym) = pattern1_source {
            self.or_coalesce_derivations.entry(y_sym).or_default().push(x_sym);
        }
        if let Some(derived) = pattern2_derived {
            self.or_coalesce_derivations.entry(x_sym).or_default().push(derived);
        }
    }

    /// Check if the function called in a multi-return group has return-only overloads.
    /// Returns the func_expr ExprId for deferred resolution when the callee is a
    /// FieldAccess that can't be resolved at build time (cross-file case).
    pub(crate) fn check_return_only_overloads_from_siblings(&self, siblings: &[(usize, SymbolIndex)]) -> OverloadCheck {
        // Get any sibling's type_source to find the FunctionCall expression
        let (_, first_sym) = siblings[0];
        // Find the version with a FunctionCall type_source (the original multi-return assignment).
        // Search in REVERSE because the multi-return assignment is the most recent version,
        // and an earlier version might be a FunctionCall to a different function (e.g. a prior
        // reassignment like `a = max(...)` before `a, b = getData()`). StripNil/StripFalsy
        // versions added by narrowing don't have FunctionCall type_sources, so they're skipped.
        let func_expr = self.ir.symbols[first_sym.val()].versions.iter().rev()
            .find_map(|v| {
                let ts = v.type_source?;
                match self.ir.expr(ts) {
                    Expr::FunctionCall { func, .. } => Some(*func),
                    _ => None,
                }
            });
        let Some(func_expr) = func_expr else { return OverloadCheck::NoOverloads };
        // Resolve func expr → symbol → FunctionDef → overloads
        let func_idx = match self.ir.expr(func_expr) {
            Expr::SymbolRef(sym_idx, _) => {
                let sym_idx = *sym_idx;
                // Look through the symbol's type_source to find FunctionDef,
                // or fall back to resolved_type for external symbols (which store
                // Function(func_idx) directly without a type_source).
                self.ir.sym(sym_idx).versions.iter().find_map(|v| {
                    if let Some(ts) = v.type_source && let Expr::FunctionDef(idx) = self.ir.expr(ts) {
                        return Some(*idx);
                    }
                    // External symbols have resolved_type set directly
                    match &v.resolved_type {
                        Some(ValueType::Function(Some(idx))) => Some(*idx),
                        _ => None,
                    }
                })
            }
            Expr::FieldAccess { table, field, .. } => {
                let table = *table;
                let field = field.clone();
                // Try to resolve the table to a TableIndex, then look up the field.
                // Only defer if the table itself can't be resolved (cross-file).
                // If the table resolves but the field doesn't exist or isn't a
                // FunctionDef, that's a definitive NoOverloads.
                match self.resolve_expr_to_table(table) {
                    Some(ti) => {
                        self.get_field(ti, &field).and_then(|fi| {
                            match self.ir.expr(fi.expr) {
                                Expr::FunctionDef(idx) => Some(*idx),
                                _ => None,
                            }
                        })
                    }
                    None => return OverloadCheck::Deferred(func_expr),
                }
            }
            _ => None,
        };
        let Some(func_idx) = func_idx else { return OverloadCheck::NoOverloads };
        if self.ir.func(func_idx).overloads.iter().any(|o| o.is_return_only) {
            OverloadCheck::HasOverloads(func_expr)
        } else {
            OverloadCheck::NoOverloads
        }
    }

    /// Try to narrow a field access from an identifier with 2+ names (e.g. `self.field`
    /// or `self.field.subField`). Marks the (root_symbol, field_chain) as narrowed in the given scope.
    pub(super) fn try_narrow_field(&mut self, names: &[String], scope_idx: ScopeIndex) {
        if names.len() >= 2
            && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                let chain = names[1..].to_vec();
                self.narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, chain.clone()));
                self.narrow_correlated_fields(sym_idx, &names[0], &chain, scope_idx, false);
            }
    }

    /// Like `try_narrow_field` but also marks the field chain as falsy-narrowed
    /// (strips both nil and false). Used for assert() and bare truthiness guards.
    fn try_narrow_field_falsy(&mut self, names: &[String], scope_idx: ScopeIndex) {
        if names.len() >= 2
            && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                let chain = names[1..].to_vec();
                self.narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, chain.clone()));
                self.falsy_narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, chain.clone()));
                self.narrow_correlated_fields(sym_idx, &names[0], &chain, scope_idx, true);
            }
    }

    /// When a field in a `@correlated` group is narrowed, also narrow all sibling fields
    /// in the same group.
    fn narrow_correlated_fields(
        &mut self,
        sym_idx: SymbolIndex,
        root_name: &str,
        chain: &[String],
        scope_idx: ScopeIndex,
        falsy: bool,
    ) {
        if chain.is_empty() { return; }
        let narrowed_field = &chain[chain.len() - 1];
        // Resolve the intermediate chain to find the table containing the narrowed field.
        // For `self._auction.itemString`, intermediate is ["_auction"] → resolve to Auction table.
        // For `self.field`, intermediate is [] → resolve self's table directly.
        let table_idx = if chain.len() == 1 {
            self.ir.find_table_for_symbol(root_name, scope_idx)
        } else {
            self.resolve_field_chain_table(root_name, &chain[..chain.len() - 1], scope_idx)
        };
        let Some(table_idx) = table_idx else { return };
        let groups = self.ir.table(table_idx).correlated_groups.clone();
        if groups.is_empty() { return; }
        for group in &groups {
            if !group.iter().any(|f| f == narrowed_field) { continue; }
            for sibling in group {
                if sibling == narrowed_field { continue; }
                let mut sibling_chain = chain[..chain.len() - 1].to_vec();
                sibling_chain.push(sibling.clone());
                self.narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, sibling_chain.clone()));
                if falsy {
                    self.falsy_narrowed_fields.entry(scope_idx).or_default()
                        .insert((sym_idx, sibling_chain));
                }
            }
        }
    }

    /// Resolve a field chain (excluding the final field) to find its TableIndex.
    /// E.g. for root_name="self", fields=["_auction"], resolves self → Foo table → _auction field → Auction table.
    fn resolve_field_chain_table(&self, root_name: &str, fields: &[String], scope_idx: ScopeIndex) -> Option<TableIndex> {
        let mut table_idx = self.ir.find_table_for_symbol(root_name, scope_idx)?;
        for field_name in fields {
            let fi = self.ir.get_field(table_idx, field_name)?;
            let vt = fi.annotation.as_ref()?;
            // Strip nil since the field may be optional (e.g. `Auction?` → `Auction`)
            table_idx = match vt.strip_nil() {
                ValueType::Table(Some(idx)) => idx,
                // Also handle Union where stripping nil leaves a single table
                ValueType::Union(ref types) => {
                    let tables: Vec<_> = types.iter().filter_map(|t| match t {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }).collect();
                    if tables.len() == 1 { tables[0] } else { return None; }
                }
                _ => return None,
            };
        }
        Some(table_idx)
    }

    /// Create a new symbol version with nil stripped (without updating narrowed_symbols).
    /// Used for short-circuit `and` narrowing where the version should be temporary.
    pub(super) fn push_strip_nil_version(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        self.ir.push_strip_nil_version(sym_idx, scope_idx);
    }

    /// Create a new symbol version with nil and false stripped (truthiness narrowing).
    pub(super) fn push_strip_falsy_version(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        if !sym_idx.is_external() {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let stripped = self.ir.push_expr(Expr::StripFalsy(prev_ref));
            let node = self.ir.symbols[sym_idx.val()].versions[prev_ver].def_node;
            let order = self.ir.next_order();
            self.ir.symbols[sym_idx.val()].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(stripped),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
                creation_order: order,
                original_type_source: None,
            });
        }
    }

    /// Create a new symbol version with a specific type stripped from the union.
    /// Used for inverse type() guard narrowing (else-branch of `if type(x) == "t"`).
    /// When `ancestors_only` is true, uses ancestors-only scope lookup to avoid
    /// picking up versions from descendant scopes (e.g. then-branch versions
    /// that would corrupt the result in early-exit narrowing).
    fn push_strip_type_version(&mut self, sym_idx: SymbolIndex, strip_type: ValueType, scope_idx: ScopeIndex, ancestors_only: bool) {
        if !sym_idx.is_external() {
            let prev_ver = if ancestors_only {
                self.ir.version_for_scope_ancestors_only(sym_idx, scope_idx)
            } else {
                self.ir.version_for_scope(sym_idx, scope_idx)
            };
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let stripped = self.ir.push_expr(Expr::CastRemove(prev_ref, strip_type));
            let node = self.ir.symbols[sym_idx.val()].versions[prev_ver].def_node;
            let order = self.ir.next_order();
            self.ir.symbols[sym_idx.val()].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(stripped),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
                creation_order: order,
                original_type_source: None,
            });
        }
    }

    /// Create a new symbol version narrowed to a specific type.
    /// Used for type() guard narrowing in short-circuit `and` expressions.
    pub(super) fn push_type_narrowed_version(&mut self, sym_idx: SymbolIndex, narrowed_type: ValueType, scope_idx: ScopeIndex) {
        if !sym_idx.is_external() {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let node = self.ir.symbols[sym_idx.val()].versions[prev_ver].def_node;
            let order = self.ir.next_order();
            self.ir.symbols[sym_idx.val()].versions.push(SymbolVersion {
                def_node: node,
                type_source: None,
                resolved_type: Some(narrowed_type),
                type_args: Vec::new(),
                created_in_scope: scope_idx,
                creation_order: order,
                original_type_source: None,
            });
        }
    }

    /// Push a version that filters the previous type to keep only types matching a
    /// type guard. Unlike `push_type_narrowed_version` (which sets a fixed type),
    /// this preserves specific types like `string[]` when narrowing with `type() == "table"`.
    /// When `ancestors_only` is true, uses ancestors-only scope lookup to avoid
    /// picking up versions from descendant scopes (e.g. then-branch versions
    /// that would corrupt the result in early-exit narrowing).
    pub(crate) fn push_type_filter_version(&mut self, sym_idx: SymbolIndex, guard_type: ValueType, scope_idx: ScopeIndex, ancestors_only: bool) {
        if !sym_idx.is_external() {
            let prev_ver = if ancestors_only {
                self.ir.version_for_scope_ancestors_only(sym_idx, scope_idx)
            } else {
                self.ir.version_for_scope(sym_idx, scope_idx)
            };
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let filtered = self.ir.push_expr(Expr::TypeFilter(prev_ref, guard_type));
            let node = self.ir.symbols[sym_idx.val()].versions[prev_ver].def_node;
            let order = self.ir.next_order();
            self.ir.symbols[sym_idx.val()].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(filtered),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
                creation_order: order,
                original_type_source: None,
            });
        }
    }

    /// Add a type to strip for a symbol in a scope, combining with any existing strip.
    fn add_type_stripped(&mut self, scope: ScopeIndex, sym_idx: SymbolIndex, vt: ValueType) {
        let map = self.type_stripped_symbols.entry(scope).or_default();
        if let Some(existing) = map.remove(&sym_idx) {
            map.insert(sym_idx, ValueType::union(existing, vt));
        } else {
            map.insert(sym_idx, vt);
        }
    }

    fn is_nil_literal(expr: &Expression<'_>) -> bool {
        matches!(expr, Expression::Literal(lit) if lit.is_nil())
    }

    /// Check if a block contains a `break` statement at the current loop level.
    /// Recurses into if/else branches but NOT into nested loops (whose breaks
    /// target the inner loop, not the outer one).
    pub(super) fn block_contains_break(block: &Block<'_>) -> bool {
        Self::node_contains_break(&block.syntax())
    }

    fn node_contains_break(node: &SyntaxNode<'_>) -> bool {
        for child in node.children_with_tokens() {
            match &child {
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BreakKeyword => {
                    return true;
                }
                NodeOrToken::Node(n) => {
                    // Skip nested loop nodes — their breaks target the inner loop
                    let kind = n.kind();
                    if kind == SyntaxKind::WhileLoop
                        || kind == SyntaxKind::RepeatUntilLoop
                        || kind == SyntaxKind::ForCountLoop
                        || kind == SyntaxKind::ForInLoop
                    {
                        continue;
                    }
                    if Self::node_contains_break(n) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Collect symbols that should be narrowed after a while loop exits.
    /// Mirrors `analyze_nil_guard` with `is_then_branch=false` (the loop exits
    /// when the condition is false) but only collects (sym_idx, strip_falsy)
    /// pairs without mutating narrowing state.
    pub(super) fn collect_while_exit_narrowings(&self, cond: &Expression<'_>, scope_idx: ScopeIndex) -> Vec<(SymbolIndex, bool)> {
        let mut result = Vec::new();
        self.collect_exit_narrowings_inner(cond, scope_idx, false, &mut result);
        // Dedup: if the same symbol appears multiple times (e.g. referenced in
        // multiple sub-expressions), keep the strongest narrowing (strip_falsy=true
        // wins over strip_falsy=false).
        result.sort_by_key(|(sym, falsy)| (*sym, !*falsy));
        result.dedup_by_key(|(sym, _)| *sym);
        result
    }

    fn collect_exit_narrowings_inner(
        &self,
        cond: &Expression<'_>,
        scope_idx: ScopeIndex,
        is_then_branch: bool,
        result: &mut Vec<(SymbolIndex, bool)>,
    ) {
        match cond {
            // Bare identifier: `while x do` (then) / `while not x do` (flipped to then)
            Expression::Identifier(ident) => {
                if is_then_branch {
                    let names = ident.names();
                    if names.len() == 1
                        && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                            result.push((sym_idx, true)); // truthiness → strip falsy
                        }
                }
            }
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                // `a and b` in then-branch: both true
                if matches!(op, Operator::And | Operator::None) && is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.collect_exit_narrowings_inner(term, scope_idx, true, result);
                        }
                        return;
                    }
                }
                // `a or b` in else-branch: NOT (a OR b) → both false
                if matches!(op, Operator::Or) && !is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.collect_exit_narrowings_inner(term, scope_idx, false, result);
                        }
                        return;
                    }
                }
                // Nil comparison: `x == nil` / `x ~= nil`
                let is_neq = matches!(op, Operator::NotEquals);
                let is_eq = matches!(op, Operator::Equals);
                if !is_neq && !is_eq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        let should_narrow = (is_neq && is_then_branch) || (is_eq && !is_then_branch);
                        if should_narrow && names.len() == 1
                            && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                                result.push((sym_idx, false)); // nil comparison → strip nil only
                            }
                    }
                }
            }
            // `not expr` flips the branch sense
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                if let Some(inner) = u.get_terms().into_iter().next() {
                    self.collect_exit_narrowings_inner(&inner, scope_idx, !is_then_branch, result);
                }
            }
            // Unwrap grouping
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.collect_exit_narrowings_inner(&inner, scope_idx, is_then_branch, result);
                }
            }
            _ => {}
        }
    }

    /// Convert a Lua type name string to a ValueType.
    fn type_name_to_value_type(type_name: &str) -> Option<ValueType> {
        match type_name {
            "string" => Some(ValueType::String(None)),
            "number" => Some(ValueType::Number),
            "boolean" => Some(ValueType::Boolean(None)),
            "table" => Some(ValueType::Table(None)),
            "function" => Some(ValueType::Function(None)),
            _ => None,
        }
    }

    /// Extract the type name string literal from an expression pair (either order).
    fn extract_type_name_literal(lhs: &Expression<'_>, rhs: &Expression<'_>) -> Option<&'static str> {
        let lit_expr = match (lhs, rhs) {
            (_, Expression::Literal(_)) => rhs,
            (Expression::Literal(_), _) => lhs,
            _ => return None,
        };
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "nil" => Some("nil"),
            "string" => Some("string"),
            "number" => Some("number"),
            "boolean" => Some("boolean"),
            "table" => Some("table"),
            "function" => Some("function"),
            "userdata" => Some("userdata"),
            "thread" => Some("thread"),
            _ => None,
        }
    }

    /// Detect `type(x) == "string"` (or "number", "boolean", "table", "function",
    /// "userdata", "thread") and return the symbol index of `x`.
    fn extract_type_guard_symbol(&self, lhs: &Expression<'_>, rhs: &Expression<'_>, scope: ScopeIndex) -> Option<SymbolIndex> {
        // Either order: type(x) == "string" or "string" == type(x)
        let (call_expr, lit_expr) = match (lhs, rhs) {
            (Expression::FunctionCall(_), Expression::Literal(_)) => (lhs, rhs),
            (Expression::Literal(_), Expression::FunctionCall(_)) => (rhs, lhs),
            _ => return None,
        };
        // Check that the literal is a valid Lua type name string
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "nil" | "string" | "number" | "boolean" | "table" | "function" | "userdata" | "thread" => {}
            _ => return None,
        }
        // Check that the call is `type(x)` with a single identifier argument
        let call = match call_expr { Expression::FunctionCall(c) => c, _ => unreachable!() };
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() != 1 || names[0] != "type" { return None; }
        let args = call.arguments()?;
        let exprs = args.expressions();
        if exprs.len() != 1 { return None; }
        if let Expression::Identifier(arg_ident) = &exprs[0] {
            let arg_names = arg_ident.names();
            if arg_names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(arg_names[0].clone()), scope);
            }
        }
        None
    }

    /// Like `extract_type_guard_symbol` but for field chains: `type(obj.field) == "string"`.
    /// Returns `(sym_idx, field_chain)` where `field_chain` has 1+ elements.
    fn extract_type_guard_field(&self, lhs: &Expression<'_>, rhs: &Expression<'_>, scope: ScopeIndex) -> Option<(SymbolIndex, Vec<String>)> {
        let (call_expr, lit_expr) = match (lhs, rhs) {
            (Expression::FunctionCall(_), Expression::Literal(_)) => (lhs, rhs),
            (Expression::Literal(_), Expression::FunctionCall(_)) => (rhs, lhs),
            _ => return None,
        };
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "nil" | "string" | "number" | "boolean" | "table" | "function" | "userdata" | "thread" => {}
            _ => return None,
        }
        let call = match call_expr { Expression::FunctionCall(c) => c, _ => unreachable!() };
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() != 1 || names[0] != "type" { return None; }
        let args = call.arguments()?;
        let exprs = args.expressions();
        if exprs.len() != 1 { return None; }
        if let Expression::Identifier(arg_ident) = &exprs[0] {
            let arg_names = arg_ident.names();
            if arg_names.len() >= 2
                && let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(arg_names[0].clone()), scope) {
                    let chain = arg_names[1..].to_vec();
                    return Some((sym_idx, chain));
                }
        }
        None
    }

    /// Extract the target symbol from a `type(x)` call expression.
    /// Returns Some(sym_idx) if the call is `type(single_identifier)`.
    pub(super) fn extract_type_call_target(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<SymbolIndex> {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() != 1 || names[0] != "type" { return None; }
        let args = call.arguments()?;
        let exprs = args.expressions();
        if exprs.len() != 1 { return None; }
        if let Expression::Identifier(arg_ident) = &exprs[0] {
            let arg_names = arg_ident.names();
            if arg_names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(arg_names[0].clone()), scope);
            }
        }
        None
    }

    /// Try to resolve a FunctionCall's callee to a FunctionIndex by walking
    /// external/local symbol → table → field chains.
    /// Resolve through StripFalsy/StripNil/SymbolRef indirection to find a table index.
    fn resolve_expr_to_table(&self, expr_id: ExprId) -> Option<TableIndex> {
        let mut current = expr_id;
        for _ in 0..10 { // limit depth to avoid infinite loops
            match self.expr(current) {
                Expr::TableConstructor(ti) => return Some(*ti),
                Expr::Literal(ValueType::Table(Some(ti))) => return Some(*ti),
                Expr::Literal(ValueType::Union(members)) => {
                    return members.iter().find_map(|m| match m {
                        ValueType::Table(Some(ti)) => Some(*ti),
                        _ => None,
                    });
                }
                Expr::StripFalsy(inner) | Expr::StripNil(inner) => { current = *inner; }
                Expr::SymbolRef(sym_idx, ver) => {
                    let ver_data = self.sym(*sym_idx).versions.get(*ver)?;
                    current = ver_data.type_source?;
                }
                _ => return None,
            }
        }
        None
    }

    /// Like `resolve_expr_to_table`, but returns ALL table indices from a union type.
    /// Follows `SymbolRef` chains via `type_source` but does NOT consult
    /// `type_narrowed_symbols` or `type_filtered_symbols` — it returns the
    /// original (pre-narrowing) type. This is intentional for
    /// `extract_bool_discriminator`, which needs the full union to discriminate.
    fn resolve_expr_to_tables(&self, expr_id: ExprId) -> Vec<TableIndex> {
        self.resolve_expr_to_tables_inner(expr_id, 0)
    }

    fn resolve_expr_to_tables_inner(&self, expr_id: ExprId, depth: usize) -> Vec<TableIndex> {
        if depth > 10 { return vec![]; }
        match self.expr(expr_id) {
            Expr::TableConstructor(ti) => vec![*ti],
            Expr::Literal(ValueType::Table(Some(ti))) => vec![*ti],
            Expr::Literal(ValueType::Union(members)) => {
                members.iter().filter_map(|m| match m {
                    ValueType::Table(Some(ti)) => Some(*ti),
                    _ => None,
                }).collect()
            }
            Expr::StripFalsy(inner) | Expr::StripNil(inner) => {
                self.resolve_expr_to_tables_inner(*inner, depth + 1)
            }
            Expr::SymbolRef(sym_idx, ver) => {
                if let Some(ver_data) = self.sym(*sym_idx).versions.get(*ver)
                    && let Some(ts) = ver_data.type_source {
                        return self.resolve_expr_to_tables_inner(ts, depth + 1);
                    }
                vec![]
            }
            Expr::FieldAccess { table, field, .. } => {
                let table = *table;
                let field = field.clone();
                let base_tables = self.resolve_expr_to_tables_inner(table, depth + 1);
                let mut result = Vec::new();
                for base_ti in base_tables {
                    if let Some(field_info) = self.ir.get_field(base_ti, &field) {
                        // Try the field's expression first
                        let sub = self.resolve_expr_to_tables_inner(field_info.expr, depth + 1);
                        if !sub.is_empty() {
                            result.extend(sub);
                        } else if let Some(ann) = &field_info.annotation {
                            // Fall back to annotation type for @field declarations
                            Self::collect_tables_from_type(ann, &mut result);
                        }
                    }
                }
                result
            }
            _ => vec![],
        }
    }

    fn collect_tables_from_type(vt: &ValueType, out: &mut Vec<TableIndex>) {
        match vt {
            ValueType::Table(Some(ti)) => out.push(*ti),
            ValueType::Union(members) => {
                for m in members {
                    if let ValueType::Table(Some(ti)) = m {
                        out.push(*ti);
                    }
                }
            }
            _ => {}
        }
    }

    fn try_resolve_call_function(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<FunctionIndex> {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.is_empty() { return None; }

        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        let sym = self.sym(sym_idx);
        let version = sym.versions.last()?;

        if names.len() == 1 {
            // Direct function call: `isType(x)`
            let expr_id = version.type_source?;
            if let Expr::FunctionDef(func_idx) = self.expr(expr_id) {
                return Some(*func_idx);
            }
            return None;
        }

        // Dotted/colon call: `Table.Method(x)` or `obj:Method()` — walk through table fields
        let resolved = version.type_source.and_then(|expr_id| self.resolve_expr_to_table(expr_id));
        if let Some(current_table) = resolved
            && let Some(result) = self.walk_table_fields_to_func(current_table, &names[1..]) {
            return Some(result);
        }
        // Fallback: symbol name matches a known class (e.g. `local UIElements = lib:Include("UIElements")`)
        let class_table = self.ir.classes.get(&names[0]).or_else(|| self.ir.ext.classes.get(&names[0]));
        if let Some(&table_idx) = class_table
            && let Some(result) = self.walk_table_fields_to_func(table_idx, &names[1..]) {
            return Some(result);
        }
        // Fallback: check addon namespace sub-tables
        self.resolve_func_via_addon_namespace(&names)
    }

    fn resolve_func_via_addon_namespace(&self, names: &[String]) -> Option<FunctionIndex> {
        let addon_idx = self.ir.addon_table_idx()?;
        for fi in self.ir.table(addon_idx).fields.values() {
            let component_table = match self.expr(fi.expr) {
                Expr::TableConstructor(ti) | Expr::Literal(ValueType::Table(Some(ti))) => *ti,
                _ => continue,
            };
            let Some(sub_field) = self.ir.get_field(component_table, &names[0]) else { continue };
            let sub_table = match self.expr(sub_field.expr) {
                Expr::TableConstructor(ti) | Expr::Literal(ValueType::Table(Some(ti))) => *ti,
                _ => continue,
            };
            if let Some(result) = self.walk_table_fields_to_func(sub_table, &names[1..]) {
                return Some(result);
            }
        }
        None
    }

    fn walk_table_fields_to_func(&self, start_table: TableIndex, names: &[String]) -> Option<FunctionIndex> {
        let mut current_table = start_table;
        for (i, name) in names.iter().enumerate() {
            let field = self.ir.get_field(current_table, name)?;
            let field_expr = self.expr(field.expr);
            if i == names.len() - 1 {
                if let Expr::FunctionDef(func_idx) = field_expr {
                    return Some(*func_idx);
                }
                return None;
            } else {
                match field_expr {
                    Expr::TableConstructor(ti) => current_table = *ti,
                    Expr::Literal(ValueType::Table(Some(ti))) => current_table = *ti,
                    _ => return None,
                }
            }
        }
        None
    }

    /// Extract type guard info from a function call with `@type-narrows`.
    /// Returns `(symbol_to_narrow, class_name)` if the callee is a type guard function.
    fn extract_type_narrows_guard(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<(SymbolIndex, String)> {
        let func_idx = self.try_resolve_call_function(call, scope)?;
        let func = self.func(func_idx);

        // Check for @type-narrows ClassName (method-style: self → ClassName)
        if let Some(ref class_name) = func.type_narrows_class {
            let ident = call.identifier()?;
            let names = ident.names();
            if names.is_empty() { return None; }
            // Target is the receiver (self) — first name in identifier for colon calls
            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
            return Some((sym_idx, class_name.clone()));
        }

        // Check for @type-narrows <target_param> <classname_param> (index-based)
        let (target_idx, classname_idx) = func.type_narrows?;

        let args = call.arguments()?.expressions();
        let ident = call.identifier()?;

        // Extract class name from string literal at classname_idx (1-based)
        if classname_idx == 0 { return None; } // classname can't be self
        let class_lit = args.get(classname_idx - 1)?;
        let class_name = if let Expression::Literal(lit) = class_lit {
            let s = lit.get_string()?;
            s.trim_matches(|c| c == '"' || c == '\'').to_string()
        } else {
            return None;
        };

        // Extract target symbol
        let sym_idx = if target_idx == 0 {
            // Target is the receiver (self) — for colon calls, first name in identifier
            let names = ident.names();
            if names.is_empty() { return None; }
            self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?
        } else {
            // Target is a call-site argument (1-based)
            let target_arg = args.get(target_idx - 1)?;
            if let Expression::Identifier(target_ident) = target_arg {
                let target_names = target_ident.names();
                if target_names.len() == 1 {
                    self.get_symbol(&SymbolIdentifier::Name(target_names[0].clone()), scope)?
                } else {
                    return None;
                }
            } else {
                return None;
            }
        };

        Some((sym_idx, class_name))
    }

    /// Apply type-narrows narrowing: record scope-level narrowing (version is pushed lazily).
    /// Returns true if narrowing was applied.
    fn apply_type_narrows(&mut self, sym_idx: SymbolIndex, class_name: &str, scope: ScopeIndex) -> bool {
        let table_idx = if let Some(&ti) = self.ir.classes.get(class_name) {
            ti
        } else if let Some(&ti) = self.ir.ext.classes.get(class_name) {
            ti
        } else {
            return false;
        };
        let narrowed = ValueType::Table(Some(table_idx));
        // Don't push a version eagerly — due to LIFO block processing, sibling
        // branches can add versions that bury this one.  Instead, the version is
        // pushed lazily when the symbol is actually referenced within the scope
        // (see `get_version_for_name` in the Identifier handler).
        self.type_narrowed_symbols.entry(scope).or_default()
            .insert(sym_idx, narrowed);
        true
    }

    /// Extract a boolean discriminator from a method call on a union receiver.
    ///
    /// When calling `x:Method()` where `x` is `A | B`, and `A:Method()` returns literal `false`
    /// while `B:Method()` returns literal `true`, returns `(sym_idx, true_types, false_types)`.
    /// This enables narrowing: then-branch → `true_types`, else-branch → `false_types`.
    fn extract_bool_discriminator(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<(SymbolIndex, ValueType, ValueType)> {
        let ident = call.identifier()?;
        let names = ident.names();
        // Must be a method/dot call with at least receiver + method name
        if names.len() < 2 { return None; }

        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        let sym = self.sym(sym_idx);
        let version = sym.versions.last()?;
        let expr_id = version.type_source?;

        // Get all table indices from the receiver's union type
        let table_indices = self.resolve_expr_to_tables(expr_id);
        if table_indices.len() < 2 { return None; }

        let method_name = &names[names.len() - 1];

        let mut true_tables: Vec<ValueType> = Vec::new();
        let mut false_tables: Vec<ValueType> = Vec::new();

        for &ti in &table_indices {
            // Walk intermediate names for chained access (e.g. x.y:Method).
            // Only resolves through TableConstructor and Literal(Table) — not
            // SymbolRef or other expr types. Sufficient for direct method calls.
            let mut current_table = ti;
            let mut ok = true;
            for name in &names[1..names.len()-1] {
                if let Some(field) = self.ir.get_field(current_table, name) {
                    match self.expr(field.expr) {
                        Expr::TableConstructor(inner_ti) => current_table = *inner_ti,
                        Expr::Literal(ValueType::Table(Some(inner_ti))) => current_table = *inner_ti,
                        _ => { ok = false; break; }
                    }
                } else {
                    ok = false;
                    break;
                }
            }
            if !ok { return None; }

            // Look up the method on this table
            let field = self.ir.get_field(current_table, method_name)?;
            let func_idx = match self.expr(field.expr) {
                Expr::FunctionDef(fi) => *fi,
                _ => return None,
            };

            let func = self.func(func_idx);
            // Check the first return annotation for a literal boolean
            let ret = func.return_annotations.first()?;
            match ret {
                ValueType::Boolean(Some(true)) => true_tables.push(ValueType::Table(Some(ti))),
                ValueType::Boolean(Some(false)) => false_tables.push(ValueType::Table(Some(ti))),
                _ => return None, // Non-literal boolean or non-boolean — bail
            }
        }

        // Must have at least one type in each branch for discrimination
        if true_tables.is_empty() || false_tables.is_empty() { return None; }

        let true_type = ValueType::make_union(true_tables);
        let false_type = ValueType::make_union(false_tables);
        Some((sym_idx, true_type, false_type))
    }

    /// Like `extract_bool_discriminator` but for field-chain method calls
    /// (e.g. `self._state.selectedAuction:IsSubRow()`).
    /// Returns `(sym_idx, field_chain, true_type, false_type)` for narrowing via `type_narrowed_fields`.
    fn extract_bool_discriminator_field(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<(SymbolIndex, Vec<String>, ValueType, ValueType)> {
        let ident = call.identifier()?;
        let names = ident.names();
        // Need at least root.field.method (3 names)
        if names.len() < 3 { return None; }

        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        let method_name = &names[names.len() - 1];
        let field_chain: Vec<String> = names[1..names.len() - 1].to_vec();

        // Resolve the field chain to find the terminal field's type.
        // Walk from the symbol's table through intermediate fields, using
        // annotation types and class lookups for intermediate resolution.
        let root_table = self.ir.find_table_for_symbol(&names[0], scope)?;
        let mut current_table = root_table;
        for name in &field_chain[..field_chain.len().saturating_sub(1)] {
            let field = self.ir.get_field(current_table, name)?;
            // Try expression-based resolution first
            match self.expr(field.expr) {
                Expr::TableConstructor(ti) => { current_table = *ti; continue; }
                Expr::Literal(ValueType::Table(Some(ti))) => { current_table = *ti; continue; }
                _ => {}
            }
            // Fall back to annotation-based class lookup
            let ann = field.annotation.as_ref()?;
            match ann {
                ValueType::Table(Some(ti)) => current_table = *ti,
                _ => return None,
            }
        }

        // Get the terminal field and resolve its type to table indices
        let terminal_field_name = field_chain.last()?;
        let terminal_field = self.ir.get_field(current_table, terminal_field_name)?;
        let field_type = terminal_field.annotation.as_ref()?;

        // Extract all table indices from the field type (must be a union of tables)
        let table_indices: Vec<TableIndex> = match field_type {
            ValueType::Union(members) => {
                let mut indices = Vec::new();
                for m in members {
                    if let ValueType::Table(Some(ti)) = m {
                        indices.push(*ti);
                    }
                }
                indices
            }
            _ => return None,
        };
        if table_indices.len() < 2 { return None; }

        let mut true_tables: Vec<ValueType> = Vec::new();
        let mut false_tables: Vec<ValueType> = Vec::new();

        for &ti in &table_indices {
            let field = self.ir.get_field(ti, method_name)?;
            let func_idx = match self.expr(field.expr) {
                Expr::FunctionDef(fi) => *fi,
                _ => return None,
            };
            let func = self.func(func_idx);
            let ret = func.return_annotations.first()?;
            match ret {
                ValueType::Boolean(Some(true)) => true_tables.push(ValueType::Table(Some(ti))),
                ValueType::Boolean(Some(false)) => false_tables.push(ValueType::Table(Some(ti))),
                _ => return None,
            }
        }

        if true_tables.is_empty() || false_tables.is_empty() { return None; }

        let true_type = ValueType::make_union(true_tables);
        let false_type = ValueType::make_union(false_tables);
        Some((sym_idx, field_chain, true_type, false_type))
    }

    /// Detect `cachedType == "string"` where `cachedType` was assigned from `type(x)`.
    /// Returns the symbol index of `x` (the original target).
    fn extract_cached_type_guard_symbol(&self, lhs: &Expression<'_>, rhs: &Expression<'_>, scope: ScopeIndex) -> Option<SymbolIndex> {
        let (ident_expr, lit_expr) = match (lhs, rhs) {
            (Expression::Identifier(_), Expression::Literal(_)) => (lhs, rhs),
            (Expression::Literal(_), Expression::Identifier(_)) => (rhs, lhs),
            _ => return None,
        };
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "nil" | "string" | "number" | "boolean" | "table" | "function" | "userdata" | "thread" => {}
            _ => return None,
        }
        let ident = match ident_expr { Expression::Identifier(i) => i, _ => unreachable!() };
        let names = ident.names();
        if names.len() != 1 { return None; }
        let alias_sym = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        self.type_of_aliases.get(&alias_sym).copied()
    }

    /// Extract the bare-name symbol from an `and` LHS (for ternary idiom suppression).
    /// Given `BinaryExpr(And, [x, ...])`, returns the symbol for `x` if it's a single name.
    pub(super) fn extract_and_lhs_symbol(expr: &Expression<'_>, resolve: impl Fn(String) -> Option<SymbolIndex>) -> Option<SymbolIndex> {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        return resolve(names[0].clone());
                    }
                }
            }
            // Parser flat form: BinaryExpr(None, [x, BinaryExpr(And, ...)])
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [Expression::Identifier(ident), Expression::BinaryExpression(rhs_bin)] = terms.as_slice()
                    && matches!(rhs_bin.kind(), Operator::And) {
                        let names = ident.names();
                        if names.len() == 1 {
                            return resolve(names[0].clone());
                        }
                    }
            }
        }
        None
    }

    /// Detect field access guards in `and` LHS (e.g. `self.field and ...` or
    /// `self.field ~= nil and ...`). Returns `(sym_idx, field_chain, strip_falsy)`
    /// where `strip_falsy` is true for bare truthiness guards and false for
    /// nil-only guards (`~= nil`).
    pub(super) fn detect_and_lhs_field_guard(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, Vec<String>, bool)> {
        // Bare field truthiness: `self.field and ...` or `self._state.x and ...`
        if let Expression::Identifier(ident) = lhs {
            let names = ident.names();
            if names.len() >= 2 {
                let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                return Some((sym_idx, names[1..].to_vec(), true));
            }
        }
        // Field nil comparison: `self.field ~= nil and ...` or `self._state.x ~= nil and ...`
        if let Expression::BinaryExpression(bin) = lhs
            && matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() >= 2 {
                            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                            return Some((sym_idx, names[1..].to_vec(), false));
                        }
                    }
                }
            }
        None
    }

    /// When lowering `a and b` where `a` is a nil/type guard (e.g. `x ~= nil`,
    /// `type(x) == "string"`), detect which symbol should be narrowed.
    /// Returns (symbol_index, guard_narrow_kind) if a guard pattern is found.
    pub(super) fn detect_and_lhs_guard(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        // Bare name: `x and ...` → truthiness guard (strip nil + false)
        if let Expression::Identifier(ident) = lhs {
            let names = ident.names();
            if names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                    .map(|s| (s, GuardNarrow::StripFalsy));
            }
        }
        if let Expression::BinaryExpression(bin) = lhs {
            // Chained and: `(x and ...) and y` → x must be truthy in y.
            // The parser may produce BinaryExpr(And, [x, ...]) or the flat form
            // BinaryExpr(None, [x, BinaryExpr(And, ...)]).
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let Some(first) = terms.first() {
                    return self.detect_and_lhs_guard(first, scope_idx);
                }
            }
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [first, Expression::BinaryExpression(rhs_bin)] = terms.as_slice()
                    && matches!(rhs_bin.kind(), Operator::And) {
                        return self.detect_and_lhs_guard(first, scope_idx);
                    }
            }
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice()
                    && let Some(sym_idx) = self.extract_type_guard_symbol(l, r, scope_idx)
                        .or_else(|| self.extract_cached_type_guard_symbol(l, r, scope_idx))
                    {
                        let narrowed_type = Self::extract_type_name_literal(l, r)
                            .and_then(Self::type_name_to_value_type);
                        return Some((sym_idx, match narrowed_type {
                            Some(vt) => GuardNarrow::FilterTo(vt),
                            None => GuardNarrow::StripNil,
                        }));
                    }
            }
            if matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
        }
        None
    }

    /// Collect ALL guard symbols from a left-associative `and` chain.
    /// For `And(And(And(a, b), c), rhs)`, given the LHS `And(And(a, b), c)`,
    /// returns guards for `[a, b, c]` — all intermediate operands that must be
    /// truthy for the RHS to execute.
    pub(super) fn collect_and_chain_guards(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Vec<(SymbolIndex, GuardNarrow)> {
        let mut guards = Vec::new();
        self.collect_and_chain_guards_inner(lhs, scope_idx, &mut guards);
        guards
    }

    fn collect_and_chain_guards_inner(&self, expr: &Expression<'_>, scope_idx: ScopeIndex, guards: &mut Vec<(SymbolIndex, GuardNarrow)>) {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Recurse into LHS to collect earlier guards
                    self.collect_and_chain_guards_inner(lhs, scope_idx, guards);
                    // The RHS of this inner `and` is also a guard for the outer RHS
                    if let Some(g) = self.detect_and_lhs_guard_leaf(rhs, scope_idx) {
                        guards.push(g);
                    }
                }
                return;
            }
            // Flat form: BinaryExpr(None, [x, BinaryExpr(And, ...)])
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [lhs, Expression::BinaryExpression(rhs_bin)] = terms.as_slice()
                    && matches!(rhs_bin.kind(), Operator::And) {
                        self.collect_and_chain_guards_inner(lhs, scope_idx, guards);
                        let rhs_terms = rhs_bin.get_terms();
                        if let [_, rhs_of_and] = rhs_terms.as_slice()
                            && let Some(g) = self.detect_and_lhs_guard_leaf(rhs_of_and, scope_idx) {
                                guards.push(g);
                            }
                        return;
                    }
            }
        }
        // Base case: a leaf expression (identifier or comparison)
        if let Some(g) = self.detect_and_lhs_guard_leaf(expr, scope_idx) {
            guards.push(g);
        }
    }

    /// Collect field-chain guards from all intermediate `and` operands.
    /// For `self.a and self.b and func(self.a, self.b)`, returns guards for
    /// both `self.a` and `self.b`. Each guard includes `strip_falsy`.
    pub(super) fn collect_and_chain_field_guards(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Vec<(SymbolIndex, Vec<String>, bool)> {
        let mut guards = Vec::new();
        self.collect_and_chain_field_guards_inner(lhs, scope_idx, &mut guards);
        guards
    }

    fn collect_and_chain_field_guards_inner(&self, expr: &Expression<'_>, scope_idx: ScopeIndex, guards: &mut Vec<(SymbolIndex, Vec<String>, bool)>) {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    self.collect_and_chain_field_guards_inner(lhs, scope_idx, guards);
                    if let Some(g) = self.detect_and_lhs_field_guard(rhs, scope_idx) {
                        guards.push(g);
                    }
                }
                return;
            }
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [lhs, Expression::BinaryExpression(rhs_bin)] = terms.as_slice()
                    && matches!(rhs_bin.kind(), Operator::And) {
                        self.collect_and_chain_field_guards_inner(lhs, scope_idx, guards);
                        let rhs_terms = rhs_bin.get_terms();
                        if let [mid, rhs_of_and] = rhs_terms.as_slice() {
                            if let Some(g) = self.detect_and_lhs_field_guard(mid, scope_idx) {
                                guards.push(g);
                            }
                            if let Some(g) = self.detect_and_lhs_field_guard(rhs_of_and, scope_idx) {
                                guards.push(g);
                            }
                        }
                        return;
                    }
            }
        }
        if let Some(g) = self.detect_and_lhs_field_guard(expr, scope_idx) {
            guards.push(g);
        }
    }

    /// Collect flavor-guard masks from all intermediate `and` operands.
    /// Returns the intersection of all detected `@flavor-narrows` masks.
    /// A return of 0 means no flavor guard was detected.
    pub(super) fn collect_and_chain_flavor_guards(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> u8 {
        if self.project_flavors == 0 { return 0; }
        let mut combined: u8 = 0;
        self.collect_and_chain_flavor_guards_inner(lhs, scope_idx, &mut combined);
        combined
    }

    fn collect_and_chain_flavor_guards_inner(&self, expr: &Expression<'_>, scope_idx: ScopeIndex, combined: &mut u8) {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    self.collect_and_chain_flavor_guards_inner(lhs, scope_idx, combined);
                    if let Some(mask) = self.detect_and_lhs_flavor_guard_leaf(rhs, scope_idx) {
                        *combined = if *combined == 0 { mask } else { *combined & mask };
                    }
                }
                return;
            }
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [lhs, Expression::BinaryExpression(rhs_bin)] = terms.as_slice()
                    && matches!(rhs_bin.kind(), Operator::And) {
                        self.collect_and_chain_flavor_guards_inner(lhs, scope_idx, combined);
                        let rhs_terms = rhs_bin.get_terms();
                        if let [mid, _] = rhs_terms.as_slice()
                            && let Some(mask) = self.detect_and_lhs_flavor_guard_leaf(mid, scope_idx) {
                                *combined = if *combined == 0 { mask } else { *combined & mask };
                            }
                        return;
                    }
            }
        }
        if let Some(mask) = self.detect_and_lhs_flavor_guard_leaf(expr, scope_idx) {
            *combined = if *combined == 0 { mask } else { *combined & mask };
        }
    }

    fn detect_and_lhs_flavor_guard_leaf(&self, expr: &Expression<'_>, scope_idx: ScopeIndex) -> Option<u8> {
        match expr {
            Expression::FunctionCall(call) => self.flavor_guard_mask_for_call(call, scope_idx),
            Expression::Identifier(ident) => self.flavor_guard_mask_for_ident(ident, scope_idx),
            Expression::GroupedExpression(g) => {
                g.get_expression().and_then(|inner| self.detect_and_lhs_flavor_guard_leaf(&inner, scope_idx))
            }
            _ => None,
        }
    }

    /// Detect a guard from a single (non-chain) expression — bare name, `x ~= nil`, or type guard.
    fn detect_and_lhs_guard_leaf(&self, expr: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        if let Expression::Identifier(ident) = expr {
            let names = ident.names();
            if names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                    .map(|s| (s, GuardNarrow::StripFalsy));
            }
        }
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice()
                    && let Some(sym_idx) = self.extract_type_guard_symbol(l, r, scope_idx)
                        .or_else(|| self.extract_cached_type_guard_symbol(l, r, scope_idx))
                    {
                        let narrowed_type = Self::extract_type_name_literal(l, r)
                            .and_then(Self::type_name_to_value_type);
                        return Some((sym_idx, match narrowed_type {
                            Some(vt) => GuardNarrow::FilterTo(vt),
                            None => GuardNarrow::StripNil,
                        }));
                    }
            }
        }
        None
    }

    /// When lowering `a or b` where `a` is an inverse nil guard (e.g. `not x`,
    /// `x == nil`), detect which symbol should be narrowed for the RHS.
    /// In `not x or f(x)`, if `not x` is true (x is nil), the or short-circuits;
    /// so when f(x) executes, x must be non-nil.
    pub(super) fn detect_or_lhs_guard(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        // `not x or ...` → x is truthy in RHS (strip nil + false)
        if let Expression::UnaryExpression(u) = lhs
            && matches!(u.kind(), Operator::Not) {
                let terms = u.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                            .map(|s| (s, GuardNarrow::StripFalsy));
                    }
                }
            }
        // `x == nil or ...` → x is non-nil in RHS
        if let Expression::BinaryExpression(bin) = lhs
            && matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
        None
    }

    /// When lowering `a or b` where `a` is an inverse field nil guard
    /// (e.g. `not self.field`, `self.field == nil`), detect the guarded field.
    pub(super) fn detect_or_lhs_field_guard(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, Vec<String>)> {
        // `not self.field or ...` or `not self._state.x or ...`
        if let Expression::UnaryExpression(u) = lhs
            && matches!(u.kind(), Operator::Not) {
                let terms = u.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() >= 2 {
                        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                        return Some((sym_idx, names[1..].to_vec()));
                    }
                }
            }
        // `self.field == nil or ...` or `self._state.x == nil or ...`
        if let Expression::BinaryExpression(bin) = lhs
            && matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() >= 2 {
                            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                            return Some((sym_idx, names[1..].to_vec()));
                        }
                    }
                }
            }
        None
    }

    /// Detect `event == "EVENT_NAME"` where `event` is a simple identifier being
    /// compared to a string literal. Store as a deferred narrowing — processed
    /// during resolve after event_params has been propagated from overload contextual typing.
    pub(super) fn try_event_param_narrowing(
        &mut self,
        lhs: &Expression<'_>,
        rhs: &Expression<'_>,
        parent_scope: ScopeIndex,
        target_scope: ScopeIndex,
    ) {
        // Extract string literal from either side
        let (ident_expr, string_literal) = match (lhs, rhs) {
            (Expression::Identifier(_), Expression::Literal(lit)) => {
                let Some(s) = lit.get_string() else { return };
                (lhs, s.trim_matches(|c: char| c == '"' || c == '\'').to_string())
            }
            (Expression::Literal(lit), Expression::Identifier(_)) => {
                let Some(s) = lit.get_string() else { return };
                (rhs, s.trim_matches(|c: char| c == '"' || c == '\'').to_string())
            }
            _ => return,
        };

        // Extract the symbol from the identifier side
        let Expression::Identifier(ident) = ident_expr else { return };
        let names = ident.names();
        if names.len() != 1 { return; }
        let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) else { return };

        // Store as deferred — will be processed during resolve once event_params is propagated
        self.deferred_event_narrowings.push((sym_idx, string_literal, target_scope));
    }

}
