//! Context guards for secret values (retail 12.x): `@secret-clears`
//! (`C_Secrets.Should*BeSecret`, `HasSecretRestrictions`) and
//! `@secret-restriction-guard` (`IsAddOnRestrictionActive`, `InCombatLockdown`)
//! prove that predicates yield no secrets in the code they guard.
//!
//! Building the IR records each guarded stretch of source as a
//! [`SecretContextRegion`]: an `if`/`elseif`/`else` or `while` body, the rest of
//! a block after an early exit or `assert`, or the right operand of `and`/`or`.
//! A region stops at nested function bodies, which run later. Both resolution
//! engines consult the regions at a call, whose `@secret-when` predicates must
//! all be cleared (a bound clear only for matching arguments), and at a struct
//! field read, whose class's `@secret-when` predicates must be (by any clear),
//! and unwrap `secret<T>` there. Which predicates an inactive restriction
//! clears is `crate::secrets::predicate_restrictions`.

use crate::ast::*;
use crate::syntax::SyntaxKind;
use crate::types::*;
use super::{ancestor_scopes, Analysis, Ir};

/// A guard argument that a later call must pass at the same position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretArgKey {
    /// No argument, or `nil`.
    Absent,
    /// A variable, identified by the assignment that set it (narrowing keeps it).
    Symbol(SymbolIndex, (u32, u32)),
    /// A static field chain on a variable (`self.unit`).
    Field(SymbolIndex, (u32, u32), Vec<String>),
    String(String),
    Number(String),
}

/// Predicates one guard clears, bound to call arguments by position (unbound
/// when `binding` is empty).
#[derive(Debug, Clone, PartialEq)]
pub struct SecretClear {
    pub predicates: Vec<String>,
    pub binding: Vec<(usize, SecretArgKey)>,
    /// The guarded code, which a write to a bound field chain inside cancels.
    pub region: (u32, u32),
}

/// What the guards around some code prove.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SecretContext {
    /// `@secret-clears *`: nothing is secret.
    pub all: bool,
    pub clears: Vec<SecretClear>,
    /// Restriction types proven inactive (bits of `secrets::RESTRICTION_TYPES`).
    pub inactive: u8,
}

/// Source `[start, end)` in `scope` (and its non-function descendants) guarded by `context`.
#[derive(Debug, Clone)]
pub struct SecretContextRegion {
    pub start: u32,
    pub end: u32,
    pub scope: ScopeIndex,
    pub context: SecretContext,
}

/// Which arguments a bound clear is checked against.
#[derive(Debug, Clone, Copy)]
pub enum ClearArgs<'a> {
    /// A call's arguments and offset: a bound clear applies when they match.
    Call(&'a [ExprId], u32),
    /// A struct field read has no arguments; any clear of the predicate applies.
    Any,
    /// Unknown arguments (a function named outside a call): only unbound clears apply.
    Unbound,
}

impl SecretContext {
    pub fn is_empty(&self) -> bool {
        !self.all && self.clears.is_empty() && self.inactive == 0
    }

    fn merge(&mut self, other: &SecretContext) {
        self.all |= other.all;
        self.inactive |= other.inactive;
        self.clears.extend(other.clears.iter().cloned());
    }

    /// Whether `predicate` yields no secrets here.
    pub fn predicate_cleared(&self, ir: &Ir, predicate: &str, args: ClearArgs<'_>) -> bool {
        if self.all || crate::secrets::predicate_restrictions(predicate).is_some_and(|set| set & !self.inactive == 0) {
            return true;
        }
        self.clears.iter().any(|clear| clear.predicates.iter().any(|p| p == predicate) && match args {
            ClearArgs::Any => true,
            ClearArgs::Unbound => clear.binding.is_empty(),
            ClearArgs::Call(call_args, offset) => clear.binding.iter().all(|(position, key)| {
                let arg = match call_args.get(*position) {
                    Some(&arg) => ir.secret_arg_key(arg),
                    None => Some(SecretArgKey::Absent),
                };
                arg.as_ref() == Some(key) && !ir.field_written_before(key, clear.region, offset)
            }),
        })
    }

