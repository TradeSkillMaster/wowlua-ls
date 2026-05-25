//! Query layer: extracts simplified, stable views from `AnalysisResult`.
//!
//! This is the stable API boundary between the plugin system and the internal IR.
//! Internal refactors should only require changes here, not in bridge.rs or plugins.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::analysis::AnalysisResult;
use crate::ast::Operator;
use crate::pre_globals::EventPayload;
use crate::types::*;

/// A snapshot of the analysis state needed by plugins.
/// Holds everything plugins might query, decoupled from the full `AnalysisResult` lifetime.
pub(super) struct AnalysisSnapshot {
    pub(super) symbols: Vec<Symbol>,
    pub(super) functions: Vec<Function>,
    pub(super) tables: Vec<TableInfo>,
    pub(super) exprs: Vec<Expr>,
    #[allow(dead_code)] // reserved for future scope-walking queries
    pub(super) scopes: Vec<Scope>,
    pub(super) field_assignments: Vec<FieldAssignment>,
    pub(super) string_literals: HashMap<ExprId, String>,
    pub(super) number_literals: HashMap<ExprId, String>,
    pub(super) event_types: HashMap<String, HashMap<String, EventPayload>>,
    pub(super) event_locations: HashMap<String, HashMap<String, ExternalLocation>>,
}

impl AnalysisSnapshot {
    pub(super) fn from_result(analysis: &AnalysisResult) -> Self {
        AnalysisSnapshot {
            symbols: analysis.ir.symbols.clone(),
            functions: analysis.ir.functions.clone(),
            tables: analysis.ir.tables.clone(),
            exprs: analysis.ir.exprs.clone(),
            scopes: analysis.ir.scopes.clone(),
            field_assignments: analysis.ir.field_assignments.clone(),
            string_literals: analysis.ir.string_literals.clone(),
            number_literals: analysis.ir.number_literals.clone(),
            event_types: analysis.ir.ext.event_types.clone(),
            event_locations: analysis.ir.ext.event_locations.clone(),
        }
    }

    fn sym_name(&self, idx: SymbolIndex) -> Option<&str> {
        let sym = self.symbols.get(idx.val())?;
        match &sym.id {
            SymbolIdentifier::Name(n) => Some(n.as_str()),
            _ => None,
        }
    }
}

// ── Query result types ──────────────────────────────────────────────────────────

/// A local variable found by `find_locals`.
pub(super) struct VariableInfo {
    pub(super) sym_idx: SymbolIndex,
    pub(super) name: String,
    pub(super) def_start: u32,
    pub(super) def_end: u32,
    pub(super) init_expr: Option<ExprId>,
}

/// Info about a table constructor field from the initializer.
pub(super) struct InitFieldInfo {
    pub(super) name: String,
    pub(super) range_start: u32,
    pub(super) range_end: u32,
    pub(super) value_kind: InitValueKind,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum InitValueKind {
    Nil,
    Function,
    Table,
    Number,
    String,
    Boolean,
    Expr,
}

impl InitValueKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Function => "function",
            Self::Table => "table",
            Self::Number => "number",
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::Expr => "expr",
        }
    }
}

/// A field access (read or write) on a variable.
pub(super) struct FieldAccessInfo {
    pub(super) field_name: String,
    pub(super) range_start: u32,
    pub(super) range_end: u32,
}

/// A method call on a variable.
pub(super) struct MethodCallInfo {
    pub(super) method_name: String,
    pub(super) range_start: u32,
    pub(super) range_end: u32,
    pub(super) arg_exprs: Vec<ExprId>,
    pub(super) arg_ranges: Vec<(u32, u32)>,
}

/// A method definition (colon-style function field assignment) on a variable.
pub(super) struct MethodDefInfo {
    pub(super) method_name: String,
    pub(super) range_start: u32,
    pub(super) range_end: u32,
    pub(super) func_idx: FunctionIndex,
}

/// Literal value from an expression.
#[derive(Debug, Clone)]
pub(super) enum LiteralValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Nil,
}

/// An argument to a method call.
pub(super) struct ArgInfo {
    #[allow(dead_code)] // available for future query extensions
    pub(super) expr_id: ExprId,
    pub(super) range_start: u32,
    pub(super) range_end: u32,
    pub(super) literal: Option<LiteralValue>,
    pub(super) kind: &'static str,
}

