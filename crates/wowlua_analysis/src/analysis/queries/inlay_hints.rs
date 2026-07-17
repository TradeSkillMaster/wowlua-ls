use super::*;

impl AnalysisResult {
    pub fn inlay_hints(
        &self,
        tree: &SyntaxTree,
        range: (u32, u32),
        config: InlayHintConfig,
    ) -> Vec<InlayHintData> {
        let mut hints = Vec::new();
        let source = tree.source();

        if config.parameter_names {
            self.collect_param_name_hints(source, range, &mut hints);
        }

        if config.chained_return_types {
            self.collect_chained_return_hints(range, &mut hints);
        }

        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            let node_range = node.text_range();
            let node_start = u32::from(node_range.start());
            let node_end = u32::from(node_range.end());
            if node_end < range.0 || node_start > range.1 {
                continue;
            }

            match node.kind() {
                SyntaxKind::LocalAssignStatement if config.variable_types => {
                    self.collect_local_type_hints(tree, node, &mut hints);
                }
                SyntaxKind::FunctionDefinition if config.function_return_types || config.parameter_types => {
                    if config.function_return_types {
                        self.collect_function_return_hints(node, &mut hints);
                    }
                    if config.parameter_types {
                        self.collect_param_type_hints(node, &mut hints);
                    }
                }
                SyntaxKind::ForInLoop if config.for_variable_types => {
                    self.collect_forin_type_hints(tree, node, &mut hints);
                }
                _ => {}
            }
        }