    /// Whether every one of `predicates` is cleared; with no predicates, only `*` clears.
    pub fn predicates_cleared<'p>(&self, ir: &Ir, predicates: impl IntoIterator<Item = &'p str>, args: ClearArgs<'_>) -> bool {
        if self.all {
            return true;
        }
        let mut any = false;
        predicates.into_iter().all(|p| {
            any = true;
            self.predicate_cleared(ir, p, args)
        }) && any
    }
}

impl Ir {
    /// The guards in effect at `offset`: every region containing it whose scope
    /// encloses it within the same function.
    pub fn secret_context_at(&self, offset: u32) -> Option<SecretContext> {
        let mut regions = self.secret_context_regions.iter()
            .filter(|r| r.start <= offset && offset < r.end)
            .peekable();
        regions.peek()?;
        let enclosing = self.function_local_scopes_at(offset);
        let mut context: Option<SecretContext> = None;
        for region in regions.filter(|r| enclosing.contains(&r.scope)) {
            context.get_or_insert_with(SecretContext::default).merge(&region.context);
        }
        context
    }

    /// The scopes enclosing `offset`, innermost first, up to and including the
    /// nearest function body.
    fn function_local_scopes_at(&self, offset: u32) -> Vec<ScopeIndex> {
        let mut out = Vec::new();
        for scope in ancestor_scopes(&self.scopes, self.scope_at_offset(offset).unwrap_or(ScopeIndex(0))) {
            out.push(scope);
            if self.function_body_scopes.contains(&scope) {
                break;
            }
        }
        out
    }

    /// Record the function body scopes regions stop at. Called once the IR is built.
    pub(super) fn index_function_body_scopes(&mut self) {
        if !self.secret_context_regions.is_empty() {
            self.function_body_scopes = self.functions.iter().map(|f| f.scope).collect();
        }
    }

    /// The binding key of a guard or call argument: a variable (by the
    /// assignment that set it), a static field chain on one, or a literal.
    pub fn secret_arg_key(&self, expr: ExprId) -> Option<SecretArgKey> {
        let expr = self.unwrap_narrowing(expr);
        if let Some(s) = self.string_literals.get(&expr) {
            return Some(SecretArgKey::String(s.clone()));
        }
        if let Some(n) = self.number_literals.get(&expr) {
            return Some(SecretArgKey::Number(n.clone()));
        }
        match self.expr(expr) {
            Expr::Literal(ValueType::Nil) => Some(SecretArgKey::Absent),
            Expr::SymbolRef(sym, ver) => Some(SecretArgKey::Symbol(*sym, self.assignment_range(*sym, *ver)?)),
            Expr::FieldAccess { .. } => {
                let mut chain = Vec::new();
                let mut current = expr;
                loop {
                    match self.expr(current) {
                        Expr::FieldAccess { table, field, .. } => {
                            chain.push(field.clone());
                            current = self.unwrap_narrowing(*table);
                        }
                        Expr::SymbolRef(sym, ver) => {
                            chain.reverse();
                            return Some(SecretArgKey::Field(*sym, self.assignment_range(*sym, *ver)?, chain));
                        }
                        _ => return None,
                    }
                }
            }
            _ => None,
        }
    }

    /// Whether a write in the guarded region replaces what the guard was asked
    /// about before the read at `offset`: an assignment to any link of the bound
    /// chain (`self.data = other` for `self.data.unit`) that precedes the read,
    /// or sits in the same loop, whose next iteration reads it again.
    fn field_written_before(&self, key: &SecretArgKey, region: (u32, u32), offset: u32) -> bool {
        let SecretArgKey::Field(sym, _, chain) = key else { return false };
        let loop_range = self.enclosing_loop_range(offset);
        self.field_assignments.iter().any(|fa| {
            fa.root_symbol == Some(*sym)
                && chain.contains(&fa.field_name)
                && (region.0..region.1).contains(&fa.ident_start)
                && (fa.ident_start < offset || loop_range.is_some_and(|(start, end)| (start..end).contains(&fa.ident_start)))
        })
    }

