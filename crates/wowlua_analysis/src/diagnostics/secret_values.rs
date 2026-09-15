//! `secret-*` diagnostics: operations that error when tainted (addon) code
//! applies them to a WoW secret value (retail 12.x). Which operations error is
//! decided by `crate::secrets`; narrowing by `issecretvalue`/`canaccessvalue`
//! guards is already reflected in the resolved operand types.

use crate::analysis::AnalysisResult;
use crate::ast::Operator;
use crate::secrets::SecretRule;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, TextSize};
use crate::types::{Expr, ExprId, ScopeIndex, SymbolIdentifier, ValueType};
use super::{unwrap_to_inner_expr, DiagnosticDef, DiagnosticPass, WowDiagnostic};

pub struct SecretValues;

impl DiagnosticPass for SecretValues {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if !analysis.secrets_enabled() {
            return;
        }
        check_operators(analysis, diags);
        check_conditions(analysis, tree, diags);
        check_table_keys(analysis, diags);
        check_arguments(analysis, diags);
    }
}

/// Whether secret values apply to `expr` (at `offset`): not inside a
/// Classic-only flavor guard or a guarded `and` chain.
fn active_at(analysis: &AnalysisResult, expr: ExprId, offset: u32) -> bool {
    let scope = analysis.scope_at_offset(offset).unwrap_or(ScopeIndex(0));
    analysis.active_flavors_for_expr(expr, scope) & crate::flavor::SECRET_VALUE_FLAVORS != 0
}

/// Whether `expr` may hold a secret, without cloning its type when it is cached.
fn is_secret(analysis: &AnalysisResult, expr: ExprId) -> bool {
    if let Some(ty) = analysis.resolved_expr_cache_get(expr) {
        return ty.has_secret();
    }
    if let Expr::SymbolRef(sym, ver) = analysis.expr(expr)
        && let Some(ty) = analysis.sym(*sym).versions.get(*ver).and_then(|v| v.resolved_type.as_ref())
    {
        return ty.has_secret();
    }
    analysis.resolve_expr_type(expr).is_some_and(|ty| ty.has_secret())
}

/// "value from `UnitHealth` may be secret".
fn describe(analysis: &AnalysisResult, expr: ExprId, noun: &str) -> String {
    match secret_origin(analysis, expr, 0) {
        Some(origin) => format!("{noun} from `{origin}` may be secret"),
        None => format!("this {noun} may be secret"),
    }
}

/// Emit `def` over `range` as "<describe>; <action>".
fn report(
    analysis: &AnalysisResult,
    diags: &mut Vec<WowDiagnostic>,
    def: &DiagnosticDef,
    culprit: ExprId,
    noun: &str,
    action: &str,
    (start, end): (u32, u32),
) {
    def.emit(diags, format!("{}; {action}", describe(analysis, culprit, noun)), start as usize, end as usize);
}

/// An arithmetic, comparison, or negation that `crate::secrets` says errors
/// because an operand may be secret: that operand, the diagnostic, and what
/// to say about it.
fn erroring_secret_op(analysis: &AnalysisResult, expr: ExprId) -> Option<(ExprId, &'static DiagnosticDef, &'static str)> {
    match analysis.expr(unwrap_to_inner_expr(&analysis.ir, expr)) {
        Expr::BinaryOp { op, lhs, rhs } if op.is_arithmetic() || op.is_comparison() => {
            let (lt, rt) = (analysis.resolve_expr_type(*lhs)?, analysis.resolve_expr_type(*rhs)?);
            if (!lt.has_secret() && !rt.has_secret()) || crate::secrets::binary_op_rule(*op, &lt, &rt) != SecretRule::Error {
                return None;
            }
            let culprit = if rt.has_secret() && !lt.has_secret() { *rhs } else { *lhs };
            Some(if op.is_comparison() {
                (culprit, &super::SECRET_COMPARISON, "comparing it errors in addon code; guard with `canaccessvalue`")
            } else {
                (culprit, &super::SECRET_ARITHMETIC, "arithmetic on it errors in addon code; guard with `canaccessvalue` or pass it to a widget")
            })
        }
        Expr::UnaryOp { op: Operator::Subtract, operand }
            if crate::secrets::NEGATE == SecretRule::Error && is_secret(analysis, *operand) =>
        {
            Some((*operand, &super::SECRET_ARITHMETIC, "negating it errors in addon code; guard with `canaccessvalue` or pass it to a widget"))
        }
        _ => None,
    }
}

