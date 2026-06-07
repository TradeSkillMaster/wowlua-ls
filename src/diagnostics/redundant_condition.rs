use crate::analysis::{AnalysisResult, ancestor_scopes};
use crate::analysis::resolve::parse_num_literal_str;
use crate::ast::Operator;
use crate::types::{EnumKind, Expr, ExprId, ScopeIndex, SymbolIndex, SymbolIdentifier, ValueType};
use super::{DiagnosticPass, WowDiagnostic, is_type_permissive, is_expr_truthiness_uncertain, types_disjoint, unwrap_to_inner_expr};

pub(crate) struct RedundantCondition;

/// The eight strings Lua's `type()` can return.
const LUA_TYPE_NAMES: [&str; 8] =
    ["nil", "boolean", "number", "string", "table", "function", "userdata", "thread"];

/// Outcome of statically evaluating a condition.
enum Verdict {
    /// The whole expression's type is guaranteed truthy/falsy (carries the type
    /// for the message). Preserves the original wording for bare-value conditions.
    Truthy(ValueType),
    Falsy(ValueType),
    /// `not` of an always-truthy/falsy expression (carries the inner type for
    /// a more informative message like "`not` of always-truthy `table`").
    NegatedTruthy(ValueType),
    NegatedFalsy(ValueType),
    /// A comparison / negation that evaluates to a constant boolean.
    AlwaysTrue,
    AlwaysFalse,
}

impl Verdict {
    fn negate(self) -> Verdict {
        match self {
            Verdict::Truthy(t) => Verdict::NegatedTruthy(t),
            Verdict::Falsy(t) => Verdict::NegatedFalsy(t),
            Verdict::NegatedTruthy(_) | Verdict::AlwaysFalse => Verdict::AlwaysTrue,
            Verdict::NegatedFalsy(_) | Verdict::AlwaysTrue => Verdict::AlwaysFalse,
        }
    }

    fn message(&self, analysis: &AnalysisResult) -> String {
        match self {
            Verdict::Truthy(t) => {
                format!("condition is always truthy (`{}`)", analysis.format_type_depth(t, 1))
            }
            Verdict::Falsy(t) => {
                format!("condition is always falsy (`{}`)", analysis.format_type_depth(t, 1))
            }
            Verdict::NegatedTruthy(t) => {
                format!("condition is always false (`not` of always-truthy `{}`)", analysis.format_type_depth(t, 1))
            }
            Verdict::NegatedFalsy(t) => {
                format!("condition is always true (`not` of always-falsy `{}`)", analysis.format_type_depth(t, 1))
            }
            Verdict::AlwaysTrue => "condition is always true".to_string(),
            Verdict::AlwaysFalse => "condition is always false".to_string(),
        }
    }
}

impl DiagnosticPass for RedundantCondition {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for site in &analysis.ir.condition_sites {
            // Skip boolean literals in loop conditions (`while true do`,
            // `repeat...until false`) — these are standard infinite-loop idioms.
            // Non-loop contexts (`if true`, `if false`, `elseif true`) are still
            // flagged as they typically indicate dead code or a forgotten condition.
            if site.is_loop && matches!(analysis.ir.expr(site.expr_id), Expr::Literal(ValueType::Boolean(Some(_)))) {
                continue;
            }

            let Some(verdict) = eval_condition_constant(analysis, site.expr_id) else { continue };

            // Suppress when the condition references a variable whose value
            // is uncertain due to reassignment inside a loop or conditional
            // block — the static type may not reflect all possible runtime
            // values.
            if has_uncertain_reassignment(&analysis.ir, site.expr_id, site.start, site.loop_scope) {
                continue;
            }

            super::REDUNDANT_CONDITION.emit(
                diags,
                verdict.message(analysis),
                site.start as usize,
                site.end as usize,
            );
        }
    }
}

