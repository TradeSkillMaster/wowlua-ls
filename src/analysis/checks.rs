use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken, TextSize};
use crate::types::*;
use super::AnalysisResult;

// ── Deferred Diagnostic Checks ──────────────────────────────────────────────────

impl AnalysisResult {
    /// Run all diagnostic checks against the resolved analysis state.
    pub fn run_diagnostics(
        &self,
        tree: &SyntaxTree,
    ) -> Vec<crate::diagnostics::WowDiagnostic> {
        crate::diagnostics::run_all(self, tree)
    }


    // ── Access diagnostics ──────────────────────────────────────────────────────

    /// Recursively collect Name tokens from an identifier node in left-to-right order.
    /// In parser2's DotAccess tree, names are nested inside child NameRef/DotAccess nodes
    /// rather than being direct children. This function walks the identifier chain to
    /// collect all Name tokens at any depth (for identifier-like nodes only).
    pub(crate) fn collect_name_tokens_recursive<'b>(node: SyntaxNode<'b>) -> Vec<SyntaxToken<'b>> {
        let mut result = Vec::new();
        Self::collect_name_tokens_inner(node, &mut result);
        result
    }

    fn collect_name_tokens_inner<'b>(node: SyntaxNode<'b>, out: &mut Vec<SyntaxToken<'b>>) {
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => {
                    if n.kind().is_identifier()
                        && n.kind() != SyntaxKind::MethodCall
                        && n.kind() != SyntaxKind::FunctionCall
                    {
                        Self::collect_name_tokens_inner(n, out);
                    }
                }
                NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::Name {
                        out.push(t);
                    }
                }
            }
        }
    }

    /// Check if actual table type is a subtype of expected table type (via class inheritance,
    /// structural field matching, or structural array equivalence).
    pub(crate) fn is_table_subtype(&self, actual: &ValueType, expected: &ValueType) -> bool {
        super::is_table_subtype_impl(&self.ir, &self.resolved_expr_cache, actual, expected)
    }

    /// Check if a table literal's fields structurally match a @class type's fields.
    /// Returns true when the literal has all required fields with compatible types.
    fn fields_structurally_match(&self, actual_idx: TableIndex, expected_idx: TableIndex) -> bool {
        super::fields_structurally_match_impl(&self.ir, &self.resolved_expr_cache, actual_idx, expected_idx)
    }

    pub(crate) fn structural_mismatch_details(&self, actual: &ValueType, expected: &ValueType) -> Option<Vec<super::StructuralMismatchDetail>> {
        super::structural_mismatch_details_impl(&self.ir, &self.resolved_expr_cache, actual, expected)
    }

    pub(crate) fn check_excess_structural_fields(
        &self,
        excess_inject: &mut Vec<InjectFieldCheck>,
        actual: &ValueType,
        expected: &ValueType,
        range_start: usize,
        range_end: usize,
    ) {
        let (actual_idx, expected_idx) = match (actual, expected) {
            (ValueType::Table(Some(a)), ValueType::Table(Some(b))) => (*a, *b),
            (ValueType::Table(Some(a)), ValueType::Union(types)) => {
                // Find the union member that the structural match succeeded against
                if let Some(b) = types.iter().find_map(|t| match t {
                    ValueType::Table(Some(b)) => {
                        let at = self.table(*a);
                        let bt = self.table(*b);
                        if at.class_name.is_none() && bt.class_name.is_some() && !at.fields.is_empty()
                            && self.fields_structurally_match(*a, *b)
                        {
                            Some(*b)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }) {
                    (*a, b)
                } else {
                    return;
                }
            }
            _ => return,
        };
        let at = self.table(actual_idx);
        let bt = self.table(expected_idx);
        // Only check table literal → @class structural matches
        if at.class_name.is_some() || bt.class_name.is_none() { return; }
        if at.fields.is_empty() { return; }

        let expected_fields = self.collect_class_fields(expected_idx);
        let expected_names: HashSet<&str> = expected_fields.iter().map(|(n, _, _)| n.as_str()).collect();

        let excess: Vec<String> = self.table(actual_idx).fields.keys()
            .filter(|name| !expected_names.contains(name.as_str()))
            .cloned()
            .collect();

        for field_name in excess {
            excess_inject.push(InjectFieldCheck {
                table_idx: expected_idx, field_name, scope_idx: ScopeIndex(0),
                start: range_start as u32, end: range_end as u32,
                field_existed_at_build: false,
            });
        }
    }

    /// Collect all fields for a @class table, including inherited fields from parents.
    fn collect_class_fields(&self, table_idx: TableIndex) -> Vec<(String, ValueType, bool)> {
        super::collect_class_fields_impl(&self.ir, &self.resolved_expr_cache, table_idx)
    }

    /// Structural function compatibility: when both sides are known functions,
    /// check that param arity, param types, and return types are compatible.
    ///
    /// Rules (pragmatic covariance, matching TypeScript's default `strictFunctionTypes:
    /// false` bivariance for param types):
    /// - Actual must accept AT MOST as many positional params as expected supplies
    ///   (Lua drops extras at runtime, so fewer is safe).
    /// - Each positional param type on the actual side must be `is_assignable_to` the
    ///   expected side's param type at the same position.
    /// - Actual's first return type must be `is_assignable_to` expected's first return.
    ///   Callers that declare no `@return` are treated as "any" (can satisfy any expected
    ///   return annotation).
    /// - `any` on either side satisfies anything (baked into `is_assignable_to`).
    /// - Vararg on either side disables arity enforcement (but param/return types still
    ///   compared at their declared positions).
    /// - One side is a generic `Function(None)` → always compatible (the loose fallback
    ///   for unannotated `function`-typed params).
    pub(crate) fn is_function_compatible(&self, actual: &ValueType, expected: &ValueType) -> bool {
        let (ValueType::Function(Some(actual_idx)), ValueType::Function(Some(expected_idx))) = (actual, expected) else {
            return true; // not both known functions — no structural check
        };
        let actual_func = self.func(*actual_idx);
        let actual_args = actual_func.args.clone();
        let actual_is_vararg = actual_func.is_vararg;
        let actual_param_optional = actual_func.param_optional.clone();
        let actual_rets = actual_func.return_annotations.clone();
        let expected_args = self.func(*expected_idx).args.clone();
        let expected_is_vararg = self.func(*expected_idx).is_vararg;
        let expected_rets = self.func(*expected_idx).return_annotations.clone();
        // Skip the implicit `self` parameter only when BOTH sides have it (both are
        // colon methods). When only one side names its first param `self` (e.g. a stub
        // callback annotation vs. a user-written `function(_, elapsed)`), comparing
        // positionally without skip is correct — the names differ but the slots align.
        let actual_has_self = actual_args.first()
            .map(|&idx| matches!(&self.sym(idx).id, SymbolIdentifier::Name(n) if n == "self"))
            .unwrap_or(false);
        let expected_has_self = expected_args.first()
            .map(|&idx| matches!(&self.sym(idx).id, SymbolIdentifier::Name(n) if n == "self"))
            .unwrap_or(false);
        let skip_self = if actual_has_self && expected_has_self { 1 } else { 0 };
        let actual_params = &actual_args[skip_self..];
        let expected_params = &expected_args[skip_self..];
        // Count required params on the actual side: walk backward from the end,
        // stopping at the first non-optional param. In Lua a function can always
        // be called with more args than it declares (extras are dropped), so
        // trailing optional params on the callee don't constrain assignability.
        let trailing_optional = (0..actual_params.len()).rev()
            .take_while(|&pos| actual_param_optional.get(skip_self + pos).copied().unwrap_or(false))
            .count();
        let actual_required = actual_params.len() - trailing_optional;
        // Arity: the actual's REQUIRED params must not exceed expected's param
        // count. Trailing optional params are excluded — the function works
        // without them, so extra args at those positions are harmless.
        if !expected_is_vararg && !actual_is_vararg
            && actual_required > expected_params.len() {
                return false;
            }
        // Param types: compare only required params on the actual side. Optional
        // trailing params don't need to match — the callee is designed to work
        // without them, so the caller's arg type at that position is irrelevant.
        // Note: this means the caller may pass a value of incompatible type at
        // optional positions (e.g. a string where boolean? is declared). We accept
        // this to avoid false positives in callback-heavy WoW addon patterns like
        // `frame:SetScript("OnDragStart", frame.StartMoving)` where the handler
        // signature has different trailing params than the callback type.
        for (pos, &actual_sym) in actual_params.iter().take(actual_required).enumerate() {
            // Skip `_` params — they're "don't care" placeholders. When duplicate
            // `_` params exist (e.g. `function(_, _, unit)`), both share a single
            // symbol with only the first version's type set, causing false mismatches
            // at later positions. Skipping them is correct: the user explicitly
            // opted out of using the value, so its type is irrelevant.
            if matches!(&self.sym(actual_sym).id, SymbolIdentifier::Name(n) if n == "_") {
                continue;
            }
            let actual_ty = self.sym(actual_sym).versions.first()
                .and_then(|v| v.resolved_type.clone())
                .unwrap_or(ValueType::Any);
            let expected_ty = if let Some(&expected_sym) = expected_params.get(pos) {
                self.sym(expected_sym).versions.first()
                    .and_then(|v| v.resolved_type.clone())
                    .unwrap_or(ValueType::Any)
            } else {
                ValueType::Any
            };
            if !actual_ty.is_assignable_to(&expected_ty)
                && !self.is_type_subclass_of(&actual_ty, &expected_ty)
                && !self.is_type_subclass_of(&expected_ty, &actual_ty)
                && !self.is_table_subtype(&actual_ty, &expected_ty)
                && !self.is_table_subtype(&expected_ty, &actual_ty)
            {
                return false;
            }
        }
        // Return type: compare first declared return slot. Missing returns on either
        // side → treat as Any (unannotated functions don't constrain return types).
        // Covariant returns: child class satisfies parent class expectation.
        let actual_ret = actual_rets.first().cloned()
            .unwrap_or(ValueType::Any);
        let expected_ret = expected_rets.first().cloned()
            .unwrap_or(ValueType::Any);
        if !actual_ret.is_assignable_to(&expected_ret)
            && !self.is_type_subclass_of(&actual_ret, &expected_ret)
            && !self.is_table_subtype(&actual_ret, &expected_ret)
        {
            return false;
        }
        true
    }

    fn is_type_subclass_of(&self, child: &ValueType, parent: &ValueType) -> bool {
        match (child, parent) {
            (ValueType::Table(Some(c)), ValueType::Table(Some(p))) => self.is_subclass_of(*c, *p),
            _ => false,
        }
    }


    /// Walk all symbols whose first version's def_node is a local declaration
    /// (excluding function parameters). Yields (sym_idx, name, name-token range).
    pub(crate) fn iter_local_def_sites<'a>(
        &'a self,
        tree: &'a SyntaxTree,
    ) -> impl Iterator<Item = (SymbolIndex, String, crate::syntax::TextRange)> + 'a {
        let param_syms: HashSet<SymbolIndex> = self.ir.functions.iter()
            .flat_map(|f| f.args.iter().copied())
            .collect();
        self.ir.symbols.iter().enumerate().filter_map(move |(i, sym)| {
            let sym_idx = SymbolIndex(i);
            if param_syms.contains(&sym_idx) { return None; }
            let name = match &sym.id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => return None,
            };
            let first_ver = sym.versions.first()?;
            let def_start = first_ver.def_node.start;
            let def_end = first_ver.def_node.end;
            if !self.is_local_declaration_site(tree, def_start) { return None; }
            let range = self.def_name_token_range(tree, def_start, def_end, &name)?;
            Some((sym_idx, name, range))
        })
    }

    pub(crate) fn find_enclosing_function_idx(
        &self,
        node: SyntaxNode<'_>,
        func_by_start: &HashMap<u32, usize>,
    ) -> Option<usize> {
        let mut current = node.parent();
        while let Some(n) = current {
            if n.kind() == SyntaxKind::FunctionDefinition {
                let start = u32::from(n.text_range().start());
                return func_by_start.get(&start).copied();
            }
            current = n.parent();
        }
        None
    }

    pub(crate) fn find_enclosing_function_generics(
        &self,
        node: SyntaxNode<'_>,
        func_by_start: &HashMap<u32, usize>,
    ) -> Option<Vec<(String, Option<String>)>> {
        if let Some(func_idx) = self.find_enclosing_function_idx(node, func_by_start) {
            let gcr = &self.ir.functions[func_idx].generic_constraints_raw;
            if !gcr.is_empty() {
                return Some(gcr.clone());
            }
        }
        None
    }

    pub(crate) fn get_check_time_type_args(&self, expr_id: ExprId) -> Vec<ValueType> {
        if let Some(args) = self.call_type_args.get(&expr_id) {
            return args.clone();
        }
        match self.ir.expr(expr_id) {
            Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::Grouped(inner) => {
                self.get_check_time_type_args(*inner)
            }
            Expr::AssignNarrow { inner, .. } => self.get_check_time_type_args(*inner),
            Expr::SymbolRef(sym_idx, ver) => {
                let sym = self.sym(*sym_idx);
                if let Some(version) = sym.versions.get(*ver) {
                    if !version.type_args.is_empty() {
                        return version.type_args.clone();
                    }
                    if let Some(src_expr) = version.type_source
                        && let Some(args) = self.call_type_args.get(&src_expr)
                    {
                        return args.clone();
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }


    /// Check if a field with an annotation exists on a class table, its built table, or parents.
    pub(crate) fn class_has_annotated_field(&self, table_idx: TableIndex, field_name: &str) -> bool {
        let mut to_check = vec![table_idx];
        let mut visited = std::collections::HashSet::new();
        while let Some(idx) = to_check.pop() {
            if !visited.insert(idx) { continue; }
            let table = self.ir.table(idx);
            if let Some(fi) = table.fields.get(field_name)
                && fi.annotation.is_some() { return true; }
            if let Some(bt) = table.built_table {
                let bt_table = self.ir.table(bt);
                if let Some(fi) = bt_table.fields.get(field_name)
                    && fi.annotation.is_some() { return true; }
            }
            to_check.extend_from_slice(&table.parent_classes);
        }
        false
    }

    /// Check if a field exists on a class table, its built table, or any parent class.
    /// Uses `ir.table()` for EXT_BASE-aware routing (parent_classes may contain external indices).
    pub(crate) fn class_has_field(&self, table_idx: TableIndex, field_name: &str) -> bool {
        super::class_has_field_impl(&self.ir, table_idx, field_name)
    }

    pub(crate) fn block_ends_with_return(block: &Block) -> bool {
        Self::block_always_exits(block)
    }

    pub(crate) fn block_always_exits(block: &Block) -> bool {
        // Check if block ends with a break keyword (not wrapped in a Statement node)
        let mut ends_with_break = false;
        for child in block.syntax().children_with_tokens() {
            match &child {
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BreakKeyword => {
                    ends_with_break = true;
                }
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::Whitespace || tok.kind() == SyntaxKind::Newline || tok.kind() == SyntaxKind::Comment => {}
                _ => {
                    ends_with_break = false;
                }
            }
        }
        if ends_with_break {
            return true;
        }

        let statements = block.statements();
        let Some(last) = statements.last() else { return false };
        match last {
            Statement::Return(_) => true,
            Statement::FunctionCall(call) => {
                // error() never returns
                if let Some(ident) = call.identifier() {
                    let names = ident.names();
                    names.len() == 1 && names[0] == "error"
                } else {
                    false
                }
            }
            Statement::While(_) | Statement::Repeat(_) => Self::is_infinite_loop_stmt(last),
            Statement::If(if_chain) => {
                // All branches must exit, and there must be an else
                let branches = if_chain.if_branches();
                let else_branch = if_chain.else_branch();
                if else_branch.is_none() { return false; }
                for branch in &branches {
                    if let Some(block) = branch.block() {
                        if !Self::block_always_exits(&block) { return false; }
                    } else {
                        return false;
                    }
                }
                if let Some(eb) = &else_branch {
                    if let Some(block) = eb.block() {
                        Self::block_always_exits(&block)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// `while true do ... end` / `repeat ... until false` whose body has no
    /// `break` that escapes this loop. Such a statement never falls through —
    /// the only way to leave it is `return` from inside (or `error()`), so any
    /// code after it is unreachable and a function ending in one never
    /// implicitly returns nil.
    pub(crate) fn is_infinite_loop_stmt(stmt: &Statement) -> bool {
        match stmt {
            Statement::While(wl) => {
                let Some(cond) = wl.condition() else { return false };
                if !Self::expression_is_literal_bool(&cond, true) { return false; }
                let Some(block) = wl.block() else { return false };
                !Self::node_has_escaping_break(block.syntax())
            }
            Statement::Repeat(rl) => {
                let Some(cond) = rl.condition() else { return false };
                if !Self::expression_is_literal_bool(&cond, false) { return false; }
                let Some(block) = rl.block() else { return false };
                !Self::node_has_escaping_break(block.syntax())
            }
            _ => false,
        }
    }

    fn expression_is_literal_bool(expr: &Expression, value: bool) -> bool {
        match expr {
            Expression::Literal(lit) => lit.get_bool() == Some(value),
            Expression::GroupedExpression(g) => g
                .get_expression()
                .as_ref()
                .is_some_and(|inner| Self::expression_is_literal_bool(inner, value)),
            _ => false,
        }
    }

    /// Walk `node`'s descendants looking for a `break` that would exit the
    /// enclosing loop. Recurses into if/do/etc. blocks, but stops at nested
    /// `while`/`for`/`repeat` loops (their `break` exits the inner loop) and
    /// at `FunctionDefinition` bodies (function-local control flow).
    fn node_has_escaping_break(node: SyntaxNode<'_>) -> bool {
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Token(tok) => {
                    if tok.kind() == SyntaxKind::BreakKeyword {
                        return true;
                    }
                }
                NodeOrToken::Node(sub) => match sub.kind() {
                    SyntaxKind::WhileLoop
                    | SyntaxKind::RepeatUntilLoop
                    | SyntaxKind::ForCountLoop
                    | SyntaxKind::ForInLoop
                    | SyntaxKind::FunctionDefinition => {}
                    _ => {
                        if Self::node_has_escaping_break(sub) {
                            return true;
                        }
                    }
                },
            }
        }
        false
    }

}

/// True when `node` is in an assignment-target position (inside `VariableList` or
/// `NameList`), but NOT when it's a bracket-index value expression within an
/// assignment target chain (e.g. `key` in `tbl[key] = val`).
pub(crate) fn is_assignment_target_position(node: &SyntaxNode) -> bool {
    let mut cur = *node;
    loop {
        let Some(parent) = cur.parent() else { return false };
        match parent.kind() {
            SyntaxKind::VariableList | SyntaxKind::NameList => return true,
            SyntaxKind::BracketAccess => {
                // If `cur` appears after the `[` token it is in the index
                // (value-expression) position, not the base/target position.
                let cur_start = cur.text_range().start();
                let in_index = parent.children_with_tokens()
                    .filter_map(|c| c.into_token())
                    .any(|t| t.kind() == SyntaxKind::LeftSquareBracket && cur_start > t.text_range().start());
                if in_index {
                    return false;
                }
            }
            _ => {}
        }
        cur = parent;
    }
}

/// True when the byte offset `def_start` falls inside a `LocalAssignStatement`
/// (i.e. `local x = ...`). Mirrors the build-time check that gated redefined-local.
pub(crate) fn is_local_definition(root: &SyntaxNode, def_start: u32) -> bool {
    let Some(token) = root.token_at_offset(TextSize::from(def_start)).right_biased() else { return false };
    let mut node = token.parent();
    while let Some(n) = node {
        match n.kind() {
            SyntaxKind::LocalAssignStatement => return true,
            SyntaxKind::FunctionDefinition => {
                // `local function x() end` is a local definition too
                return n.first_child_or_token_by_kind(&|k| k == SyntaxKind::LocalKeyword).is_some();
            }
            SyntaxKind::Block => return false,
            _ => node = n.parent(),
        }
    }
    false
}

/// Extract a string-literal key from a bracket-keyed table field's syntax node.
/// Mirrors the `string_literals` build-time logic: trims surrounding `"`/`'` quotes.
pub(crate) fn extract_bracket_string_key(field_node: &SyntaxNode) -> Option<String> {
    let key_expr = field_node.children().find_map(Expression::cast)?;
    let lit = match key_expr {
        Expression::Literal(l) => l,
        _ => return None,
    };
    let raw = lit.get_string()?;
    Some(raw.trim_matches(|c| c == '"' || c == '\'').to_string())
}