    /// The innermost loop body containing `offset`.
    fn enclosing_loop_range(&self, offset: u32) -> Option<(u32, u32)> {
        self.block_scopes.iter()
            .filter(|&&(start, end, scope)| (start..end).contains(&offset) && self.scope(scope).is_loop)
            .map(|&(start, end, _)| (start, end))
            .min_by_key(|(start, end)| end - start)
    }

    /// Narrowing versions copy the definition node of the version they narrow,
    /// so it identifies the assignment across them. A branch or loop merge copies
    /// it too, but its value may come from a reassignment, so it binds nothing.
    fn assignment_range(&self, sym: SymbolIndex, mut ver: usize) -> Option<(u32, u32)> {
        for _ in 0..16 {
            let version = self.sym(sym).versions.get(ver)?;
            let range = (version.def_node.start, version.def_node.end);
            let Some(source) = version.type_source else { return Some(range) };
            match self.expr(self.unwrap_narrowing(source)) {
                Expr::BranchMerge(_) => return None,
                Expr::SymbolRef(other, prev) if *other == sym && *prev != ver => ver = *prev,
                _ => return Some(range),
            }
        }
        None
    }

    fn unwrap_narrowing(&self, mut expr: ExprId) -> ExprId {
        for _ in 0..16 {
            match self.expr(expr) {
                Expr::Grouped(inner) | Expr::StripNil(inner) | Expr::StripFalsy(inner)
                | Expr::CastRemove(inner, _) | Expr::TypeFilter(inner, _)
                | Expr::AssignNarrow { inner, .. } => expr = *inner,
                _ => break,
            }
        }
        expr
    }

    /// The call-site position of `func_idx`'s parameter `param` (`...` = the
    /// first vararg position). A method call passes its receiver as the implicit
    /// first parameter, so that one is skipped.
    pub fn param_position(&self, func_idx: FunctionIndex, param: &str, is_method_call: bool) -> Option<usize> {
        let func = self.func(func_idx);
        let skip = usize::from(is_method_call && !func.args.is_empty());
        let mut names = func.args.iter().skip(skip).map(|&s| match &self.sym(s).id {
            SymbolIdentifier::Name(n) => n.as_str(),
            _ => "",
        });
        if param == "..." { Some(names.count()) } else { names.position(|n| n == param) }
    }

    /// Whether the guards at `offset` clear every `@secret-when` predicate of a
    /// call to `func_idx` with `args` (with none, only `*` clears).
    pub fn call_secrecy_cleared(&self, func_idx: FunctionIndex, args: &[ExprId], offset: u32) -> bool {
        self.secret_context_at(offset).is_some_and(|context| {
            let when = self.func(func_idx).secret.as_ref().map(|m| m.when.as_slice()).unwrap_or_default();
            context.predicates_cleared(self, when.iter().map(|p| p.name.as_str()), ClearArgs::Call(args, offset))
        })
    }

    /// Whether the guards at `offset` clear the `@secret-when` predicates of
    /// every class `receiver` may be (with none, only `*` clears).
    pub fn field_secrecy_cleared(&self, receiver: &ValueType, offset: u32) -> bool {
        self.secret_context_at(offset).is_some_and(|context| {
            context.all || self.class_secret_predicates(receiver)
                .is_some_and(|preds| context.predicates_cleared(self, preds.iter().map(String::as_str), ClearArgs::Any))
        })
    }