/// Info about a function parameter.
pub(super) struct ParamInfo {
    pub(super) name: String,
    pub(super) sym_idx: SymbolIndex,
    pub(super) param_index: usize,
}

/// An equality comparison involving a symbol.
pub(super) struct ComparisonInfo {
    pub(super) literal: Option<LiteralValue>,
    pub(super) range_start: u32,
    pub(super) range_end: u32,
}

/// An event declaration from `@event TypeName "EVENT_NAME"`.
pub(super) struct EventDeclInfo {
    pub(super) type_name: String,
    pub(super) event_name: String,
    pub(super) params: Vec<EventParamInfo>,
    /// Byte range of the declaration site, or `None` for built-in stubs.
    pub(super) range: Option<(u32, u32)>,
    pub(super) source_path: Option<PathBuf>,
}

/// A parameter of an event declaration.
pub(super) struct EventParamInfo {
    pub(super) name: String,
    pub(super) type_name: String,
    pub(super) nilable: bool,
    pub(super) description: Option<String>,
}

/// Info about a call init (for `var = Something:Method(args)` patterns).
pub(super) struct CallInitInfo {
    pub(super) receiver: Option<String>,
    pub(super) method: Option<String>,
    pub(super) arg_exprs: Vec<ExprId>,
    pub(super) arg_ranges: Vec<(u32, u32)>,
}

// ── Query functions ─────────────────────────────────────────────────────────────

/// Find local variables at file scope.
pub(super) fn find_locals(snap: &AnalysisSnapshot, name_filter: Option<&str>, init_filter: Option<&str>) -> Vec<VariableInfo> {
    let mut results = Vec::new();

    // Scope 0 is the implicit file scope. Scope 1 (if exists) is also file-level in some cases.
    // We look for symbols in scope 0 (file-level locals).
    // Actually, in wowlua-ls, scope 0's symbols are globals. Local variables at file level
    // are in scopes created by the file's block. Let's walk all symbols and check if their
    // scope parent chain is short (top-level).
    for (idx, sym) in snap.symbols.iter().enumerate() {
        let sym_idx = SymbolIndex::from(idx);
        let name = match &sym.id {
            SymbolIdentifier::Name(n) => n,
            _ => continue,
        };

        // Apply name filter
        if name_filter.is_some_and(|f| name != f) {
            continue;
        }

        // Check if this is a top-level local: symbol lives in scope 0 (file scope)
        if sym.scope_idx.val() != 0 {
            continue;
        }

        let version = &sym.versions[0];
        let init_expr = version.type_source;

        // Apply init filter
        if let Some(filter) = init_filter {
            let matches = match filter {
                "table" => init_expr.is_some_and(|e| matches!(snap.exprs.get(e.val()), Some(Expr::TableConstructor(_)))),
                "call" => init_expr.is_some_and(|e| matches!(snap.exprs.get(e.val()), Some(Expr::FunctionCall { .. }))),
                "function" => init_expr.is_some_and(|e| matches!(snap.exprs.get(e.val()), Some(Expr::FunctionDef(_)))),
                _ => true,
            };
            if !matches {
                continue;
            }
        }

        results.push(VariableInfo {
            sym_idx,
            name: name.clone(),
            def_start: version.def_node.start,
            def_end: version.def_node.end,
            init_expr,
        });
    }

    results
}

/// Get the kind of an initializer expression.
pub(super) fn init_kind(snap: &AnalysisSnapshot, expr_id: ExprId) -> &'static str {
    match snap.exprs.get(expr_id.val()) {
        Some(Expr::TableConstructor(_)) => "table",
        Some(Expr::FunctionCall { .. }) => "call",
        Some(Expr::FunctionDef(_)) => "function",
        Some(Expr::Literal(_)) => "literal",
        _ => "other",
    }
}

/// Get the fields of a table constructor expression.
pub(super) fn table_fields(snap: &AnalysisSnapshot, expr_id: ExprId) -> Vec<InitFieldInfo> {
    let Some(Expr::TableConstructor(table_idx)) = snap.exprs.get(expr_id.val()) else {
        return Vec::new();
    };
    let Some(table) = snap.tables.get(table_idx.val()) else {
        return Vec::new();
    };

    table.fields.iter().map(|(name, field)| {
        let value_kind = classify_expr(snap, field.expr);
        let (range_start, range_end) = field.def_range.unwrap_or((0, 0));
        InitFieldInfo {
            name: name.clone(),
            range_start,
            range_end,
            value_kind,
        }
    }).collect()
}