/// Try to prove a condition expression is constant. Returns `None` when the
/// condition is not provably always-true or always-false. Folds in the
/// permissive / truthiness-uncertainty FP guards at the leaves.
fn eval_condition_constant(analysis: &AnalysisResult, expr_id: ExprId) -> Option<Verdict> {
    let ir = &analysis.ir;
    let inner = unwrap_to_inner_expr(&ir.exprs, expr_id);
    match ir.expr(inner) {
        // `not <expr>`: evaluate the operand and flip the verdict. Handles
        // `if not t` (t always truthy → always false) and `if not (x == nil)`.
        Expr::UnaryOp { op: Operator::Not, operand } => {
            return eval_condition_constant(analysis, *operand).map(Verdict::negate);
        }
        Expr::BinaryOp { op, lhs, rhs } if op.is_comparison() => {
            if let Some(v) = eval_comparison(analysis, *op, *lhs, *rhs) {
                return Some(v);
            }
        }
        _ => {}
    }
    // Fallthrough: the whole expression's type is wholly truthy/falsy.
    eval_type_truthiness(analysis, expr_id)
}

/// Original behavior: the resolved type of the expression is guaranteed
/// truthy or guaranteed falsy.
fn eval_type_truthiness(analysis: &AnalysisResult, expr_id: ExprId) -> Option<Verdict> {
    let cond_type = analysis.resolve_expr_type(expr_id)?;
    if is_type_permissive(&cond_type) { return None; }
    // Skip expressions whose truthiness can't be reliably determined from
    // static types (lateinit fields, unannotated fields, dynamic indices,
    // unannotated parameters).
    if is_expr_truthiness_uncertain(analysis, expr_id) { return None; }
    if cond_type.is_guaranteed_truthy() {
        Some(Verdict::Truthy(cond_type))
    } else if cond_type.is_guaranteed_falsy() {
        Some(Verdict::Falsy(cond_type))
    } else {
        None
    }
}

fn eval_comparison(analysis: &AnalysisResult, op: Operator, lhs: ExprId, rhs: ExprId) -> Option<Verdict> {
    match op {
        Operator::Equals | Operator::NotEquals => eval_equality(analysis, op, lhs, rhs),
        Operator::LessThan | Operator::GreaterThan
        | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals => eval_ordered(analysis, op, lhs, rhs),
        _ => None,
    }
}

/// Evaluate `==` / `~=` between two operands.
fn eval_equality(analysis: &AnalysisResult, op: Operator, lhs: ExprId, rhs: ExprId) -> Option<Verdict> {
    // Redundant `type(x) == "..."` guard.
    if let Some(v) = eval_type_guard(analysis, op, lhs, rhs) {
        return Some(v);
    }

    // Don't trust operands whose static type may diverge from runtime reality.
    if is_expr_truthiness_uncertain(analysis, lhs) || is_expr_truthiness_uncertain(analysis, rhs) {
        return None;
    }

    let lt = resolve_enum_runtime_type(analysis, effective_type(analysis, lhs)?);
    let rt = resolve_enum_runtime_type(analysis, effective_type(analysis, rhs)?);
    if is_type_permissive(&lt) || is_type_permissive(&rt) { return None; }

    if types_disjoint(&lt, &rt) {
        return Some(verdict_eq(op, false));
    }
    if same_singleton_literal(&lt, &rt) {
        // Negative narrowing may strip an open literal-union param
        // (`@param x "A"|"B"|"C"`) down to a single remaining member along an
        // if/elseif chain, making the final `x == "C"` look "always true". But
        // the annotation is an open contract — the caller could pass an unlisted
        // value — so this comparison is genuinely meaningful and must not be
        // flagged. This applies *only* when the narrowed value came from stripping
        // members of the union, not from a positive assignment/filter to a literal
        // (e.g. `x = "A"; if x == "A"`), which is genuinely redundant.
        if operand_is_stripped_open_union(analysis, lhs) || operand_is_stripped_open_union(analysis, rhs) {
            return None;
        }
        return Some(verdict_eq(op, true));
    }
    None
}