    /// The `@secret-when` predicates of the classes a value may be, parents
    /// included; `None` when some member is no class.
    pub fn class_secret_predicates(&self, value: &ValueType) -> Option<Vec<String>> {
        let mut tables = Vec::new();
        collect_receiver_tables(value, &mut tables)?;
        let mut predicates: Vec<String> = Vec::new();
        let mut seen = crate::collections::HashSet::default();
        while let Some(table_idx) = tables.pop() {
            if !seen.insert(table_idx) {
                continue;
            }
            let table = self.table(table_idx);
            for p in &table.secret_when {
                if !predicates.contains(p) {
                    predicates.push(p.clone());
                }
            }
            tables.extend(table.parent_classes.iter().copied());
        }
        Some(predicates)
    }
}

/// The classes of a field-read receiver. `nil` and `any` members carry no fields
/// of their own and are dropped, so a `Class|any` receiver reads like the class
/// (which is also all the display can see); anything else — an inline shape whose
/// fields have no class to carry predicates — gives up.
fn collect_receiver_tables(value: &ValueType, out: &mut Vec<TableIndex>) -> Option<()> {
    match value {
        ValueType::Table(Some(idx)) => out.push(*idx),
        ValueType::Nil | ValueType::Any => {}
        ValueType::OpaqueAlias(_, inner) => collect_receiver_tables(inner, out)?,
        ValueType::Union(members) | ValueType::Intersection(members) => {
            for m in members {
                collect_receiver_tables(m, out)?;
            }
        }
        _ => return None,
    }
    Some(())
}

impl<'a> Analysis<'a> {
    /// What `expr` evaluating truthy (`truthy`) or falsy proves about secrets.
    pub(super) fn secret_context_of(&self, expr: &Expression<'_>, scope: ScopeIndex, truthy: bool) -> SecretContext {
        let mut context = SecretContext::default();
        self.collect_secret_context(expr, scope, truthy, &mut context);
        context
    }

