//! Narrowing from `@secret-guard` calls (`issecretvalue`, `canaccessvalue`, …).
//!
//! A guard's boolean result proves its argument secret or non-secret. The facts
//! reuse the `type()`-guard machinery with the `ValueType::secret_guard()`
//! wildcard: filtering to it keeps secret values, stripping it unwraps them.

use crate::ast::*;
use crate::collections::HashMap;
use crate::syntax::SyntaxKind;
use crate::types::*;
use super::narrowing::GuardNarrow;
use super::secret_context::SecretContext;
use super::{Analysis, NarrowTarget};

/// Symbol targets with the narrowing each receives.
type SymbolGuards = Vec<(SymbolIndex, GuardNarrow)>;
/// Field-chain targets with the narrowing each receives.
type FieldGuards = Vec<(SymbolIndex, Vec<String>, GuardNarrow)>;

/// What a guard call's truthiness proves.
pub(super) enum SecretFact {
    /// Every guarded target is non-secret.
    Accessible(Vec<NarrowTarget>),
    /// The (single) guarded target is secret.
    Secret(NarrowTarget),
}

/// What a boolean variable holding a guard's result proves, by that result:
/// `local ok = canaccessvalue(x)` makes `if ok then` narrow `x` exactly as the
/// call would. A later write with no guard drops the entry, so only the value
/// the variable currently holds is trusted.
#[derive(Default)]
pub(crate) struct StoredSecretGuards {
    symbols: HashMap<SymbolIndex, StoredGuard>,
}

#[derive(Default)]
struct StoredGuard {
    fact_true: Option<StoredFact>,
    fact_false: Option<StoredFact>,
    context_true: SecretContext,
    context_false: SecretContext,
}

/// A value-guard fact, flattened so it survives the guard call's expression.
struct StoredFact {
    /// Whether the targets are proven secret rather than accessible.
    secret: bool,
    targets: Vec<NarrowTarget>,
}

impl StoredSecretGuards {
    /// The value-guard fact `sym` carries for a `truthy` result.
    fn fact(&self, sym: SymbolIndex, truthy: bool) -> Option<SecretFact> {
        let guard = self.symbols.get(&sym)?;
        let stored = if truthy { guard.fact_true.as_ref() } else { guard.fact_false.as_ref() }?;
        Some(if stored.secret {
            SecretFact::Secret(stored.targets.first()?.clone())
        } else {
            SecretFact::Accessible(stored.targets.clone())
        })
    }

    /// The context guards `sym` carries for a `truthy` result.
    pub(super) fn context(&self, sym: SymbolIndex, truthy: bool) -> Option<SecretContext> {
        let guard = self.symbols.get(&sym)?;
        let context = if truthy { &guard.context_true } else { &guard.context_false };
        (!context.is_empty()).then(|| context.clone())
    }
}

impl SecretFact {
    fn narrow(&self) -> GuardNarrow {
        match self {
            SecretFact::Accessible(_) => GuardNarrow::StripType(ValueType::secret_guard()),
            SecretFact::Secret(_) => GuardNarrow::FilterTo(ValueType::secret_guard()),
        }
    }

    fn into_targets(self) -> Vec<NarrowTarget> {
        match self {
            SecretFact::Accessible(targets) => targets,
            SecretFact::Secret(target) => vec![target],
        }
    }
}

impl<'a> Analysis<'a> {
    /// The facts established when `expr` — a guard call, possibly negated,
    /// parenthesized, or joined to an operand that can't decide the result —
    /// evaluates truthy (`truthy`) or falsy.
    pub(super) fn secret_guard_fact(&self, expr: &Expression<'_>, scope: ScopeIndex, truthy: bool) -> Option<SecretFact> {
        match expr {
            Expression::GroupedExpression(g) => self.secret_guard_fact(&g.get_expression()?, scope, truthy),
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                self.secret_guard_fact(u.get_terms().first()?, scope, !truthy)
            }
            // `issecretvalue and issecretvalue(x)` / `not issecretvalue or not
            // issecretvalue(x)`: checking that a guard exists (it may not on older
            // clients) has fixed truthiness, so the other operand decides the result.
            Expression::BinaryExpression(bin) => {
                let is_and = match bin.kind() {
                    Operator::And => true,
                    Operator::Or => false,
                    _ => return None,
                };
                let [lhs, rhs] = <[Expression<'_>; 2]>::try_from(bin.get_terms()).ok()?;
                // An always-truthy `and` operand, or an always-falsy `or` operand, drops out.
                let decider = if self.fixed_truthiness(&lhs, scope) == Some(is_and) {
                    rhs
                } else if self.fixed_truthiness(&rhs, scope) == Some(is_and) {
                    lhs
                } else {
                    return None;
                };
                self.secret_guard_fact(&decider, scope, truthy)
            }
            // A boolean holding a guard's result (`local ok = canaccessvalue(x)`).
            Expression::Identifier(ident) => {
                self.stored_secret_guards.fact(self.stored_guard_symbol(ident, scope)?, truthy)
            }
            Expression::FunctionCall(call) => {
                let (kind, targets) = self.secret_guard_call(call, scope)?;
                if kind.implies_accessible(truthy, targets.len())? {
                    let targets: Vec<NarrowTarget> = targets.into_iter().flatten().collect();
                    (!targets.is_empty()).then_some(SecretFact::Accessible(targets))
                } else {
                    targets.into_iter().next().flatten().map(SecretFact::Secret)
                }
            }
            _ => None,
        }
    }