fn classify_expr(snap: &AnalysisSnapshot, expr_id: ExprId) -> InitValueKind {
    match snap.exprs.get(expr_id.val()) {
        Some(Expr::Literal(ValueType::Nil)) => InitValueKind::Nil,
        Some(Expr::Literal(ValueType::Number)) => InitValueKind::Number,
        Some(Expr::Literal(ValueType::String(_))) => InitValueKind::String,
        Some(Expr::Literal(ValueType::Boolean(_))) => InitValueKind::Boolean,
        Some(Expr::FunctionDef(_)) => InitValueKind::Function,
        Some(Expr::TableConstructor(_)) => InitValueKind::Table,
        _ => InitValueKind::Expr,
    }
}

/// Extract call init info (receiver, method, args) from a FunctionCall expression.
pub(super) fn call_init_info(snap: &AnalysisSnapshot, expr_id: ExprId) -> Option<CallInitInfo> {
    let Expr::FunctionCall { func, args, arg_ranges, is_method_call, .. } = snap.exprs.get(expr_id.val())? else {
        return None;
    };

    let (receiver, method) = if *is_method_call {
        // Method call: func is FieldAccess { table, field }
        if let Some(Expr::FieldAccess { table, field, .. }) = snap.exprs.get(func.val()) {
            let recv = resolve_name(snap, *table);
            (recv, Some(field.clone()))
        } else {
            (None, None)
        }
    } else {
        // Regular call: func might be FieldAccess (dot call) or SymbolRef
        if let Some(Expr::FieldAccess { table, field, .. }) = snap.exprs.get(func.val()) {
            let recv = resolve_name(snap, *table);
            (recv, Some(field.clone()))
        } else {
            let name = resolve_name(snap, *func);
            (name, None)
        }
    };

    Some(CallInitInfo {
        receiver,
        method,
        arg_exprs: args.clone(),
        arg_ranges: arg_ranges.clone(),
    })
}

/// Resolve an expression to a simple name (for SymbolRef chains).
fn resolve_name(snap: &AnalysisSnapshot, expr_id: ExprId) -> Option<String> {
    match snap.exprs.get(expr_id.val())? {
        Expr::SymbolRef(sym_idx, _) => snap.sym_name(*sym_idx).map(|s| s.to_string()),
        _ => None,
    }
}

/// Find all field reads on a variable (FieldAccess expressions where the table is this symbol).
pub(super) fn field_reads(snap: &AnalysisSnapshot, sym_idx: SymbolIndex) -> Vec<FieldAccessInfo> {
    let mut results = Vec::new();

    for expr in &snap.exprs {
        if let Expr::FieldAccess { table, field, field_range } = expr
            && is_symbol_ref(snap, *table, sym_idx)
        {
            let (start, end) = field_range.unwrap_or((0, 0));
            results.push(FieldAccessInfo {
                field_name: field.clone(),
                range_start: start,
                range_end: end,
            });
        }
    }

    results
}

/// Find all field writes on a variable (from field_assignments).
pub(super) fn field_writes(snap: &AnalysisSnapshot, sym_idx: SymbolIndex) -> Vec<FieldAccessInfo> {
    // Find the table index for this symbol (if it has one).
    let table_idx = symbol_table(snap, sym_idx);

    snap.field_assignments.iter().filter_map(|fa| {
        // Match by table index or root name
        let matches = table_idx.is_some_and(|t| t == fa.table_idx)
            || snap.sym_name(sym_idx).is_some_and(|name| name == fa.root_name);
        if !matches { return None; }

        Some(FieldAccessInfo {
            field_name: fa.field_name.clone(),
            range_start: fa.ident_start,
            range_end: fa.ident_end,
        })
    }).collect()
}

/// Find the table index associated with a symbol's first version.
fn symbol_table(snap: &AnalysisSnapshot, sym_idx: SymbolIndex) -> Option<TableIndex> {
    let sym = snap.symbols.get(sym_idx.val())?;
    let expr_id = sym.versions[0].type_source?;
    match snap.exprs.get(expr_id.val())? {
        Expr::TableConstructor(t) => Some(*t),
        _ => None,
    }
}