    fn collect_secret_context(&self, expr: &Expression<'_>, scope: ScopeIndex, truthy: bool, out: &mut SecretContext) {
        match expr {
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.collect_secret_context(&inner, scope, truthy, out);
                }
            }
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                if let Some(inner) = u.get_terms().first() {
                    self.collect_secret_context(inner, scope, !truthy, out);
                }
            }
            Expression::BinaryExpression(bin) => {
                let terms = bin.get_terms();
                match (bin.kind(), truthy, terms.as_slice()) {
                    // Both operands of a true `and`, or of a false `or`, share its truthiness.
                    (Operator::And, true, _) | (Operator::Or, false, _) => {
                        for term in &terms {
                            self.collect_secret_context(term, scope, truthy, out);
                        }
                    }
                    // `C_Secrets and C_Secrets.ShouldAurasBeSecret()` false, or `not C_Secrets
                    // or not …` true: where the API doesn't exist no value is secret, so the
                    // guard call decides.
                    (Operator::And, false, [api, call]) if self.is_api_existence_check(api, call, scope) => {
                        self.collect_secret_context(call, scope, false, out);
                    }
                    (Operator::Or, true, [Expression::UnaryExpression(not_api), call])
                        if not_api.kind() == Operator::Not
                            && not_api.get_terms().first().is_some_and(|api| self.is_api_existence_check(api, call, scope)) =>
                    {
                        self.collect_secret_context(call, scope, true, out);
                    }
                    (op @ (Operator::Equals | Operator::NotEquals), _, [lhs, rhs]) if (op == Operator::Equals) == truthy => {
                        if !self.collect_equality_guard(lhs, rhs, scope, out) {
                            self.collect_equality_guard(rhs, lhs, scope, out);
                        }
                    }
                    _ => {}
                }
            }
            Expression::FunctionCall(call) if !truthy => self.collect_guard_call(call, scope, None, out),
            _ => {}
        }
    }

    /// Whether `api` names a global (from the API stubs) that `call` — possibly
    /// negated or parenthesized — calls through: `C_Secrets` or
    /// `C_Secrets.ShouldAurasBeSecret` for `C_Secrets.ShouldAurasBeSecret()`.
    fn is_api_existence_check(&self, api: &Expression<'_>, call: &Expression<'_>, scope: ScopeIndex) -> bool {
        let mut call = *call;
        loop {
            call = match call {
                Expression::GroupedExpression(g) => match g.get_expression() {
                    Some(inner) => inner,
                    None => return false,
                },
                Expression::UnaryExpression(u) if u.kind() == Operator::Not => match u.get_terms().first() {
                    Some(inner) => *inner,
                    None => return false,
                },
                _ => break,
            };
        }
        let (Expression::Identifier(api), Expression::FunctionCall(call)) = (api, call) else { return false };
        let Some(callee) = call.identifier() else { return false };
        let (api_path, callee_path) = (api.names(), callee.names());
        !api.has_any_dynamic_bracket()
            && callee_path.starts_with(&api_path)
            && api_path.first()
                .and_then(|root| self.get_symbol(&SymbolIdentifier::Name(root.clone()), scope))
                .is_some_and(SymbolIndex::is_external)
    }

    /// `call == Value`: a guard whose `== Value` form matches `value`.
    fn collect_equality_guard(&self, call: &Expression<'_>, value: &Expression<'_>, scope: ScopeIndex, out: &mut SecretContext) -> bool {
        let (Expression::FunctionCall(call), Expression::Identifier(value)) = (call, value) else { return false };
        if value.has_any_dynamic_bracket() {
            return false;
        }
        self.collect_guard_call(call, scope, Some(&value.names().join(".")), out);
        true
    }

    /// A clearing result of a `@secret-clears` / `@secret-restriction-guard`
    /// call: false (`equals` = `None`) or equal to `equals`.
    fn collect_guard_call(&self, call: &FunctionCall<'_>, scope: ScopeIndex, equals: Option<&str>, out: &mut SecretContext) {
        let Some(func_idx) = self.resolve_call_function_by_path(call, scope) else { return };
        let Some(meta) = self.func(func_idx).secret.as_deref() else { return };
        if meta.clears.is_none() && meta.restriction_guard.is_none() {
            return;
        }
        let range = call.syntax().text_range();
        let call_args = self.ir.call_exprs_at_range((u32::from(range.start()), u32::from(range.end())))
            .find_map(|(_, e)| match e {
                Expr::FunctionCall { args, .. } => Some(args.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let is_method = call.syntax().kind() == SyntaxKind::MethodCall;
        let arg_key = |param: &str| {
            let position = self.ir.param_position(func_idx, param, is_method)?;
            let key = match call_args.get(position) {
                Some(&arg) => self.ir.secret_arg_key(arg)?,
                None => SecretArgKey::Absent,
            };
            Some((position, key))
        };
        if let Some(clears) = meta.clears.as_ref().filter(|c| c.equals.as_deref() == equals) {
            // A clear bound to an argument that isn't a variable, field chain, or
            // literal proves nothing about later calls.
            if let Some(binding) = clears.params.iter().map(|p| arg_key(p)).collect::<Option<Vec<_>>>() {
                // `*` clears the whole region, including reports no call
                // produced, so it binds no argument (rejected at parse).
                if clears.all {
                    out.all = true;
                } else {
                    out.clears.push(SecretClear { predicates: clears.predicates.clone(), binding, region: (0, 0) });
                }
            }
        }
        if let Some(guard) = meta.restriction_guard.as_ref().filter(|g| g.equals.as_deref() == equals) {
            let bit = guard.fixed_bit().or_else(|| {
                let position = self.ir.param_position(func_idx, &guard.restriction, is_method)?;
                self.restriction_bit_of(*call_args.get(position)?)
            });
            out.inactive |= bit.unwrap_or(0);
        }
    }

    /// The restriction type an argument names: `Enum.AddOnRestrictionType.<Type>`
    /// (also through a local alias of the enum) or its number.
    fn restriction_bit_of(&self, arg: ExprId) -> Option<u8> {
        if let Some(n) = self.ir.number_literals.get(&arg) {
            let index: usize = n.parse().ok()?;
            return crate::secrets::RESTRICTION_TYPES.get(index).map(|_| 1 << index);
        }
        let (root, root_version, chain) = self.field_chain_with_root_version(arg)?;
        let SymbolIdentifier::Name(root_name) = &self.sym(root).id else { return None };
        let path: Vec<String> = std::iter::once(root_name.clone()).chain(chain.iter().cloned()).collect();
        crate::secrets::restriction_bit_for_path(&path).or_else(|| {
            let alias_source = self.sym(root).versions.get(root_version)?.type_source?;
            let (alias_root, alias_chain) = self.ir.extract_field_chain(alias_source)?;
            let SymbolIdentifier::Name(alias_root_name) = &self.sym(alias_root).id else { return None };
            let path: Vec<String> = std::iter::once(alias_root_name.clone()).chain(alias_chain).chain(chain).collect();
            crate::secrets::restriction_bit_for_path(&path)
        })
    }

    /// `extract_field_chain`, plus the version of the root variable read.
    fn field_chain_with_root_version(&self, expr: ExprId) -> Option<(SymbolIndex, usize, Vec<String>)> {
        let mut chain = Vec::new();
        let mut current = expr;
        loop {
            match self.expr(current) {
                Expr::FieldAccess { table, field, .. } => {
                    chain.push(field.clone());
                    current = *table;
                }
                Expr::Grouped(inner) | Expr::StripNil(inner) | Expr::StripFalsy(inner) => current = *inner,
                Expr::SymbolRef(sym, ver) => {
                    chain.reverse();
                    return Some((*sym, *ver, chain));
                }
                _ => return None,
            }
        }
    }

    /// Record that `context` holds for `[start, end)` in `scope`.
    pub(super) fn record_secret_context(&mut self, mut context: SecretContext, scope: ScopeIndex, (start, end): (u32, u32)) {
        if context.is_empty() || start >= end {
            return;
        }
        for clear in &mut context.clears {
            clear.region = (start, end);
        }
        // An `elseif` condition is lowered in its branch's scope but sits outside
        // that block, so the region belongs to the nearest scope containing it.
        let mut scope = scope;
        for _ in 0..16 {
            if self.block_range_containing(scope, start).is_some() {
                break;
            }
            match self.ir.scope(scope).parent {
                Some(parent) => scope = parent,
                None => break,
            }
        }
        // An expression lowered more than once (`local a, b = f() and g()`) records its region again.
        let recorded = self.ir.secret_context_regions.iter()
            .any(|r| (r.start, r.end, r.scope) == (start, end, scope) && r.context == context);
        if !recorded {
            self.ir.secret_context_regions.push(SecretContextRegion { start, end, scope, context });
        }
    }

    /// Record what `cond` evaluating `truthy` proves for `range` in `scope`.
    pub(super) fn record_secret_guard(&mut self, cond: &Expression<'_>, parent_scope: ScopeIndex, scope: ScopeIndex, truthy: bool, range: (u32, u32)) {
        let context = self.secret_context_of(cond, parent_scope, truthy);
        self.record_secret_context(context, scope, range);
    }

    /// The source range of the block whose scope is `scope` and that contains `offset`.
    pub(super) fn block_range_containing(&self, scope: ScopeIndex, offset: u32) -> Option<(u32, u32)> {
        self.ir.block_scopes.iter()
            .filter(|&&(start, end, s)| s == scope && start <= offset && offset < end)
            .map(|&(start, end, _)| (start, end))
            .min_by_key(|(start, end)| end - start)
    }
}