/// True when `expr_id` references an open literal-union symbol *and* the
/// referenced version was produced by stripping members from that union
/// (a `CastRemove` version created by negative narrowing down an if/elseif
/// chain). A positive assignment/filter to a single literal is excluded, since
/// comparing against that literal is genuinely always-true.
fn operand_is_stripped_open_union(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let ir = &analysis.ir;
    let id = unwrap_to_inner_expr(&ir.exprs, expr_id);
    match ir.expr(id) {
        // A direct strip expression (`CastRemove(SymbolRef, ..)`).
        Expr::CastRemove(inner, _) => {
            let inner_id = unwrap_to_inner_expr(&ir.exprs, *inner);
            matches!(ir.expr(inner_id),
                Expr::SymbolRef(s, _) if ir.is_open_literal_union_symbol(*s))
        }
        // A reference to a strip-derived version.
        Expr::SymbolRef(sym_idx, ver) => {
            ir.is_open_literal_union_symbol(*sym_idx)
                && analysis.sym(*sym_idx).versions.get(*ver)
                    .and_then(|v| v.type_source)
                    .is_some_and(|ts| matches!(ir.expr(ts), Expr::CastRemove(..)))
        }
        _ => false,
    }
}

/// Evaluate ordered comparisons (`<` `>` `<=` `>=`).
fn eval_ordered(analysis: &AnalysisResult, op: Operator, lhs: ExprId, rhs: ExprId) -> Option<Verdict> {
    let ir = &analysis.ir;

    // Self-comparison: `x < x` / `x > x` are always false (NaN-safe: `NaN < NaN`
    // and `NaN > NaN` are both false). `<=` / `>=` are intentionally excluded
    // because `NaN <= NaN` is false, so they are not always-true under NaN.
    // This is a purely syntactic check — no type resolution needed.
    if matches!(op, Operator::LessThan | Operator::GreaterThan)
        && exprs_syntactically_equal(ir, lhs, rhs) {
        return Some(Verdict::AlwaysFalse);
    }

    // Two concrete numeric literals: evaluate directly.
    let lt = effective_type(analysis, lhs)?;
    let rt = effective_type(analysis, rhs)?;
    if let (ValueType::NumberLiteral(a), ValueType::NumberLiteral(b)) = (&lt, &rt)
        && let (Some(av), Some(bv)) = (parse_num_literal_str(a), parse_num_literal_str(b)) {
            let result = match op {
                Operator::LessThan => av < bv,
                Operator::GreaterThan => av > bv,
                Operator::LessThanOrEquals => av <= bv,
                Operator::GreaterThanOrEquals => av >= bv,
                _ => return None,
            };
            return Some(if result { Verdict::AlwaysTrue } else { Verdict::AlwaysFalse });
        }
    None
}

/// Detect `type(x) == "literal"` (either operand order) where `x`'s static type
/// makes the guard constant.
fn eval_type_guard(analysis: &AnalysisResult, op: Operator, lhs: ExprId, rhs: ExprId) -> Option<Verdict> {
    let ir = &analysis.ir;
    let (arg, type_name) = match (type_call_arg(analysis, lhs), string_literal_value(ir, rhs)) {
        (Some(a), Some(n)) => (a, n),
        _ => match (type_call_arg(analysis, rhs), string_literal_value(ir, lhs)) {
            (Some(a), Some(n)) => (a, n),
            _ => return None,
        },
    };
    if !LUA_TYPE_NAMES.contains(&type_name.as_str()) { return None; }
    if is_expr_truthiness_uncertain(analysis, arg) { return None; }
    let arg_type = resolve_enum_runtime_type(analysis, analysis.resolve_expr_type(arg)?);
    if is_type_permissive(&arg_type) { return None; }
    let kinds = possible_type_kinds(&arg_type)?;
    if kinds.is_empty() { return None; }

    let contains = kinds.iter().any(|k| *k == type_name);
    if contains && kinds.iter().all(|k| *k == type_name) {
        // `x` is always this type → guard is always satisfied.
        Some(verdict_eq(op, true))
    } else if !contains {
        // `x` can never be this type → guard is never satisfied.
        Some(verdict_eq(op, false))
    } else {
        // Mixed union: not constant.
        None
    }
}

