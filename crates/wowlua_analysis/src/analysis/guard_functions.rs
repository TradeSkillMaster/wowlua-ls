//! Guard functions: a function whose body is a single `return <condition>`
//! narrows the arguments of a call to it the way the condition narrows its
//! parameters — `if frame:IsScrolling() then` narrows `frame.target` when
//! `IsScrolling` returns `self.target ~= nil`. The facts come from the syntax
//! alone ([`infer_guard`]), so the workspace scan carries them to other files
//! (`ExternalGlobal::inferred_guard`).

use crate::ast::*;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::types::*;
use super::narrowing::{chain_operands, FieldGuards, GuardNarrow, SymbolGuards};
use super::Analysis;

/// The guard `func` is, when its body is a single `return` of one value that
/// says something about its parameters (`is_colon`: `self` is the first).
pub fn infer_guard(func: &FunctionDefinition<'_>, is_colon: bool) -> Option<Box<InferredGuard>> {
    let statements = func.block()?.statements();
    let [Statement::Return(ret)] = statements.as_slice() else { return None };
    let values = ret.expression_list()?.expressions();
    let [value] = values.as_slice() else { return None };
    let mut params = func.params()?.parameters();
    if is_colon {
        params.insert(0, "self".to_string());
    }
    let mut guard = InferredGuard::default();
    collect_facts(value, true, &params, &mut guard.when_true);
    collect_facts(value, false, &params, &mut guard.when_false);
    (!guard.when_true.is_empty() || !guard.when_false.is_empty()).then(|| Box::new(guard))
}

