use super::*;

pub struct CallSiteResult {
    pub call_range: (u32, u32),
    pub enclosing_func: Option<FunctionIndex>,
}

pub struct OutgoingCallResult {
    pub func_idx: FunctionIndex,
    pub name: String,
    pub call_ranges: Vec<(u32, u32)>,
}

impl AnalysisResult {
    /// Resolve the function at `offset` for call hierarchy. Returns the function
    /// index and the display name (with class prefix for methods).
    pub fn call_hierarchy_item_at(
        &self,
        tree: &SyntaxTree,
        offset: u32,
    ) -> Option<(FunctionIndex, String)> {
        // Try symbol-based resolution first (cursor on a function name).
        if let Some((sym_idx, name, _)) = self.find_symbol_at(tree, offset)
            && let Some(func_idx) = self.resolve_symbol_to_function(sym_idx)
        {
            let display = self.call_hierarchy_display_name(func_idx, &name);
            return Some((func_idx, display));
        }
        // Try field-based resolution (cursor on a method name like `Foo:Bar`).
        if let Some((table_idx, field_name, expr_id, access_kind, _)) = self.resolve_field_chain_at(tree, offset)
        {
            let field_type = self.resolve_expr_type(expr_id);
            if let Some(ValueType::Function(Some(func_idx))) = field_type {
                let sep = match access_kind {
                    FieldAccessKind::Colon => ":",
                    FieldAccessKind::Dot => ".",
                };
                let class = self.table(table_idx).class_name.as_deref().unwrap_or("?");
                let display = format!("{}{}{}", class, sep, field_name);
                return Some((func_idx, display));
            }
        }
        None
    }

    pub(super) fn resolve_symbol_to_function(&self, sym_idx: SymbolIndex) -> Option<FunctionIndex> {
        let sym = self.sym(sym_idx);
        for ver in &sym.versions {
            if let Some(ValueType::Function(Some(idx))) = &ver.resolved_type {
                return Some(*idx);
            }
            if let Some(src) = ver.type_source
                && let Expr::FunctionDef(idx) = self.expr(src) {
                    return Some(*idx);
            }
        }
        None
    }

    pub fn call_hierarchy_display_name(&self, func_idx: FunctionIndex, base_name: &str) -> String {
        if let Some(class_name) = self.function_owner_class.get(&func_idx) {
            let func = self.func(func_idx);
            let has_self = func.args.first().is_some_and(|&s| {
                matches!(&self.sym(s).id, SymbolIdentifier::Name(n) if n == "self")
            });
            let sep = if has_self { ":" } else { "." };
            format!("{}{}{}", class_name, sep, base_name)
        } else {
            base_name.to_string()
        }
    }

    /// Returns the class name at the cursor position, or `None` if the cursor is
    /// not on a class name.
    ///
    /// Checks two contexts:
    /// 1. Annotation context (`---@class Foo`, `---@type Foo`, etc.) — `annotation_word_at`
    ///    extracts the word and we verify it is a known class.
    /// 2. Non-annotation context — resolves the symbol under the cursor and checks
    ///    whether its type is a class table.
    pub fn type_hierarchy_class_at(&self, tree: &SyntaxTree, offset: u32) -> Option<String> {
        // Annotation context: cursor on a class name inside a ---@ comment.
        if let Some(word) = self.annotation_word_at(tree, offset)
            && (self.ir.classes.contains_key(&word) || self.ir.ext.classes.contains_key(&word))
        {
            return Some(word);
        }
        // Non-annotation context: symbol whose resolved type is a class table.
        let (sym_idx, _, _) = self.find_symbol_at(tree, offset)?;
        let sym = self.sym(sym_idx);
        for version in sym.versions.iter().rev() {
            if let Some(ValueType::Table(Some(table_idx))) = &version.resolved_type
                && let Some(class_name) = self.table(*table_idx).class_name.as_deref()
            {
                return Some(class_name.to_string());
            }
        }
        None
    }

    /// Find the innermost function containing the given byte offset.
    /// Returns `None` for file-level code outside any function.
    pub fn enclosing_function_at(&self, offset: u32) -> Option<FunctionIndex> {
        let scope_idx = self.scope_at_offset(offset)?;
        let scope_to_func = self.build_scope_to_function_map();
        Self::enclosing_function_for_scope(&self.ir, scope_idx, &scope_to_func)
    }

    pub(super) fn build_scope_to_function_map(&self) -> HashMap<ScopeIndex, FunctionIndex> {
        let mut map = HashMap::new();
        for (i, func) in self.ir.functions.iter().enumerate() {
            map.insert(func.scope, FunctionIndex(i));
        }
        map
    }