/// Check if an expression is a SymbolRef to the given symbol.
fn is_symbol_ref(snap: &AnalysisSnapshot, expr_id: ExprId, target: SymbolIndex) -> bool {
    matches!(snap.exprs.get(expr_id.val()), Some(Expr::SymbolRef(s, _)) if *s == target)
}

/// Find method/function calls on a variable (both colon-style `var:method()` and dot-style `var.func()`).
///
/// Intentionally matches all `FunctionCall` expressions on this symbol's fields,
/// regardless of `is_method_call`, so plugins see both calling conventions.
pub(super) fn method_calls(snap: &AnalysisSnapshot, sym_idx: SymbolIndex) -> Vec<MethodCallInfo> {
    let mut results = Vec::new();

    for expr in &snap.exprs {
        if let Expr::FunctionCall { func, args, arg_ranges, call_range, .. } = expr
            && let Some(Expr::FieldAccess { table, field, .. }) = snap.exprs.get(func.val())
            && is_symbol_ref(snap, *table, sym_idx)
        {
            results.push(MethodCallInfo {
                method_name: field.clone(),
                range_start: call_range.0,
                range_end: call_range.1,
                arg_exprs: args.clone(),
                arg_ranges: arg_ranges.clone(),
            });
        }
    }

    results
}

/// Find method definitions (function field assignments) on a variable.
pub(super) fn method_defs(snap: &AnalysisSnapshot, sym_idx: SymbolIndex) -> Vec<MethodDefInfo> {
    let table_idx = symbol_table(snap, sym_idx);
    let mut results = Vec::new();

    for fa in &snap.field_assignments {
        let matches = table_idx.is_some_and(|t| t == fa.table_idx)
            || snap.sym_name(sym_idx).is_some_and(|name| name == fa.root_name);
        if !matches { continue; }

        // Check if the assigned value is a function definition
        if let Some(Expr::FunctionDef(func_idx)) = snap.exprs.get(fa.actual_expr.val()) {
            results.push(MethodDefInfo {
                method_name: fa.field_name.clone(),
                range_start: fa.ident_start,
                range_end: fa.ident_end,
                func_idx: *func_idx,
            });
        }
    }

    results
}

/// Get the parameters of a function.
pub(super) fn function_params(snap: &AnalysisSnapshot, func_idx: FunctionIndex) -> Vec<ParamInfo> {
    let Some(func) = snap.functions.get(func_idx.val()) else {
        return Vec::new();
    };

    func.args.iter().enumerate().filter_map(|(i, &sym_idx)| {
        let name = snap.sym_name(sym_idx)?.to_string();
        Some(ParamInfo {
            name,
            sym_idx,
            param_index: i,
        })
    }).collect()
}

/// Find equality comparisons (== or ~=) involving a symbol.
pub(super) fn symbol_comparisons(snap: &AnalysisSnapshot, sym_idx: SymbolIndex) -> Vec<ComparisonInfo> {
    let mut results = Vec::new();

    for expr in &snap.exprs {
        if let Expr::BinaryOp { op, lhs, rhs } = expr {
            if *op != Operator::Equals && *op != Operator::NotEquals {
                continue;
            }

            let (_is_lhs, other) = if is_symbol_ref(snap, *lhs, sym_idx) {
                (true, *rhs)
            } else if is_symbol_ref(snap, *rhs, sym_idx) {
                (false, *lhs)
            } else {
                continue;
            };

            let literal = extract_literal(snap, other);
            // We don't have the expression range directly in the Expr enum,
            // so use the operands' ranges as a best-effort for the comparison site.
            let (range_start, range_end) = expr_range_from_operands(snap, *lhs, *rhs);

            results.push(ComparisonInfo {
                literal,
                range_start,
                range_end,
            });
        }
    }

    results
}

fn expr_range_from_operands(snap: &AnalysisSnapshot, lhs: ExprId, rhs: ExprId) -> (u32, u32) {
    // Try to get ranges from field accesses or symbol refs
    let start = expr_start(snap, lhs).unwrap_or(0);
    let end = expr_end(snap, rhs).unwrap_or(0);
    (start, end)
}