    /// The symbol a single-name identifier that may carry a stored guard refers
    /// to; `None` for a field chain or bracket access, which carry none.
    pub(super) fn stored_guard_symbol(&self, ident: &Identifier<'_>, scope: ScopeIndex) -> Option<SymbolIndex> {
        let names = ident.names();
        let [name] = names.as_slice() else { return None };
        self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope)
    }

    /// Record what a write of `expression` to `sym` leaves the variable
    /// holding: the guard facts a later truth test of it can use, or nothing,
    /// which drops whatever an earlier write stored.
    pub(super) fn record_stored_secret_guard(&mut self, sym: SymbolIndex, scope: ScopeIndex, expression: Option<&Expression<'_>>) {
        let stored = expression.map(|expr| StoredGuard {
            fact_true: self.stored_fact(expr, scope, true),
            fact_false: self.stored_fact(expr, scope, false),
            context_true: self.secret_context_of(expr, scope, true),
            context_false: self.secret_context_of(expr, scope, false),
        });
        match stored.filter(|g| {
            g.fact_true.is_some() || g.fact_false.is_some() || !g.context_true.is_empty() || !g.context_false.is_empty()
        }) {
            Some(guard) => { self.stored_secret_guards.symbols.insert(sym, guard); }
            None => { self.stored_secret_guards.symbols.remove(&sym); }
        }
    }

    fn stored_fact(&self, expr: &Expression<'_>, scope: ScopeIndex, truthy: bool) -> Option<StoredFact> {
        let fact = self.secret_guard_fact(expr, scope, truthy)?;
        let secret = matches!(fact, SecretFact::Secret(_));
        Some(StoredFact { secret, targets: fact.into_targets() })
    }

    /// If `call` invokes a `@secret-guard` function, its guard kind and the
    /// guarded arguments (`None` for an argument that isn't a plain name or a
    /// static field chain).
    fn secret_guard_call(
        &self,
        call: &FunctionCall<'_>,
        scope: ScopeIndex,
    ) -> Option<(crate::secrets::SecretGuardKind, Vec<Option<NarrowTarget>>)> {
        let func_idx = self.resolve_call_function_by_path(call, scope)?;
        let guard = self.func(func_idx).secret.as_ref()?.guard.as_ref()?;
        let args = call.arguments().map(|a| a.expressions()).unwrap_or_default();
        let is_method = call.syntax().kind() == SyntaxKind::MethodCall;
        let pos = self.param_position(func_idx, &guard.param, is_method)?;
        let guarded: &[Expression<'_>] = if guard.param == "..." {
            args.get(pos..).unwrap_or_default()
        } else {
            std::slice::from_ref(args.get(pos)?)
        };
        let targets = guarded.iter().map(|arg| self.secret_narrow_target(arg, scope)).collect();
        Some((guard.kind, targets))
    }

    /// `Some(true)` for an expression that is always truthy — a reference to a
    /// function, by name — `Some(false)` for its negation, `None` otherwise.
    fn fixed_truthiness(&self, expr: &Expression<'_>, scope: ScopeIndex) -> Option<bool> {
        match expr {
            Expression::GroupedExpression(g) => self.fixed_truthiness(&g.get_expression()?, scope),
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                self.fixed_truthiness(u.get_terms().first()?, scope).map(|truthy| !truthy)
            }
            Expression::Identifier(ident) if ident.syntax().kind() == SyntaxKind::NameRef => {
                let name = ident.names().into_iter().next()?;
                let sym = self.get_symbol(&SymbolIdentifier::Name(name), scope)?;
                self.find_function_for_symbol(sym, scope).map(|_| true)
            }
            _ => None,
        }
    }

    pub(super) fn param_position(&self, func_idx: FunctionIndex, param: &str, is_method_call: bool) -> Option<usize> {
        self.ir.param_position(func_idx, param, is_method_call)
    }

    fn secret_narrow_target(&self, arg: &Expression<'_>, scope: ScopeIndex) -> Option<NarrowTarget> {
        let Expression::Identifier(ident) = arg else { return None };
        if ident.has_any_dynamic_bracket() {
            return None;
        }
        let names = ident.names_with_brackets();
        let sym = self.get_symbol(&SymbolIdentifier::Name(names.first()?.clone()), scope)?;
        Some(if names.len() == 1 {
            NarrowTarget::Symbol(sym)
        } else {
            NarrowTarget::Field(sym, names[1..].to_vec())
        })
    }

    /// Apply a fact to the scope a guard selects (`if` then/else, `assert`), or
    /// — `after_exit` — to the code after an early-exit guard, where symbols get
    /// creation-ordered versions so references before the guard stay un-narrowed.
    pub(super) fn apply_secret_fact(&mut self, fact: SecretFact, scope: ScopeIndex, after_exit: bool) {
        let guard = ValueType::secret_guard();
        let secret = matches!(fact, SecretFact::Secret(_));
        for target in fact.into_targets() {
            match (target, secret) {
                (NarrowTarget::Symbol(sym), true) if after_exit => {
                    self.push_type_filter_version(sym, guard.clone(), scope, true);
                }
                (NarrowTarget::Symbol(sym), true) => {
                    self.narrowing.type_filtered_symbols.entry(scope).or_default().insert(sym, guard.clone());
                }
                (NarrowTarget::Symbol(sym), false) => {
                    if !after_exit {
                        self.add_type_stripped(scope, sym, guard.clone());
                    }
                    self.push_strip_type_version(sym, guard.clone(), scope, after_exit);
                }
                (target @ NarrowTarget::Field(..), true) => {
                    self.narrowing.type_narrowed.entry(scope).or_default().insert(target, guard.clone());
                }
                (NarrowTarget::Field(sym, chain), false) => self.add_type_stripped_field(scope, sym, chain, guard.clone()),
            }
        }
    }

    /// The guards the left side of an `and` chain (every operand truthy) or an
    /// `or` chain (every operand falsy) establishes for its right operand, split
    /// into symbol and field-chain targets. Each operand is resolved once.
    pub(super) fn secret_chain_guards(&self, lhs: &Expression<'_>, scope: ScopeIndex, op: Operator) -> (SymbolGuards, FieldGuards) {
        let mut operands = Vec::new();
        chain_operands(*lhs, op, &mut operands);
        let (mut symbols, mut fields) = (Vec::new(), Vec::new());
        for operand in &operands {
            let Some(fact) = self.secret_guard_fact(operand, scope, op == Operator::And) else { continue };
            let narrow = fact.narrow();
            for target in fact.into_targets() {
                match target {
                    NarrowTarget::Symbol(sym) => symbols.push((sym, narrow.clone())),
                    NarrowTarget::Field(sym, chain) => fields.push((sym, chain, narrow.clone())),
                }
            }
        }
        (symbols, fields)
    }
}

/// The operands of an `op` (`and`/`or`) chain, left to right: `a and b and c` →
/// `[a, b, c]`. Mirrors the parser's shapes — left-nested chains, parentheses,
/// and the flat `None[x, And[y, z]]` form it produces for mixed precedence.
fn chain_operands<'t>(expr: Expression<'t>, op: Operator, out: &mut Vec<Expression<'t>>) {
    match expr {
        Expression::GroupedExpression(g) => {
            if let Some(inner) = g.get_expression() {
                chain_operands(inner, op, out);
            }
        }
        Expression::BinaryExpression(bin) if bin.kind() == op => {
            if let [lhs, rhs] = bin.get_terms().as_slice() {
                chain_operands(*lhs, op, out);
                out.push(*rhs);
            }
        }
        Expression::BinaryExpression(bin) if bin.kind() == Operator::None => {
            match bin.get_terms().as_slice() {
                [lhs, Expression::BinaryExpression(inner)] if inner.kind() == op => {
                    chain_operands(*lhs, op, out);
                    out.extend(inner.get_terms());
                }
                _ => out.push(expr),
            }
        }
        _ => out.push(expr),
    }
}