/// Map a comparison operator and an equality outcome to a verdict.
fn verdict_eq(op: Operator, equal: bool) -> Verdict {
    let cond_true = match op {
        Operator::Equals => equal,
        Operator::NotEquals => !equal,
        _ => return Verdict::AlwaysFalse, // unreachable for callers
    };
    if cond_true { Verdict::AlwaysTrue } else { Verdict::AlwaysFalse }
}

/// Resolve an expression's type, but recover concrete literal values for source
/// literals (which lower to generic `String(None)` / `Number` with the spelling
/// kept in side tables). This lets `"a" == "b"` and `x == "c"` be evaluated.
fn effective_type(analysis: &AnalysisResult, expr_id: ExprId) -> Option<ValueType> {
    let ir = &analysis.ir;
    let id = unwrap_to_inner_expr(&ir.exprs, expr_id);
    if let Expr::Literal(vt) = ir.expr(id) {
        match vt {
            ValueType::String(None) => {
                if let Some(s) = ir.string_literals.get(&id) {
                    return Some(ValueType::String(Some(s.clone())));
                }
            }
            ValueType::Number => {
                if let Some(s) = ir.number_literals.get(&id) {
                    return Some(ValueType::NumberLiteral(s.clone()));
                }
            }
            _ => {}
        }
    }
    analysis.resolve_expr_type(expr_id)
}

/// If `t` is an enum table type, return its runtime base type (`Number` or
/// `String`). Enum classes are tables in the type system but integers/strings
/// at runtime, so equality comparisons against number/string literals should
/// not be considered disjoint.
fn resolve_enum_runtime_type(analysis: &AnalysisResult, t: ValueType) -> ValueType {
    match &t {
        ValueType::Table(Some(idx)) => {
            match analysis.table(*idx).enum_kind {
                EnumKind::Number => ValueType::Number,
                EnumKind::String => ValueType::String(None),
                EnumKind::NotEnum => t,
            }
        }
        ValueType::Union(members) => {
            let has_enum = members.iter().any(|m| matches!(m,
                ValueType::Table(Some(idx)) if analysis.table(*idx).enum_kind != EnumKind::NotEnum));
            if !has_enum { return t; }
            let resolved: Vec<_> = members.iter().map(|m| {
                resolve_enum_runtime_type(analysis, m.clone())
            }).collect();
            ValueType::Union(resolved)
        }
        _ => t,
    }
}

/// Two singleton (single-value) literal types that name the same value.
fn same_singleton_literal(a: &ValueType, b: &ValueType) -> bool {
    match (a, b) {
        (ValueType::Nil, ValueType::Nil) => true,
        (ValueType::Boolean(Some(x)), ValueType::Boolean(Some(y))) => x == y,
        (ValueType::String(Some(x)), ValueType::String(Some(y))) => x == y,
        (ValueType::NumberLiteral(x), ValueType::NumberLiteral(y)) => {
            match (parse_num_literal_str(x), parse_num_literal_str(y)) {
                (Some(xv), Some(yv)) => xv == yv,
                _ => x == y,
            }
        }
        _ => false,
    }
}

