use super::*;

impl AnalysisResult {
    pub fn code_lens(&self) -> Vec<CodeLensData> {
        let mut results = Vec::new();

        // Build class_name → set of methods defined on that class (across all
        // tables). In per-file analysis, methods from `function Class:Method()`
        // end up on the variable-backed table, not the prescan class table.
        // Use function_owner_class to associate methods with classes.
        let mut class_methods: HashMap<&str, HashSet<&str>> = HashMap::new();
        // (class, method) → the method's defining expr, used to resolve the
        // overridden (parent) method's location for the "overrides X" lens. Same
        // iteration order/source as `class_methods`, so any name in the latter has
        // an entry here; first writer wins (local definitions before external).
        let mut method_exprs: HashMap<(&str, &str), ExprId> = HashMap::new();
        for table in &self.ir.tables {
            for (field_name, field) in &table.fields {
                if let Some(func_idx) = self.field_func_idx(field)
                    && let Some(cn) = self.function_owner_class.get(&func_idx)
                {
                    class_methods.entry(cn.as_str()).or_default().insert(field_name.as_str());
                    method_exprs.entry((cn.as_str(), field_name.as_str())).or_insert(field.expr);
                }
            }
        }
        // Also include methods from external class tables.
        for (name, &table_idx) in &self.ir.ext.classes {
            let table = self.table(table_idx);
            for (field_name, field) in &table.fields {
                let is_func = field.annotation.as_ref().is_some_and(|a| matches!(a, ValueType::Function(_)))
                    || (field.expr.is_external()
                        && matches!(self.ir.try_expr(field.expr), Some(Expr::FunctionDef(_))));
                if is_func {
                    class_methods.entry(name.as_str()).or_default().insert(field_name.as_str());
                    method_exprs.entry((name.as_str(), field_name.as_str())).or_insert(field.expr);
                }
            }
        }

        // Build child-count map: parent_class_name → count of direct subclasses.
        let mut child_counts: HashMap<&str, usize> = HashMap::new();
        for &table_idx in self.ir.classes.values() {
            for &parent_idx in &self.table(table_idx).parent_classes {
                if let Some(parent_name) = &self.table(parent_idx).class_name {
                    *child_counts.entry(parent_name.as_str()).or_insert(0) += 1;
                }
            }
        }
        for &table_idx in self.ir.ext.classes.values() {
            for &parent_idx in &self.table(table_idx).parent_classes {
                if let Some(parent_name) = &self.table(parent_idx).class_name {
                    *child_counts.entry(parent_name.as_str()).or_insert(0) += 1;
                }
            }
        }

        // Emit "N implementations" lens for each local @class declaration.
        for (class_name, &(range_start, range_end)) in &self.ir.class_def_ranges {
            let count = child_counts.get(class_name.as_str()).copied().unwrap_or(0);
            results.push(CodeLensData {
                range_start,
                range_end,
                kind: CodeLensKind::Implementations {
                    count,
                    class_name: class_name.clone(),
                },
            });
        }

        // Emit "overrides Parent" lens for methods that override a parent method.
        for table in &self.ir.tables {
            for (field_name, field) in &table.fields {
                let func_idx = match self.field_func_idx(field) {
                    Some(idx) => idx,
                    None => continue,
                };
                let class_name = match self.function_owner_class.get(&func_idx) {
                    Some(n) => n,
                    None => continue,
                };
                let func = self.func(func_idx);
                if func.def_node == DefNode::DUMMY { continue; }
                let class_table_idx = match self.ir.classes.get(class_name.as_str())
                    .or_else(|| self.ir.ext.classes.get(class_name.as_str()))
                {
                    Some(&idx) => idx,
                    None => continue,
                };
                if self.table(class_table_idx).parent_classes.is_empty() { continue; }
                if let Some(parent_name) = self.find_overridden_parent(class_table_idx, field_name, &class_methods) {
                    // Resolve the overridden (parent) method's definition so the
                    // "overrides X" lens navigates straight to the parent.
                    let parent_defs = method_exprs
                        .get(&(parent_name.as_str(), field_name.as_str()))
                        .and_then(|&expr| self.definition_for_expr(expr))
                        .into_iter()
                        .collect();
                    results.push(CodeLensData {
                        range_start: func.def_node.start,
                        range_end: func.def_node.end,
                        kind: CodeLensKind::Overrides {
                            parent_class: parent_name,
                            parent_defs,
                        },
                    });
                }
            }
        }

        results.sort_by_key(|l| l.range_start);
        results
    }