    pub(super) fn enclosing_function_for_scope(
        ir: &super::Ir,
        scope_idx: ScopeIndex,
        scope_to_func: &HashMap<ScopeIndex, FunctionIndex>,
    ) -> Option<FunctionIndex> {
        let mut cur = Some(scope_idx);
        while let Some(s) = cur {
            if s.is_external() { break; }
            if let Some(&func_idx) = scope_to_func.get(&s) {
                return Some(func_idx);
            }
            cur = ir.scopes.get(s.val()).and_then(|sc| sc.parent);
        }
        None
    }

    pub(super) fn find_event_vararg_types_at_scope(&self, scope_idx: ScopeIndex) -> Option<&Vec<ValueType>> {
        crate::analysis::ancestor_scopes(&self.ir.scopes, scope_idx)
            .find_map(|s| self.event_vararg_types.get(&s))
    }

    pub fn outgoing_calls_from_function(
        &self,
        func_idx: FunctionIndex,
    ) -> Vec<OutgoingCallResult> {
        let func = self.func(func_idx);
        let body_start = func.def_node.start;
        let body_end = func.def_node.end;

        // Collect ranges of nested function definitions to exclude their calls.
        let nested_ranges: Vec<(u32, u32)> = self.ir.functions.iter()
            .filter(|f| {
                let dn = &f.def_node;
                dn.start > body_start && dn.end <= body_end
            })
            .map(|f| (f.def_node.start, f.def_node.end))
            .collect();

        let mut calls: HashMap<FunctionIndex, (String, Vec<(u32, u32)>)> = HashMap::new();

        for (expr_id, expr) in self.ir.exprs.iter().enumerate() {
            if let Expr::FunctionCall { call_range, func: callee_expr, ret_index, .. } = expr {
                if *ret_index != 0 { continue; }
                if call_range.0 < body_start || call_range.1 > body_end { continue; }
                if nested_ranges.iter().any(|&(ns, ne)| call_range.0 >= ns && call_range.1 <= ne) {
                    continue;
                }

                if let Some(resolution) = self.ir.call_resolutions.get(&ExprId(expr_id)) {
                    let target_idx = resolution.func_idx;
                    let name = self.callee_display_name(target_idx, *callee_expr);
                    let entry = calls.entry(target_idx).or_insert_with(|| (name, Vec::new()));
                    entry.1.push(*call_range);
                }
            }
        }

        calls.into_iter()
            .map(|(func_idx, (name, call_ranges))| OutgoingCallResult { func_idx, name, call_ranges })
            .collect()
    }

    pub(super) fn callee_display_name(&self, func_idx: FunctionIndex, callee_expr: ExprId) -> String {
        if let Expr::FieldAccess { field, table, .. } = self.expr(callee_expr) {
            // Try class_name first, then fall back to the expression's symbol name
            let table_name = self.resolve_expr_type(*table)
                .and_then(|vt| match vt {
                    ValueType::Table(Some(idx)) => self.table(idx).class_name.clone(),
                    _ => None,
                })
                .or_else(|| self.expr_symbol_name(*table).map(str::to_owned));

            if let Some(name) = table_name {
                let func = self.func(func_idx);
                let has_self = func.args.first().is_some_and(|&s| {
                    matches!(&self.sym(s).id, SymbolIdentifier::Name(n) if n == "self")
                });
                let sep = if has_self { ":" } else { "." };
                return format!("{}{}{}", name, sep, field);
            }
        }
        self.function_name(func_idx).unwrap_or_else(|| "(anonymous)".to_string())
    }

    /// Get the symbol name for an expression if it's a simple symbol reference.
    pub(super) fn expr_symbol_name(&self, expr_id: ExprId) -> Option<&str> {
        if let Expr::SymbolRef(sym_idx, _) = self.expr(expr_id)
            && let SymbolIdentifier::Name(name) = &self.sym(*sym_idx).id
        {
            return Some(name.as_str());
        }
        None
    }

    /// Collect all function call expression ranges where the callee resolves to
    /// `target_func_idx`. Used by incoming-calls to find call sites.
    pub fn call_sites_for_function(
        &self,
        target_func_idx: FunctionIndex,
    ) -> Vec<CallSiteResult> {
        let scope_to_func = self.build_scope_to_function_map();
        let mut results: Vec<CallSiteResult> = Vec::new();

        for (expr_id, expr) in self.ir.exprs.iter().enumerate() {
            if let Expr::FunctionCall { call_range, ret_index, .. } = expr {
                if *ret_index != 0 { continue; }
                if let Some(resolution) = self.ir.call_resolutions.get(&ExprId(expr_id))
                    && resolution.func_idx == target_func_idx
                {
                    let enclosing = self.scope_at_offset(call_range.0)
                        .and_then(|s| Self::enclosing_function_for_scope(&self.ir, s, &scope_to_func));
                    results.push(CallSiteResult {
                        call_range: *call_range,
                        enclosing_func: enclosing,
                    });
                }
            }
        }

        results
    }
}