fn check_operators(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    let binary = analysis.ir.binary_op_sites.iter().map(|s| (s.expr_id, s.expr_start, s.expr_end));
    let unary = analysis.ir.unary_op_sites.iter().copied();
    for (expr, start, end) in binary.chain(unary) {
        let Some((culprit, def, action)) = erroring_secret_op(analysis, expr) else { continue };
        // One report per root cause: an operand that is itself an erroring
        // secret operation was already reported at its own site.
        if !active_at(analysis, expr, start) || erroring_secret_op(analysis, culprit).is_some() {
            continue;
        }
        report(analysis, diags, def, culprit, "value", action, (start, end));
    }
}

/// Every truth test Lua forces: a condition, `not`, and the left operand of
/// `and`/`or`. Testing an `and`/`or` expression tests its right operand (the
/// operator tests its left one), so each operand of a chain is checked once.
fn check_conditions(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    let conditions = analysis.ir.condition_sites.iter().map(|s| (s.expr_id, (s.start, s.end)));
    let nots = analysis.ir.unary_op_sites.iter().filter_map(|&(expr_id, start, end)| match *analysis.expr(expr_id) {
        Expr::UnaryOp { op: Operator::Not, operand } => Some((operand, (start, end))),
        _ => None,
    });
    let left_operands = analysis.ir.binary_op_sites.iter().filter_map(|site| match *analysis.expr(site.expr_id) {
        Expr::BinaryOp { op: Operator::And | Operator::Or, lhs, .. } => Some((lhs, (site.expr_start, site.op_start))),
        _ => None,
    });
    for (tested, range) in conditions.chain(nots).chain(left_operands) {
        check_truth_test(analysis, tree, tested, range, diags);
    }
}

/// The operand a truth test of `expr` tests — through `and`/`or` to the right
/// operand — and the innermost `and`/`or` it was reached through.
fn tested_operand(analysis: &AnalysisResult, expr: ExprId) -> (ExprId, Option<ExprId>) {
    let mut expr = unwrap_to_inner_expr(&analysis.ir, expr);
    let mut via = None;
    while let Expr::BinaryOp { op: Operator::And | Operator::Or, rhs, .. } = *analysis.expr(expr) {
        via = Some(expr);
        expr = unwrap_to_inner_expr(&analysis.ir, rhs);
    }
    (expr, via)
}

fn check_truth_test(analysis: &AnalysisResult, tree: &SyntaxTree, expr: ExprId, fallback: (u32, u32), diags: &mut Vec<WowDiagnostic>) {
    let (operand, via) = tested_operand(analysis, expr);
    if !is_secret(analysis, operand) {
        return;
    }
    let Some(ty) = analysis.resolve_expr_type(operand) else { return };
    if crate::secrets::truth_test_rule(&ty) != SecretRule::Error || !active_at(analysis, operand, fallback.0) {
        return;
    }
    // A right operand without a range of its own spans from after its operator.
    let range = expr_range(analysis, operand)
        .or_else(|| {
            let site = analysis.ir.binary_op_sites.iter().find(|s| Some(s.expr_id) == via)?;
            Some((skip_trivia(tree, site.op_end), site.expr_end))
        })
        .unwrap_or(fallback);
    report(
        analysis, diags, &super::SECRET_CONDITION, operand, "boolean",
        "testing it errors in addon code; guard with `canaccessvalue` or use a `...FromBoolean` API",
        range,
    );
}

/// The first offset at or after `offset` that isn't whitespace or a comment.
fn skip_trivia(tree: &SyntaxTree, mut offset: u32) -> u32 {
    let root = SyntaxNode::new_root(tree);
    while let Some(token) = root.token_at_offset(TextSize::from(offset)).right_biased()
        && token.kind().is_trivia()
    {
        offset = u32::from(token.text_range().end());
    }
    offset
}

/// Best-effort source range of an expression: a binary-op site, call, or field access.
fn expr_range(analysis: &AnalysisResult, expr: ExprId) -> Option<(u32, u32)> {
    let expr = unwrap_to_inner_expr(&analysis.ir, expr);
    if let Some(site) = analysis.ir.binary_op_sites.iter().find(|s| s.expr_id == expr) {
        return Some((site.expr_start, site.expr_end));
    }
    match analysis.expr(expr) {
        Expr::FunctionCall { call_range, .. } => Some(*call_range),
        Expr::FieldAccess { field_range, .. } => *field_range,
        _ => None,
    }
}