    pub(super) fn find_overridden_parent(
        &self,
        table_idx: TableIndex,
        method_name: &str,
        class_methods: &HashMap<&str, HashSet<&str>>,
    ) -> Option<String> {
        let mut visited = HashSet::new();
        self.find_overridden_parent_inner(table_idx, method_name, class_methods, &mut visited)
    }

    pub(super) fn find_overridden_parent_inner(
        &self,
        table_idx: TableIndex,
        method_name: &str,
        class_methods: &HashMap<&str, HashSet<&str>>,
        visited: &mut HashSet<TableIndex>,
    ) -> Option<String> {
        let table = self.table(table_idx);
        for &parent_idx in &table.parent_classes {
            if !visited.insert(parent_idx) { continue; }
            let parent = self.table(parent_idx);
            let Some(parent_name) = parent.class_name.as_deref() else { continue; };
            if class_methods.get(parent_name).is_some_and(|m| m.contains(method_name)) {
                return Some(parent_name.to_string());
            }
            if let Some(name) = self.find_overridden_parent_inner(parent_idx, method_name, class_methods, visited) {
                return Some(name);
            }
        }
        None
    }

    /// Collect code-lens targets: one entry per non-external function definition
    /// in this file. Each entry carries the function name, definition range, and
    /// a byte offset inside the name token suitable for `reference_target_at`.
    pub fn code_lens_targets(&self, tree: &SyntaxTree) -> Vec<CodeLensTarget> {
        let mut targets = Vec::new();

        // Top-level named functions (scope 0)
        for (id, sym_idx) in self.ir.scope0_local_symbols() {
            let SymbolIdentifier::Name(name) = id else { continue };
            if sym_idx.is_external() { continue; }
            let sym = self.sym(sym_idx);
            let ver = match sym.versions.first() {
                Some(v) => v,
                None => continue,
            };
            if ver.def_node == DefNode::DUMMY { continue; }
            let Some(ValueType::Function(Some(func_idx))) = &ver.resolved_type else { continue };
            let func = self.func(*func_idx);
            let func_def = func.def_node;
            if func_def == DefNode::DUMMY { continue; }

            if let Some(name_offset) = self.def_name_token_offset(tree, ver.def_node.start, ver.def_node.end, name) {
                targets.push(CodeLensTarget {
                    name: name.clone(),
                    def_start: func_def.start,
                    def_end: func_def.end,
                    name_offset,
                });
            }
        }

        // Class/table methods and non-class table functions
        let mut visited_tables: HashSet<TableIndex> = HashSet::new();

        // Class tables (from ir.classes)
        for &table_idx in self.ir.classes.values() {
            if table_idx.is_external() { continue; }
            visited_tables.insert(table_idx);
            self.collect_field_lens_targets(tree, table_idx, &mut targets);
        }

        // Scope-0 non-class tables (e.g. `local M = {}; function M.foo() end`)
        for (id, sym_idx) in self.ir.scope0_local_symbols() {
            let SymbolIdentifier::Name(_) = id else { continue };
            if sym_idx.is_external() { continue; }
            let sym = self.sym(sym_idx);
            let ver = match sym.versions.first() {
                Some(v) => v,
                None => continue,
            };
            if let Some(ValueType::Table(Some(table_idx))) = &ver.resolved_type
                && !table_idx.is_external() && visited_tables.insert(*table_idx) {
                    self.collect_field_lens_targets(tree, *table_idx, &mut targets);
                }
        }

        targets.sort_by_key(|t| t.def_start);
        targets.dedup_by_key(|t| t.def_start);
        targets
    }

    pub(super) fn collect_field_lens_targets(&self, tree: &SyntaxTree, table_idx: TableIndex, targets: &mut Vec<CodeLensTarget>) {
        let table = self.table(table_idx);
        for (field_name, field) in &table.fields {
            let Some(func_idx) = self.field_func_idx(field) else { continue };
            let func = self.func(func_idx);
            if func.def_node == DefNode::DUMMY { continue; }
            let search_start = match field.def_range {
                Some((s, _)) => s,
                None => func.def_node.start,
            };
            let search_end = func.def_node.end;
            let Some(name_offset) = self.def_name_token_offset(tree, search_start, search_end, field_name) else {
                continue;
            };
            targets.push(CodeLensTarget {
                name: field_name.clone(),
                def_start: func.def_node.start,
                def_end: func.def_node.end,
                name_offset,
            });
        }
    }
}