/// Collect the set of possible `type()` kinds for a type. Returns `None` if any
/// part is permissive (`any`/type-var/intersection) and so could be anything.
fn possible_type_kinds(t: &ValueType) -> Option<Vec<&'static str>> {
    fn collect(t: &ValueType, out: &mut Vec<&'static str>) -> bool {
        match t {
            ValueType::Nil => out.push("nil"),
            ValueType::Boolean(_) => out.push("boolean"),
            ValueType::Number | ValueType::NumberLiteral(_) => out.push("number"),
            ValueType::String(_) => out.push("string"),
            ValueType::Table(_) => out.push("table"),
            ValueType::Function(_) => out.push("function"),
            ValueType::Userdata => out.push("userdata"),
            ValueType::Thread => out.push("thread"),
            ValueType::OpaqueAlias(_, inner) => return collect(inner, out),
            ValueType::Union(members) => {
                for m in members {
                    if !collect(m, out) { return false; }
                }
            }
            ValueType::Any | ValueType::TypeVariable(_) | ValueType::Intersection(_) => return false,
        }
        true
    }
    let mut out = Vec::new();
    if collect(t, &mut out) { Some(out) } else { None }
}

/// If `expr_id` is a single-argument call to the global `type`, return the
/// argument expression.
fn type_call_arg(analysis: &AnalysisResult, expr_id: ExprId) -> Option<ExprId> {
    let ir = &analysis.ir;
    let id = unwrap_to_inner_expr(&ir.exprs, expr_id);
    let Expr::FunctionCall { func, args, .. } = ir.expr(id) else { return None };
    if args.len() != 1 { return None; }
    let arg = args[0];
    let f = unwrap_to_inner_expr(&ir.exprs, *func);
    let Expr::SymbolRef(sym_idx, _) = ir.expr(f) else { return None };
    match &analysis.sym(*sym_idx).id {
        SymbolIdentifier::Name(n) if n == "type" && sym_idx.is_external() => Some(arg),
        _ => None,
    }
}

/// If `expr_id` is a string literal, return its (delimiter-stripped) value.
fn string_literal_value(ir: &crate::analysis::Ir, expr_id: ExprId) -> Option<String> {
    let id = unwrap_to_inner_expr(&ir.exprs, expr_id);
    if matches!(ir.expr(id), Expr::Literal(ValueType::String(_))) {
        ir.string_literals.get(&id).cloned()
    } else {
        None
    }
}

/// Conservative syntactic equality for self-comparison: same symbol, or the
/// same field-access chain.
fn exprs_syntactically_equal(ir: &crate::analysis::Ir, a: ExprId, b: ExprId) -> bool {
    let a = unwrap_to_inner_expr(&ir.exprs, a);
    let b = unwrap_to_inner_expr(&ir.exprs, b);
    match (ir.expr(a), ir.expr(b)) {
        // Version index intentionally ignored: two references to the same
        // variable in `x < x` share the same version at the comparison site,
        // and even if narrowing created different versions they refer to the
        // same runtime value.
        (Expr::SymbolRef(sa, _), Expr::SymbolRef(sb, _)) => sa == sb,
        (
            Expr::FieldAccess { table: ta, field: fa, .. },
            Expr::FieldAccess { table: tb, field: fb, .. },
        ) => fa == fb && exprs_syntactically_equal(ir, *ta, *tb),
        _ => false,
    }
}

/// Collect all `SymbolRef`s (with version indices) reachable from an
/// expression by unwrapping narrowing wrappers, `not`, `and`/`or`, comparison
/// operands, and function-call arguments (so `type(x) == "..."` is reached).
fn collect_symbol_refs(ir: &crate::analysis::Ir, expr_id: ExprId, out: &mut Vec<(SymbolIndex, usize)>) {
    match ir.expr(expr_id) {
        Expr::SymbolRef(sym_idx, ver_idx) => out.push((*sym_idx, *ver_idx)),
        Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::StripTruthy(inner)
        | Expr::Grouped(inner) => collect_symbol_refs(ir, *inner, out),
        Expr::UnaryOp { op: Operator::Not, operand } => collect_symbol_refs(ir, *operand, out),
        Expr::BinaryOp {
            op: Operator::And | Operator::Or
            | Operator::Equals | Operator::NotEquals
            | Operator::LessThan | Operator::GreaterThan
            | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals,
            lhs, rhs,
        } => {
            let (lhs, rhs) = (*lhs, *rhs);
            collect_symbol_refs(ir, lhs, out);
            collect_symbol_refs(ir, rhs, out);
        }
        Expr::FunctionCall { args, .. } => {
            for &arg in args {
                collect_symbol_refs(ir, arg, out);
            }
        }
        _ => {}
    }
}