fn expr_start(snap: &AnalysisSnapshot, expr_id: ExprId) -> Option<u32> {
    match snap.exprs.get(expr_id.val())? {
        Expr::SymbolRef(sym_idx, _) => {
            let sym = snap.symbols.get(sym_idx.val())?;
            Some(sym.versions[0].def_node.start)
        }
        Expr::FieldAccess { field_range, .. } => field_range.map(|(s, _)| s),
        Expr::Literal(_) => None, // literals don't carry ranges in the IR
        _ => None,
    }
}

fn expr_end(snap: &AnalysisSnapshot, expr_id: ExprId) -> Option<u32> {
    match snap.exprs.get(expr_id.val())? {
        Expr::SymbolRef(sym_idx, _) => {
            let sym = snap.symbols.get(sym_idx.val())?;
            Some(sym.versions[0].def_node.end)
        }
        Expr::FieldAccess { field_range, .. } => field_range.map(|(_, e)| e),
        Expr::Literal(_) => None,
        _ => None,
    }
}

/// Extract a literal value from an expression.
pub(super) fn extract_literal(snap: &AnalysisSnapshot, expr_id: ExprId) -> Option<LiteralValue> {
    match snap.exprs.get(expr_id.val())? {
        Expr::Literal(ValueType::Nil) => Some(LiteralValue::Nil),
        Expr::Literal(ValueType::Boolean(b)) => Some(LiteralValue::Boolean(b.unwrap_or(true))),
        Expr::Literal(ValueType::Number) => {
            let num_str = snap.number_literals.get(&expr_id)?;
            let n: f64 = num_str.parse().ok()?;
            Some(LiteralValue::Number(n))
        }
        Expr::Literal(ValueType::String(_)) => {
            let s = snap.string_literals.get(&expr_id)?;
            Some(LiteralValue::String(s.clone()))
        }
        _ => None,
    }
}

/// Classify an expression for the `arg.kind` field.
pub(super) fn expr_kind(snap: &AnalysisSnapshot, expr_id: ExprId) -> &'static str {
    match snap.exprs.get(expr_id.val()) {
        Some(Expr::Literal(ValueType::String(_))) => "string",
        Some(Expr::Literal(ValueType::Number)) => "number",
        Some(Expr::Literal(ValueType::Boolean(_))) => "boolean",
        Some(Expr::Literal(ValueType::Nil)) => "nil",
        Some(Expr::TableConstructor(_)) => "table",
        Some(Expr::FunctionDef(_)) => "function",
        _ => "other",
    }
}

/// Get argument info for a list of expression IDs with ranges.
pub(super) fn args_info(snap: &AnalysisSnapshot, arg_exprs: &[ExprId], arg_ranges: &[(u32, u32)]) -> Vec<ArgInfo> {
    arg_exprs.iter().zip(arg_ranges.iter()).map(|(expr_id, (start, end))| {
        ArgInfo {
            expr_id: *expr_id,
            range_start: *start,
            range_end: *end,
            literal: extract_literal(snap, *expr_id),
            kind: expr_kind(snap, *expr_id),
        }
    }).collect()
}

/// Find event declarations, optionally filtered by type name.
pub(super) fn find_event_declarations(snap: &AnalysisSnapshot, type_name_filter: Option<&str>) -> Vec<EventDeclInfo> {
    let mut results = Vec::new();

    for (type_name, events) in &snap.event_types {
        if type_name_filter.is_some_and(|f| type_name != f) {
            continue;
        }
        let locations = snap.event_locations.get(type_name);
        for (event_name, payload) in events {
            let location = locations.and_then(|locs| locs.get(event_name));
            results.push(EventDeclInfo {
                type_name: type_name.clone(),
                event_name: event_name.clone(),
                params: payload.params.iter().map(|p| EventParamInfo {
                    name: p.name.clone(),
                    type_name: p.type_name.clone(),
                    nilable: p.nilable,
                    description: p.description.clone(),
                }).collect(),
                range: location.map(|loc| (loc.start, loc.end)),
                source_path: location.map(|loc| loc.path.clone()),
            });
        }
    }

    // Sort for deterministic order
    results.sort_by(|a, b| a.type_name.cmp(&b.type_name).then(a.event_name.cmp(&b.event_name)));
    results
}