fn check_table_keys(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    // The same bracket can be recorded twice; dedupe only what gets reported.
    let mut seen = std::collections::HashSet::new();
    for &(key, start, end) in &analysis.ir.bracket_index_sites {
        if crate::secrets::TABLE_KEY != SecretRule::Error
            || !is_secret(analysis, key)
            || !active_at(analysis, key, start)
            || erroring_secret_op(analysis, key).is_some()
            || !seen.insert((start, end))
        {
            continue;
        }
        report(
            analysis, diags, &super::SECRET_TABLE_KEY, key, "value",
            "using it as a table key errors in addon code; guard with `canaccessvalue`",
            (start, end),
        );
    }
}

fn check_arguments(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    for (call_expr, cr) in &analysis.ir.call_resolutions {
        let policy = analysis.func(cr.func_idx).secret.as_ref().and_then(|m| m.args);
        if crate::secrets::argument_rule(policy) != SecretRule::Error {
            continue;
        }
        let callee = match analysis.expr(*call_expr) {
            Expr::FunctionCall { func, is_method_call, .. } => callee_name(analysis, *func, *is_method_call),
            _ => None,
        };
        let callee = callee.map_or_else(|| "this function".to_string(), |n| format!("`{n}`"));
        for arg in &cr.expected_args {
            if !is_secret(analysis, arg.arg_expr) || !active_at(analysis, arg.arg_expr, arg.start) {
                continue;
            }
            super::SECRET_ARGUMENT.emit(
                diags,
                format!("{callee} never accepts secret values, but this {}; guard with `canaccessvalue`", describe(analysis, arg.arg_expr, "value")),
                arg.start as usize,
                arg.end as usize,
            );
        }
    }
}

/// Where a secret value came from, for messages: the API call that produced it
/// (`UnitHealth`, `C_Spell.GetSpellCooldown`, `frame:GetText`) or the structure
/// field it was read from.
fn secret_origin(analysis: &AnalysisResult, expr: ExprId, depth: usize) -> Option<String> {
    if depth > 16 {
        return None;
    }
    match analysis.expr(unwrap_to_inner_expr(&analysis.ir, expr)) {
        Expr::CastAdd(inner, _) | Expr::CastRemove(inner, _) | Expr::TypeFilter(inner, _) => {
            secret_origin(analysis, *inner, depth + 1)
        }
        Expr::SymbolRef(sym, ver) => {
            let symbol = analysis.sym(*sym);
            let from_source = symbol.versions.get(*ver)
                .and_then(|v| v.type_source)
                .filter(|_| !sym.is_external())
                .and_then(|source| secret_origin(analysis, source, depth + 1));
            // A value typed by annotation (or with an untraceable source) is named
            // after its variable.
            from_source.or_else(|| match &symbol.id {
                SymbolIdentifier::Name(name) => Some(name.clone()),
                _ => None,
            })
        }
        Expr::BranchMerge(branches) => branches.iter()
            .find(|b| is_secret(analysis, **b))
            .and_then(|b| secret_origin(analysis, *b, depth + 1)),
        Expr::FunctionCall { func, is_method_call, .. } => callee_name(analysis, *func, *is_method_call),
        Expr::FieldAccess { table, field, .. } => {
            let class = match analysis.resolve_expr_type(*table)?.into_strip_opaque() {
                ValueType::Table(Some(idx)) => analysis.table(idx).class_name.clone(),
                _ => None,
            };
            Some(match class {
                Some(class) => format!("{class}.{field}"),
                None => field.clone(),
            })
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            let secret_side = if is_secret(analysis, *lhs) { *lhs } else { *rhs };
            secret_origin(analysis, secret_side, depth + 1)
        }
        Expr::UnaryOp { operand, .. } => secret_origin(analysis, *operand, depth + 1),
        _ => None,
    }
}

/// `UnitHealth`, `C_Spell.GetSpellCooldown`, or `bar:GetValue` from a callee expression.
fn callee_name(analysis: &AnalysisResult, func: ExprId, is_method_call: bool) -> Option<String> {
    let func = unwrap_to_inner_expr(&analysis.ir, func);
    if let Expr::SymbolRef(sym, _) = analysis.expr(func) {
        return match &analysis.sym(*sym).id {
            SymbolIdentifier::Name(name) => Some(name.clone()),
            _ => None,
        };
    }
    let (root, chain) = analysis.ir.extract_field_chain(func)?;
    let SymbolIdentifier::Name(root_name) = &analysis.sym(root).id else { return None };
    let (last, init) = chain.split_last()?;
    let sep = if is_method_call { ":" } else { "." };
    let mut name = root_name.clone();
    for part in init {
        name.push('.');
        name.push_str(part);
    }
    Some(format!("{name}{sep}{last}"))
}