/// What `expr` evaluating truthy (`truthy`) or falsy proves about `params`.
fn collect_facts(expr: &Expression<'_>, truthy: bool, params: &[String], out: &mut Vec<GuardFact>) {
    match expr {
        Expression::GroupedExpression(g) => {
            if let Some(inner) = g.get_expression() {
                collect_facts(&inner, truthy, params, out);
            }
        }
        Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
            if let Some(operand) = u.get_terms().first() {
                collect_facts(operand, !truthy, params, out);
            }
        }
        Expression::Identifier(ident) => {
            let kind = if truthy { GuardFactKind::Truthy } else { GuardFactKind::Falsy };
            push_fact(ident.syntax(), kind, params, out);
        }
        Expression::BinaryExpression(bin) => {
            let terms = bin.get_terms();
            let [lhs, rhs] = terms.as_slice() else { return };
            match bin.kind() {
                op @ (Operator::And | Operator::Or) => {
                    let is_and = op == Operator::And;
                    if truthy == is_and {
                        // A truthy `and` or a falsy `or`: so is each operand.
                        collect_facts(lhs, truthy, params, out);
                        collect_facts(rhs, truthy, params, out);
                    } else if literal_truthiness(rhs) == Some(is_and) {
                        // Otherwise only an operand of fixed truthiness drops out:
                        // `x ~= nil and true or false` is decided by `x ~= nil`.
                        collect_facts(lhs, truthy, params, out);
                    } else if literal_truthiness(lhs) == Some(is_and) {
                        collect_facts(rhs, truthy, params, out);
                    }
                }
                op @ (Operator::Equals | Operator::NotEquals) => {
                    if let Some((subject, kind)) = equality_fact(lhs, rhs) {
                        let kind = if truthy == (op == Operator::Equals) { kind } else { kind.negated() };
                        push_fact(subject, kind, params, out);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// What `lhs == rhs` proves, and the operand it is about: `x == nil` that `x`
/// is nil, `type(x) == "string"` that `x` is a string.
fn equality_fact<'t>(lhs: &Expression<'t>, rhs: &Expression<'t>) -> Option<(SyntaxNode<'t>, GuardFactKind)> {
    let is_nil = |e: &Expression<'_>| matches!(e, Expression::Literal(lit) if lit.is_nil());
    if is_nil(rhs) {
        return Some((lhs.syntax(), GuardFactKind::Nil));
    }
    if is_nil(lhs) {
        return Some((rhs.syntax(), GuardFactKind::Nil));
    }
    let type_name = Analysis::extract_type_name_literal(lhs, rhs)?;
    let call = [lhs, rhs].into_iter().find_map(|e| match e {
        Expression::FunctionCall(call) => Some(*call),
        _ => None,
    })?;
    if call.syntax().kind() != SyntaxKind::FunctionCall || call.identifier()?.names() != ["type"] {
        return None;
    }
    let args = call.arguments()?.expressions();
    let [arg] = args.as_slice() else { return None };
    Some((arg.syntax(), GuardFactKind::TypeIs(type_name.to_string())))
}

/// Record `kind` for `subject` when it is a parameter or a field chain below one.
fn push_fact(subject: SyntaxNode<'_>, kind: GuardFactKind, params: &[String], out: &mut Vec<GuardFact>) {
    let Some(mut chain) = dotted_names(subject) else { return };
    let param = chain.remove(0);
    // A name the parameter list repeats is ambiguous at the call site.
    if params.iter().filter(|p| **p == param).count() == 1 {
        out.push(GuardFact { param, chain, kind });
    }
}

/// The truthiness of a literal operand, which no value changes.
fn literal_truthiness(expr: &Expression<'_>) -> Option<bool> {
    match expr {
        Expression::GroupedExpression(g) => literal_truthiness(&g.get_expression()?),
        Expression::Literal(lit) => Some(!lit.is_nil() && lit.get_bool() != Some(false)),
        Expression::TableConstructor(_) | Expression::Function(_) => Some(true),
        _ => None,
    }
}

/// The names of a plain name chain — `x` or `x.y.z` — or `None` when a link is
/// a bracket index, a call, or parenthesized.
fn dotted_names(node: SyntaxNode<'_>) -> Option<Vec<String>> {
    let own_name = || node.children_with_tokens().find_map(|child| match child {
        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some(t.text().to_string()),
        _ => None,
    });
    match node.kind() {
        SyntaxKind::NameRef => Some(vec![own_name()?]),
        SyntaxKind::DotAccess => {
            let mut names = dotted_names(node.children().next()?)?;
            names.push(own_name()?);
            Some(names)
        }
        _ => None,
    }
}

/// Where a guard call's facts apply, which decides how they narrow: the same
/// way the condition they stand for would, written there.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum GuardSite {
    /// The branch (or loop body) the call's condition selects.
    Branch,
    /// The code after `assert(call)`.
    Assert,
    /// The code after an early exit the call's result skipped.
    AfterExit,
}

/// The narrowing a fact gives the right operand of an `and`/`or`, when it has one.
fn operand_narrow(kind: &GuardFactKind) -> Option<GuardNarrow> {
    match kind {
        GuardFactKind::Truthy => Some(GuardNarrow::StripFalsy),
        GuardFactKind::NonNil => Some(GuardNarrow::StripNil),
        GuardFactKind::TypeIs(name) => Analysis::type_name_to_value_type(name).map(GuardNarrow::FilterTo),
        GuardFactKind::TypeIsNot(name) if name == "nil" => Some(GuardNarrow::StripNil),
        GuardFactKind::TypeIsNot(name) => Analysis::type_name_to_value_type(name).map(GuardNarrow::StripType),
        GuardFactKind::Falsy | GuardFactKind::Nil => None,
    }
}

impl<'a> Analysis<'a> {
    /// What `call` proves when its result is `truthy` (or falsy), if it calls a
    /// guard function: each fact with its target as the caller's name chain —
    /// the argument (for a method's `self`, the receiver) followed by the
    /// fact's field chain. An argument that isn't a plain name chain proves
    /// nothing. A call that may run one of several functions (`obj:Check()`
    /// with `obj` an `A|B`) proves only the facts each of them does.
    fn guard_call_facts(&self, call: &FunctionCall<'_>, scope: ScopeIndex, truthy: bool) -> Vec<(Vec<String>, GuardFactKind)> {
        let mut callees = self.guard_callees(call, scope).into_iter();
        let Some(first) = callees.next() else { return Vec::new() };
        let mut facts = self.callee_facts(first, call, truthy);
        for func_idx in callees {
            let other = self.callee_facts(func_idx, call, truthy);
            facts.retain(|fact| other.contains(fact));
        }
        facts
    }

    /// [`Self::guard_call_facts`] for one function `call` may run.
    fn callee_facts(&self, func_idx: FunctionIndex, call: &FunctionCall<'_>, truthy: bool) -> Vec<(Vec<String>, GuardFactKind)> {
        let func = self.func(func_idx);
        let Some(guard) = func.inferred_guard.as_deref() else { return Vec::new() };
        let is_method = call.syntax().kind() == SyntaxKind::MethodCall;
        let receiver_param = func.args.first().filter(|_| is_method).map(|&sym| &self.sym(sym).id);
        let args = call.arguments().map(|a| a.expressions()).unwrap_or_default();
        let facts = if truthy { &guard.when_true } else { &guard.when_false };
        facts.iter().filter_map(|fact| {
            let arg = if receiver_param.is_some_and(|id| matches!(id, SymbolIdentifier::Name(n) if *n == fact.param)) {
                call.syntax().children().next()?
            } else {
                args.get(self.param_position(func_idx, &fact.param, is_method)?)?.syntax()
            };
            let mut names = dotted_names(arg)?;
            names.extend(fact.chain.iter().cloned());
            Some((names, fact.kind.clone()))
        }).collect()
    }

    /// Every function `call` may run: for `a.b:m()` / `a.b.m()`, `m` on each
    /// class `a.b` may be — through `nil` (`---@field b B?`), but not choosing
    /// among the classes of a union — or none when one of them lacks it. A
    /// call whose prefix doesn't resolve to classes that way runs the function
    /// its name path does.
    fn guard_callees(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Vec<FunctionIndex> {
        let names = call.identifier().map(|ident| ident.names()).unwrap_or_default();
        if let Some((name, path)) = names.split_last()
            && let Some(tables) = self.path_tables(path, scope)
        {
            let functions: Option<Vec<FunctionIndex>> = tables.iter().map(|&table| {
                match self.expr(self.ir.get_field(table, name)?.expr) {
                    Expr::FunctionDef(func_idx) | Expr::Literal(ValueType::Function(Some(func_idx))) => Some(*func_idx),
                    _ => None,
                }
            }).collect();
            match functions {
                Some(functions) => return functions,
                None if tables.len() > 1 => return Vec::new(),
                None => {}
            }
        }
        self.resolve_call_function_by_path(call, scope).into_iter().collect()
    }

    /// The classes the value of name path `path` may be: the root's, then
    /// each field's in turn, keeping every class of a union and dropping
    /// `nil`. `None` when the path doesn't resolve to classes that way and
    /// never passed a union of several; empty when it did.
    fn path_tables(&self, path: &[String], scope: ScopeIndex) -> Option<Vec<TableIndex>> {
        let sym = self.get_symbol(&SymbolIdentifier::Name(path.first()?.clone()), scope)?;
        let mut tables = match self.sym(sym).versions.get(self.ir.version_for_scope(sym, scope))?.type_source {
            Some(source) => self.resolve_expr_to_tables(source),
            None => self.find_table_for_symbol_phase1(sym, scope).into_iter().collect(),
        };
        let mut several = tables.len() > 1;
        for name in &path[1..] {
            let mut next = Vec::new();
            for &table in &tables {
                if let Some(field) = self.ir.get_field(table, name) {
                    let mut field_tables = self.resolve_expr_to_tables(field.expr);
                    if field_tables.is_empty() && let Some(annotation) = &field.annotation {
                        Self::collect_tables_from_type(annotation, &mut field_tables);
                    }
                    next.extend(field_tables);
                }
            }
            next.sort_unstable();
            next.dedup();
            tables = next;
            several |= tables.len() > 1;
        }
        match (tables.is_empty(), several) {
            (true, false) => None,
            (true, true) => Some(Vec::new()),
            (false, _) => Some(tables),
        }
    }

    /// Narrow `target` by what `call` proves when its result is `truthy`, if
    /// it calls a guard function. `lookup` is the scope the call runs in.
    pub(super) fn narrow_guard_call(&mut self, call: &FunctionCall<'_>, lookup: ScopeIndex, target: ScopeIndex, truthy: bool, site: GuardSite) {
        for (names, kind) in self.guard_call_facts(call, lookup, truthy) {
            self.apply_guard_fact(&names, &kind, lookup, target, site);
        }
    }

    /// Narrow `names` by `kind` as the condition it stands for (`x`, `x ~= nil`,
    /// `type(x) == "t"`, …) narrows at `site` — or doesn't: an `assert` proves
    /// nothing from a falsy operand, nor an early exit from `x ~= nil`.
    fn apply_guard_fact(&mut self, names: &[String], kind: &GuardFactKind, lookup: ScopeIndex, target: ScopeIndex, site: GuardSite) {
        use GuardFactKind::*;
        let Some(sym) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), lookup) else { return };
        if names.len() == 1 {
            match (kind, site) {
                (Truthy, GuardSite::Branch) => self.narrow_symbol_truthy(sym, target),
                (Truthy, _) => {
                    self.narrow_symbol_strip_falsy(sym, target);
                    self.try_apply_type_guard_alias(sym, target, true);
                }
                (Falsy, GuardSite::Branch | GuardSite::AfterExit) => self.narrow_symbol_falsy(sym, target),
                (NonNil, GuardSite::AfterExit) => self.narrow_symbol_strip_nil(sym, target),
                (NonNil, _) => self.narrow_symbol_non_nil(sym, target),
                (Nil, GuardSite::Branch) => self.narrow_symbol_nil(sym, target),
                (TypeIs(name) | TypeIsNot(name), GuardSite::AfterExit) => {
                    self.narrow_symbol_type_guard_after_exit(sym, name, matches!(kind, TypeIs(_)), target);
                }
                (TypeIs(name) | TypeIsNot(name), _) => {
                    self.narrow_symbol_type_guard(sym, name, matches!(kind, TypeIs(_)), target);
                }
                _ => {}
            }
        } else {
            match (kind, site) {
                (Truthy, _) => self.narrow_field_truthy(names, lookup, target),
                (NonNil, GuardSite::Assert) => self.try_narrow_field(names, target),
                (NonNil, _) => self.narrow_field_non_nil(names, lookup, target),
                (Falsy | Nil, GuardSite::Branch) | (Falsy, GuardSite::AfterExit) => {
                    self.narrow_field_absent(names, lookup, target);
                }
                (TypeIs(name) | TypeIsNot(name), _) => {
                    self.narrow_field_type_guard(sym, names[1..].to_vec(), name, matches!(kind, TypeIs(_)), target);
                }
                _ => {}
            }
        }
    }

    /// The guards that guard-function calls on the left of an `and` chain
    /// (each truthy) or an `or` chain (each falsy) establish for its right
    /// operand, split into symbol and field-chain targets.
    pub(super) fn guard_call_chain_guards(&self, lhs: &Expression<'_>, scope: ScopeIndex, op: Operator) -> (SymbolGuards, FieldGuards) {
        let mut operands = Vec::new();
        chain_operands(*lhs, op, &mut operands);
        let (mut symbols, mut fields) = (Vec::new(), Vec::new());
        for operand in &operands {
            let Some((call, truthy)) = negated_call(operand, op == Operator::And) else { continue };
            for (names, kind) in self.guard_call_facts(&call, scope, truthy) {
                let Some(narrow) = operand_narrow(&kind) else { continue };
                let Some(sym) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope) else { continue };
                if names.len() == 1 {
                    symbols.push((sym, narrow));
                } else {
                    fields.push((sym, names[1..].to_vec(), narrow));
                }
            }
        }
        (symbols, fields)
    }
}

/// The call `expr` is, possibly negated or parenthesized, with the truthiness
/// of its result when `expr` is `truthy`.
fn negated_call<'t>(expr: &Expression<'t>, truthy: bool) -> Option<(FunctionCall<'t>, bool)> {
    match expr {
        Expression::FunctionCall(call) => Some((*call, truthy)),
        Expression::GroupedExpression(g) => negated_call(&g.get_expression()?, truthy),
        Expression::UnaryExpression(u) if u.kind() == Operator::Not => negated_call(u.get_terms().first()?, !truthy),
        _ => None,
    }
}