/// Check whether the condition references a variable whose value is uncertain
/// due to reassignment inside a loop body or a conditional block.  Covers:
///
/// - Case 1: condition is inside a loop that reassigns the variable
/// - Case 2: condition follows a loop that reassigned the variable
/// - Case 3: condition depends (one level) on a loop-reassigned variable
/// - Case 4: condition follows a conditional block that reassigned the variable
fn has_uncertain_reassignment(
    ir: &crate::analysis::Ir,
    expr_id: ExprId,
    offset: u32,
    loop_scope_hint: Option<ScopeIndex>,
) -> bool {
    let mut sym_refs = Vec::new();
    collect_symbol_refs(ir, expr_id, &mut sym_refs);
    if sym_refs.is_empty() { return false; }

    let local_syms: Vec<_> = sym_refs.iter().filter_map(|&(idx, _)| {
        if idx.is_external() { return None; }
        Some((idx, ir.sym(idx)))
    }).collect();

    let cond_scope = ir.scope_at_offset(offset);

    // Case 1: condition is inside a loop (or in while/repeat...until position
    // where ancestor-walking won't find the loop body — use the stored hint).
    let enclosing_loop = loop_scope_hint.or_else(|| {
        find_enclosing_loop(ir, cond_scope?)
    });

    if enclosing_loop.is_some_and(|loop_scope| {
        local_syms.iter().any(|(_, sym)| {
            sym.versions.iter().any(|ver| {
                is_scope_inside(ir, ver.created_in_scope, loop_scope)
            })
        })
    }) {
        return true;
    }

    // Case 2: condition is after a preceding loop. A variable defined before
    // the loop but reassigned inside it may hold either its pre-loop or
    // in-loop value. Only suppress when the loop ends before the condition.
    if local_syms.iter().any(|(_, sym)| {
        sym.versions.iter().any(|ver| {
            // Find the innermost loop enclosing this version's creation scope.
            let Some(ver_loop) = find_enclosing_loop(ir, ver.created_in_scope) else { return false };
            // Only suppress when the symbol was defined outside that loop.
            if is_scope_inside(ir, sym.scope_idx, ver_loop) { return false; }
            // Only suppress when the loop precedes the condition — a loop
            // appearing after the condition cannot affect the condition's value.
            let Some(&(_, loop_end, _)) = ir.block_scopes.iter().find(|&&(_, _, s)| s == ver_loop) else { return false };
            loop_end <= offset
        })
    }) {
        return true;
    }

    // Case 3: transitive — the condition references a variable whose defining
    // expression depends on a loop-reassigned variable (one level deep).
    // Handles patterns like `local result = expr and loopVar or nil`.
    if sym_refs.iter().any(|&(sym_idx, ver_idx)| {
        sym_def_has_loop_reassigned_dep(ir, sym_idx, ver_idx, offset)
    }) {
        return true;
    }

    // Case 4: condition is after a preceding conditional (non-loop) block.
    // A variable defined before the block but reassigned inside one branch
    // may hold its pre-block or in-block value, so the condition is not
    // redundant.  The narrowing system may propagate the in-block type to
    // the post-block scope, masking the fact that the assignment was
    // conditional.  Example:
    //   local x = getValue()
    //   if not x then
    //       if other then x = fallback() end
    //   end
    //   if x then ...   -- not redundant: x could still be nil
    //
    // To avoid over-suppression (e.g. `local x = "a"; if c then x = "b" end;
    // if x then`), we check whether all versions agree on truthiness — if they
    // do, the conditional reassignment can't change the verdict.
    if let Some(cs) = cond_scope
        && local_syms.iter().any(|(_, sym)| {
            let has_conditional_version = sym.versions.iter().any(|ver| {
                // Skip versions in the same scope or an ancestor scope of the
                // condition — those are unconditionally visible.
                if is_scope_inside(ir, cs, ver.created_in_scope) { return false; }
                // The symbol must be defined outside the version's scope.
                if is_scope_inside(ir, sym.scope_idx, ver.created_in_scope) { return false; }
                // The version's scope must have a valid block range and end
                // before the condition.
                let Some(&(_, scope_end, _)) = ir.block_scopes.iter()
                    .find(|&&(start, end, s)| s == ver.created_in_scope && end > start) else { return false };
                scope_end <= offset
            });
            if !has_conditional_version { return false; }

            // If all versions agree on truthiness (all guaranteed-truthy or
            // all guaranteed-falsy), the conditional reassignment cannot
            // change the verdict — don't suppress.
            let mut all_truthy = true;
            let mut all_falsy = true;
            let mut any_uncertain = false;
            for v in &sym.versions {
                match &v.resolved_type {
                    Some(t) if !is_type_permissive(t) => {
                        if !t.is_guaranteed_truthy() { all_truthy = false; }
                        if !t.is_guaranteed_falsy() { all_falsy = false; }
                    }
                    _ => { any_uncertain = true; }
                }
            }
            if !any_uncertain && (all_truthy || all_falsy) { return false; }
            true
        })
    {
        return true;
    }

    false
}