        hints.sort_by_key(|h| h.position);
        hints
    }

    pub(super) fn collect_param_name_hints(
        &self,
        source: &str,
        range: (u32, u32),
        hints: &mut Vec<InlayHintData>,
    ) {
        let mut seen_call_ranges: HashSet<(u32, u32)> = HashSet::new();
        for (&expr_id, cr) in &self.ir.call_resolutions {
            let call_range = match self.ir.expr(expr_id) {
                Expr::FunctionCall { call_range, .. } => *call_range,
                _ => continue,
            };
            // Multi-return expansion re-lowers the same source call multiple times,
            // creating duplicate call_resolutions entries with identical call_range.
            // Skip duplicates to avoid emitting repeated parameter hints.
            if !seen_call_ranges.insert(call_range) {
                continue;
            }
            if call_range.1 < range.0 || call_range.0 > range.1 {
                continue;
            }

            for arg in &cr.expected_args {
                // A trailing call/`...` argument sitting before another named param
                // may fan out across those params; a single param-name label there
                // would misrepresent the mapping (set in build_resolved_call_args).
                if arg.suppress_param_name_hint {
                    continue;
                }
                let name = &arg.param_name;
                if name.is_empty() || name == "?" || name == "self" || name == "..." {
                    continue;
                }

                let arg_start = arg.start as usize;
                let arg_end = arg.end as usize;
                if arg_start >= source.len() || arg_end > source.len() {
                    continue;
                }
                let arg_text = source[arg_start..arg_end].trim();

                if arg_text == name {
                    continue;
                }

                hints.push(InlayHintData {
                    position: arg.start,
                    label: format!("{}:", name),
                    kind: InlayHintKindTag::Parameter,
                    padding_left: false,
                    padding_right: true,
                });
            }
        }
    }

    pub(super) fn collect_local_type_hints(
        &self,
        tree: &SyntaxTree,
        node: SyntaxNode<'_>,
        hints: &mut Vec<InlayHintData>,
    ) {
        let Some(assign) = LocalAssign::cast(node) else { return };
        let Some(name_list) = assign.name_list() else { return };

        let rhs_exprs: Vec<Expression<'_>> = assign.expression_list()
            .map(|el| el.expressions())
            .unwrap_or_default();

        for (i, token) in name_list.name_tokens().iter().enumerate() {
            if token.text() == "_" {
                continue;
            }
            if rhs_exprs.get(i).is_some_and(|e| matches!(e, Expression::Function(_))) {
                continue;
            }

            let token_start = u32::from(token.text_range().start());
            let token_end = u32::from(token.text_range().end());

            let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, token_start) else { continue };

            if self.ir.symbol_type_annotations.contains_key(&symbol_idx) {
                continue;
            }

            let Some(resolved) = self.sym(symbol_idx).versions.first()
                .and_then(|v| v.resolved_type.as_ref())
            else { continue };

            if matches!(resolved, ValueType::Any | ValueType::Nil | ValueType::Function(Some(_))) {
                continue;
            }

            // For tables mutated via bracket assignment, show the constructor's
            // initial element type rather than the post-mutation type.
            let formatted = self.initial_array_display(resolved)
                .unwrap_or_else(|| self.format_type_for_hint(resolved));
            if formatted == "?" { continue; }

            // Append bound generic type args (e.g. Schema → Schema<string>)
            let type_args = self.get_symbol_type_args(symbol_idx, token_start);
            let formatted = self.append_type_args_to_class(&formatted, resolved, &type_args);

            hints.push(InlayHintData {
                position: token_end,
                label: format!(": {}", formatted),
                kind: InlayHintKindTag::Type,
                padding_left: true,
                padding_right: false,
            });
        }
    }

    pub(super) fn collect_function_return_hints(
        &self,
        node: SyntaxNode<'_>,
        hints: &mut Vec<InlayHintData>,
    ) {
        let Some(func_def) = FunctionDefinition::cast(node) else { return };
        let node_start = u32::from(func_def.syntax().text_range().start());

        let func_idx = self.ir.functions.iter().enumerate()
            .find(|(_, f)| f.def_node.start == node_start)
            .map(|(i, _)| FunctionIndex(i));
        let Some(func_idx) = func_idx else { return };
        let func = self.func(func_idx);

        if !func.return_annotations.is_empty() || func.returns_self || func.explicit_void_return {
            return;
        }

        let rets = self.format_inferred_returns_for_hint(func);
        if rets.is_empty() { return; }

        let Some(pl) = func_def.params() else { return };
        let hint_pos = u32::from(pl.syntax().text_range().end());

        hints.push(InlayHintData {
            position: hint_pos,
            label: format!("-> {}", join_returns(&rets)),
            kind: InlayHintKindTag::Type,
            padding_left: true,
            padding_right: false,
        });
    }

    pub(super) fn collect_param_type_hints(
        &self,
        node: SyntaxNode<'_>,
        hints: &mut Vec<InlayHintData>,
    ) {
        let Some(func_def) = FunctionDefinition::cast(node) else { return };
        let node_start = u32::from(func_def.syntax().text_range().start());

        let func_idx = self.ir.functions.iter().enumerate()
            .find(|(_, f)| f.def_node.start == node_start)
            .map(|(i, _)| FunctionIndex(i));
        let Some(func_idx) = func_idx else { return };
        let func = self.func(func_idx);
        let sentinel = crate::annotations::AnnotationType::Simple(String::new());

        let Some(pl) = func_def.params() else { return };
        let src_params: Vec<_> = pl.syntax().children_with_tokens()
            .filter_map(|t| match t {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Parameter => Some(t),
                _ => None,
            })
            .collect();

        let self_injected = func.args.len() == src_params.len() + 1
            && matches!(&self.sym(func.args[0]).id,
                SymbolIdentifier::Name(n) if n == "self");
        let arg_offset = if self_injected { 1 } else { 0 };

        for (i, token) in src_params.iter().enumerate() {
            if token.text() == "self" { continue; }

            let arg_i = i + arg_offset;
            if arg_i >= func.args.len() { break; }

            let annotated = func.param_annotations.get(arg_i)
                .is_some_and(|a| a != &sentinel);
            if annotated { continue; }

            let sym_idx = func.args[arg_i];
            if sym_idx.is_external() { continue; }

            let Some(resolved) = self.sym(sym_idx).versions.first()
                .and_then(|v| v.resolved_type.as_ref())
            else { continue };

            if matches!(resolved, ValueType::Any | ValueType::Nil) { continue; }

            let formatted = self.format_type_for_hint(resolved);

            let token_start = u32::from(token.text_range().start());
            let type_args = self.get_symbol_type_args(sym_idx, token_start);
            let formatted = self.append_type_args_to_class(&formatted, resolved, &type_args);

            let token_end = u32::from(token.text_range().end());
            hints.push(InlayHintData {
                position: token_end,
                label: format!(": {}", formatted),
                kind: InlayHintKindTag::Type,
                padding_left: false,
                padding_right: false,
            });
        }

        // Varargs parameter hint (suppressed when the user wrote @param ...)
        if func.is_vararg
            && !self.vararg_user_annotated_fns.contains(&func_idx)
            && let Some(ref ann) = func.vararg_annotation
        {
            let vararg_token = pl.syntax().children_with_tokens()
                .find_map(|t| match t {
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::ParameterVarArgs => Some(t),
                    _ => None,
                });
            if let Some(token) = vararg_token {
                let type_text = crate::annotations::format_annotation_type(ann);
                let token_end = u32::from(token.text_range().end());
                hints.push(InlayHintData {
                    position: token_end,
                    label: format!(": {}", type_text),
                    kind: InlayHintKindTag::Type,
                    padding_left: false,
                    padding_right: false,
                });
            }
        }
    }

    pub(super) fn collect_forin_type_hints(
        &self,
        tree: &SyntaxTree,
        node: SyntaxNode<'_>,
        hints: &mut Vec<InlayHintData>,
    ) {
        let Some(for_in) = ForInLoop::cast(node) else { return };
        let Some(name_list) = for_in.name_list() else { return };

        for token in name_list.name_tokens() {
            if token.text() == "_" {
                continue;
            }

            let token_start = u32::from(token.text_range().start());
            let token_end = u32::from(token.text_range().end());

            let Some((symbol_idx, _, _)) = self.find_symbol_at(tree, token_start) else { continue };

            if self.ir.symbol_type_annotations.contains_key(&symbol_idx) {
                continue;
            }

            let Some(resolved) = self.sym(symbol_idx).versions.first()
                .and_then(|v| v.resolved_type.as_ref())
            else { continue };

            if matches!(resolved, ValueType::Any) { continue; }

            let formatted = self.format_type_for_hint(resolved);
            if formatted == "?" { continue; }

            let type_args = self.get_symbol_type_args(symbol_idx, token_start);
            let formatted = self.append_type_args_to_class(&formatted, resolved, &type_args);

            hints.push(InlayHintData {
                position: token_end,
                label: format!(": {}", formatted),
                kind: InlayHintKindTag::Type,
                padding_left: true,
                padding_right: false,
            });
        }
    }

    pub(super) fn collect_chained_return_hints(
        &self,
        range: (u32, u32),
        hints: &mut Vec<InlayHintData>,
    ) {
        // Build a set of expr IDs that are used as the `table` of a FieldAccess.
        // These are the "chained" call results — their return type feeds into the next access.
        let mut chained_exprs: HashSet<ExprId> = HashSet::new();
        for expr in &self.ir.exprs {
            if let Expr::FieldAccess { table, .. } = expr {
                chained_exprs.insert(*table);
            }
        }

        for &expr_id in self.ir.call_resolutions.keys() {
            // Only emit for calls whose result is used as receiver of further access
            if !chained_exprs.contains(&expr_id) {
                continue;
            }

            let call_range = match self.ir.expr(expr_id) {
                Expr::FunctionCall { call_range, .. } => *call_range,
                _ => continue,
            };

            // Range filter
            if call_range.1 < range.0 || call_range.0 > range.1 {
                continue;
            }

            let Some(resolved) = self.resolve_expr_type(expr_id) else { continue };

            if matches!(resolved, ValueType::Any | ValueType::Nil) {
                continue;
            }

            let formatted = self.format_type_for_hint(&resolved);
            if formatted == "?" {
                continue;
            }

            let type_args = self.get_type_args_for_expr(expr_id);
            let formatted = self.append_type_args_to_class(&formatted, &resolved, &type_args);

            hints.push(InlayHintData {
                position: call_range.1,
                label: format!(": {}", formatted),
                kind: InlayHintKindTag::Type,
                padding_left: false,
                padding_right: false,
            });
        }
    }
}