/// Find the nearest ancestor scope (inclusive) that is a loop body.
fn find_enclosing_loop(ir: &crate::analysis::Ir, scope: ScopeIndex) -> Option<ScopeIndex> {
    ancestor_scopes(&ir.scopes, scope).find(|&s| ir.scopes[s.val()].is_loop)
}

/// Returns true if `scope` is `container` or a descendant of `container`.
fn is_scope_inside(ir: &crate::analysis::Ir, scope: ScopeIndex, container: ScopeIndex) -> bool {
    ancestor_scopes(&ir.scopes, scope).any(|s| s == container)
}

/// One level of transitive expansion: check if the visible version's defining
/// expression references symbols that have preceding loop versions.  This
/// handles cases like `local result = expr and loopVar or nil`.
///
/// Deeper chains (e.g. `local a = loopVar; local b = a; if b then`) are not
/// followed — the second indirection is a known limitation.
fn sym_def_has_loop_reassigned_dep(
    ir: &crate::analysis::Ir,
    sym_idx: SymbolIndex,
    ver_idx: usize,
    offset: u32,
) -> bool {
    if sym_idx.is_external() { return false; }
    let sym = ir.sym(sym_idx);
    let Some(ver) = sym.versions.get(ver_idx) else { return false };
    let Some(src) = ver.type_source else { return false };

    let mut dep_refs = Vec::new();
    collect_symbol_refs(ir, src, &mut dep_refs);
    dep_refs.iter().any(|&(dep_sym_idx, _)| {
        if dep_sym_idx.is_external() { return false; }
        let dep_sym = ir.sym(dep_sym_idx);
        dep_sym.versions.iter().any(|dep_ver| {
            let Some(ver_loop) = find_enclosing_loop(ir, dep_ver.created_in_scope) else { return false };
            if is_scope_inside(ir, dep_sym.scope_idx, ver_loop) { return false; }
            let Some(&(_, loop_end, _)) = ir.block_scopes.iter().find(|&&(_, _, s)| s == ver_loop) else { return false };
            loop_end <= offset
        })
    })
}
