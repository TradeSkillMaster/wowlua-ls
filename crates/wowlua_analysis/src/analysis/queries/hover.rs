use super::*;

impl AnalysisResult {
    pub fn hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        // Compute enclosing class for visibility filtering in hover tooltips
        let enclosing_class = {
            let text_size = TextSize::from(offset);
            let node = SyntaxNode::new_root(tree).token_at_offset(text_size)
                .right_biased()
                .and_then(|t| t.parent());
            node.and_then(|n| self.find_enclosing_class(&n))
        };
        // Try field access first (e.g. "GetText" in Inbox.GetText) so that
        // a same-named global doesn't shadow the field result.
        if let Some((table_idx, field_name, expr_id, access_kind, receiver_tables)) = self.resolve_field_chain_at(tree, offset) {
            let is_g_env = self.ir.is_global_env(table_idx);
            // Try to resolve the field's type for function detection
            let resolved_type = self.resolve_expr_type(expr_id);
            let is_func = matches!(&resolved_type, Some(ValueType::Function(Some(_))));
            let table_name = self.table(table_idx).class_name.clone();
            let sep = match access_kind {
                FieldAccessKind::Colon => ":",
                FieldAccessKind::Dot => ".",
            };

            if is_func
                && let Some(ValueType::Function(Some(func_idx))) = &resolved_type {
                    let skip_self = access_kind == FieldAccessKind::Colon;
                    let qualified_name = if is_g_env {
                        field_name.clone()
                    } else {
                        match &table_name {
                            Some(tname) => format!("{}{}{}", tname, sep, field_name),
                            None => field_name.clone(),
                        }
                    };
                    let kind_label = if is_g_env { "global" } else if access_kind == FieldAccessKind::Colon { "method" } else { "field" };
                    // For a method/field on a parameterized-class receiver, look up
                    // the class type-param substitution (e.g. `T → string`) recorded
                    // at call resolution, keyed by this method-name token's range,
                    // so the displayed signature shows concrete bound types.
                    let subs = self.method_name_range_at(tree, offset)
                        .and_then(|r| self.method_decl_subs.get(&r));
                    let mut type_str = format!("({}) {}", kind_label, self.format_function_decl(*func_idx, &qualified_name, skip_self, subs));
                    let mut doc = self.format_function_doc(*func_idx);
                    // When the receiver is a union type, collect additional function
                    // signatures from other union members for the hover display.
                    if !is_g_env && receiver_tables.len() > 1 {
                        let all_fields = self.find_all_fields_in_tables(&receiver_tables, &field_name);
                        for (alt_table_idx, alt_expr_id) in all_fields {
                            if alt_table_idx == table_idx { continue; }
                            let Some(ValueType::Function(Some(alt_func_idx))) = self.resolve_expr_type(alt_expr_id) else { continue };
                            let alt_name = self.table(alt_table_idx).class_name.as_ref()
                                .map(|n| format!("{}{}{}", n, sep, field_name))
                                .unwrap_or_else(|| field_name.clone());
                            type_str.push_str(&format!("\n({}) {}", kind_label,
                                self.format_function_decl(alt_func_idx, &alt_name, skip_self, None)));
                            if let Some(alt_doc) = self.format_function_doc(alt_func_idx) {
                                doc = Some(match doc {
                                    Some(d) => format!("{}\n\n---\n\n{}", d, alt_doc),
                                    None => alt_doc,
                                });
                            }
                        }
                    }
                    return Some(HoverResult { type_str, doc });
                }

            if let Some(field_info) = self.get_field(table_idx, &field_name) {
                let (formatted, effective_type, has_annotation) = {
                    if let Some(ref text) = field_info.annotation_text {
                        let expansion = field_info.annotation_type_raw.as_ref()
                            .and_then(|raw| self.expand_alias_fun_signature(raw));
                        let s = match expansion {
                            Some(exp) => format!("{}\n  = {}", text, exp),
                            None => text.clone(),
                        };
                        (s, resolved_type.clone(), true)
                    } else if field_info.lateinit {
                        // Lateinit fields use compact format so "!" appears cleanly after the type name
                        (self.format_field_type(field_info, 0), resolved_type.clone(), true)
                    } else if let Some(ref ann) = field_info.annotation {
                        // An `Any` field annotation is a placeholder (scan-injected
                        // existence-only self-field, inherited-parent `Any`). Refine
                        // it from the field's own assignment exprs — matching the
                        // diagnostics engine — so hover shows the concrete assigned
                        // type instead of a bare `any`. (Can't use `resolved_type`
                        // here: resolve_field_chain_at reports the field's placeholder
                        // definition expr, which is the `Any` itself.)
                        if matches!(ann, ValueType::Any)
                            && let Some(refined) = self.refine_any_field_type(field_info)
                        {
                            (self.format_type_accessible(&refined, enclosing_class), Some(refined), false)
                        } else {
                            (self.format_type_accessible(ann, enclosing_class), Some(ann.clone()), true)
                        }
                    } else {
                        let has_extras = !field_info.extra_exprs.is_empty();
                        let skip_primary = has_extras
                            && matches!(self.resolve_expr_type(field_info.expr), Some(ValueType::Nil));
                        let mut types: Vec<ValueType> = Vec::new();
                        let exprs: Vec<ExprId> = if skip_primary {
                            field_info.extra_exprs.clone()
                        } else {
                            std::iter::once(field_info.expr).chain(field_info.extra_exprs.iter().copied()).collect()
                        };
                        let mut has_unresolvable = false;
                        for eid in exprs {
                            if let Some(vt) = self.resolve_expr_type(eid) {
                                if !types.contains(&vt) {
                                    types.push(vt);
                                }
                            } else {
                                has_unresolvable = true;
                            }
                        }
                        // If the primary was a nil placeholder (skipped) and
                        // any reassignment couldn't be resolved, the field
                        // could hold any type — widen to Any.
                        if has_unresolvable && skip_primary
                            && !types.contains(&ValueType::Any)
                        {
                            types.push(ValueType::Any);
                        }
                        if types.is_empty() {
                            ("?".to_string(), None, false)
                        } else {
                            let unified = ValueType::make_union(self.ir.collapse_subset_tables(types));
                            let s = self.format_type_accessible(&unified, enclosing_class);
                            (s, Some(unified), false)
                        }
                    }
                };
                let formatted = if !has_annotation {
                    let mut type_args = self.get_type_args_for_expr(expr_id);
                    if type_args.is_empty() {
                        if let Some(args) = self.call_type_args.get(&field_info.expr) {
                            type_args = args.clone();
                        }
                        if type_args.is_empty() {
                            for &extra in &field_info.extra_exprs {
                                if let Some(args) = self.call_type_args.get(&extra) {
                                    type_args = args.clone();
                                    break;
                                }
                            }
                        }
                    }
                    if let Some(ref rt) = effective_type {
                        self.append_type_args_to_class(&formatted, rt, &type_args)
                    } else {
                        formatted
                    }
                } else {
                    formatted
                };
                let mut type_str = format!("(field) {}: {}", field_name, formatted);
                if self.table(table_idx).enum_kind.is_enum()
                    && let Some(val) = self.get_field_literal_value(field_info)
                {
                    type_str.push_str(&format!(" = {}", val));
                }
                let doc = effective_type.as_ref().and_then(|r| self.doc_for_type(r));
                let doc = if let Some(ValueType::Table(Some(table_idx))) = &effective_type {
                    self.append_call_hover(*table_idx, &mut type_str, doc)
                } else {
                    doc
                };
                return Some(HoverResult { type_str, doc });
            }
            if let Some(resolved) = resolved_type {
                let type_args = self.get_type_args_for_expr(expr_id);
                let formatted = self.format_type(&resolved);
                let formatted = self.append_type_args_to_class(&formatted, &resolved, &type_args);
                let label = if is_g_env { "global" } else { "field" };
                let mut type_str = format!("({}) {}: {}", label, field_name, formatted);
                let doc = self.doc_for_type(&resolved);
                let doc = if let ValueType::Table(Some(table_idx)) = &resolved {
                    self.append_call_hover(*table_idx, &mut type_str, doc)
                } else {
                    doc
                };
                return Some(HoverResult { type_str, doc });
            }
            return None;
        }
        // Injected field carried cross-file by an inline `TableShape` member
        // (e.g. `dropdown.DropDown` where `dropdown: Frame & { DropDown: ... }`).
        // These have no arena `TableIndex`, so the class-field chain above misses
        // them; resolve the field's type directly off the shape.
        if let Some((field_name, field_ty)) = self.shape_field_hover_at(offset) {
            let formatted = self.format_type(&field_ty);
            return Some(HoverResult {
                type_str: format!("(field) {}: {}", field_name, formatted),
                doc: self.doc_for_type(&field_ty),
            });
        }
        // Check for @accessor token hover (e.g. __private in Widget.__private:Method)
        if let Some(result) = self.accessor_hover_at(tree, offset, enclosing_class) {
            return Some(result);
        }
        if Self::is_field_position(tree, offset) && !self.is_g_dot_field(tree, offset) {
            return None;
        }
        // Try varargs hover (... in expressions or parameter lists)
        if let Some(result) = self.varargs_hover_at(tree, offset) {
            return Some(result);
        }
        // Try table constructor field before symbol lookup, so that a same-named
        // global doesn't shadow the field result (e.g. `{ ARMOR = expr }` where
        // ARMOR is also a global string).
        if let Some((field_name, field_info)) = self.find_constructor_field_at(tree, offset) {
            if let Some(ref text) = field_info.annotation_text {
                let expansion = field_info.annotation_type_raw.as_ref()
                    .and_then(|raw| self.expand_alias_fun_signature(raw));
                let type_str = match expansion {
                    Some(exp) => format!("(field) {}: {}\n  = {}", field_name, text, exp),
                    None => format!("(field) {}: {}", field_name, text),
                };
                return Some(HoverResult { type_str, doc: None });
            }
            let type_str = format!("(field) {}: {}", field_name, self.format_field_type(&field_info, 0));
            return Some(HoverResult { type_str, doc: None });
        }
        if let Some((symbol_idx, name, token_start)) = self.find_symbol_at(tree, offset) {
            let symbol = self.sym(symbol_idx);
            let is_param = self.is_param_symbol(symbol_idx);
            // Resolve the type at this reference using shared version-selection logic.
            let resolved = self.symbol_resolved_type_at(symbol_idx, token_start);
            // Determine kind prefix
            let kind = if symbol_idx.is_external() {
                "global"
            } else if symbol.scope_idx == ScopeIndex(0) {
                let def_start = symbol.versions.first().map(|v| v.def_node.start).unwrap_or(0);
                if self.is_local_declaration_site(tree, def_start) { "local" } else { "global" }
            } else if is_param {
                "param"
            } else {
                "local"
            };
            if let Some(resolved) = resolved {
                let ver_idx = self.symbol_version_at.get(&token_start).copied().unwrap_or(0);
                // A token with no `symbol_version_at` entry is a declaration /
                // assignment target, not a use. Guard-based narrowing (e.g.
                // `if not x then return end`) is recorded scope-wide but textually
                // follows the declaration, so it must not apply at the declaration
                // site. Skipping `narrow_type_for_display` here keeps a guarded local
                // (or param) showing its declared, un-narrowed type at the line where
                // it is introduced — matching sibling multi-return values, which are
                // narrowed via position-aware versions and already show the
                // un-narrowed type there.
                let is_decl_site = !symbol_idx.is_external()
                    && !self.symbol_version_at.contains_key(&token_start);
                let display_type = if is_decl_site {
                    None
                } else {
                    self.narrow_type_for_display(resolved, symbol_idx, offset)
                };
                let display_ref = display_type.as_ref().unwrap_or(resolved);
                let doc = self.doc_for_type(display_ref);
                // A param whose annotation is a function-typed alias (e.g.
                // `@param cb MyFuncType` where `MyFuncType = fun(...)`) is
                // materialized to a concrete Function for type-checking, but its
                // hover should keep showing the alias name + expanded signature
                // (handled by the param-annotation block below), not the bare
                // `function name(...)` form. A direct `fun(...)` annotation is
                // excluded so it still renders as `function name(...)`.
                let is_fun_alias_param = kind == "param" && ver_idx == 0 && display_type.is_none()
                    && self.find_param_annotation_raw(symbol_idx).is_some_and(|raw| {
                        !matches!(raw, crate::annotations::AnnotationType::Fun(..))
                            && self.expand_alias_fun_signature(raw).is_some()
                    });
                // Declaration-style for functions
                if !is_fun_alias_param
                    && let ValueType::Function(Some(func_idx)) = display_ref {
                    let type_str = format!("({}) {}", kind, self.format_function_decl(*func_idx, &name, false, None));
                    return Some(HoverResult { type_str, doc });
                }
                // For params at declaration (not narrowed/reassigned), prefer annotation text
                if kind == "param" && ver_idx == 0 && display_type.is_none()
                    && let Some(ann_text) = self.find_param_annotation_text(symbol_idx) {
                        let optional = self.is_param_optional(symbol_idx) || display_ref.contains_nil();
                        let suffix = if optional { "?" } else { "" };
                        let value_suffix = self.get_string_value(symbol_idx, token_start)
                            .map(|s| format!(" = \"{}\"", s))
                            .or_else(|| self.get_number_value(symbol_idx, token_start)
                                .map(|n| format!(" = {}", n)))
                            .unwrap_or_default();
                        let expansion = self.find_param_annotation_raw(symbol_idx)
                            .and_then(|raw| self.expand_alias_fun_signature(raw));
                        let type_str = match expansion {
                            Some(exp) => format!("({}) {}: {}{}{}\n  = {}", kind, name, ann_text, suffix, value_suffix, exp),
                            None => format!("({}) {}: {}{}{}", kind, name, ann_text, suffix, value_suffix),
                        };
                        return Some(HoverResult { type_str, doc });
                    }
                // For locals assigned from event params (e.g. `local e = event`),
                // show the event type alias (e.g. "WowEvent") instead of "string".
                if !symbol_idx.is_external() && display_type.is_none()
                    && let Some(alias) = self.ir.event_type_display.get(&(symbol_idx, ver_idx))
                {
                    let type_str = format!("({}) {}: {}", kind, name, alias);
                    return Some(HoverResult { type_str, doc });
                }
                // For params that are optional or accept nil, strip nil and show ? suffix.
                // Only check contains_nil() — not is_param_optional() — so that
                // narrowed types (e.g. inside `x and abs(x)`) display without `?`.
                let (final_type, optional_suffix) = if kind == "param" && display_ref.contains_nil() {
                    let stripped = display_ref.strip_nil();
                    if matches!(stripped, ValueType::Nil)
                        || matches!(&stripped, ValueType::Union(v) if v.is_empty())
                    {
                        // Type was only nil (stripped result is nil or never) — don't strip, show as-is
                        (None, "")
                    } else {
                        (Some(stripped), "?")
                    }
                } else {
                    (None, "")
                };
                let type_to_format = final_type.as_ref().unwrap_or(display_ref);
                let value_suffix = self.get_string_value(symbol_idx, token_start)
                    .map(|s| format!(" = \"{}\"", s))
                    .or_else(|| self.get_number_value(symbol_idx, token_start)
                        .map(|n| format!(" = {}", n)))
                    .unwrap_or_default();
                // For tables mutated via bracket assignment, show the constructor's
                // initial element type rather than the post-mutation type.
                let formatted = self.initial_array_display(type_to_format)
                    .unwrap_or_else(|| {
                        let f = self.format_type_accessible(type_to_format, enclosing_class);
                        let type_args = self.get_symbol_type_args(symbol_idx, token_start);
                        self.append_type_args_to_class(&f, type_to_format, &type_args)
                    });
                let mut type_str = format!("({}) {}: {}{}{}", kind, name, formatted, optional_suffix, value_suffix);
                let doc = if let ValueType::Table(Some(table_idx)) = type_to_format {
                    self.append_call_hover(*table_idx, &mut type_str, doc)
                } else {
                    doc
                };
                return Some(HoverResult { type_str, doc });
            }
            return Some(HoverResult { type_str: format!("({}) {}: ?", kind, name), doc: None });
        }
        // Try expression string hover (e.g. hovering over "scanProgress" in Publisher([[scanProgress == 1]]))
        if let Some(result) = self.expression_hover_at(tree, offset) {
            return Some(result);
        }
        // Try event string hover (e.g. hovering over "ENCOUNTER_END" in RegisterEvent("ENCOUNTER_END"))
        if let Some(result) = self.event_string_hover_at(tree, offset) {
            return Some(result);
        }
        // Try `keyof X` string hover (e.g. hovering over "doThing" in RegisterEvent("EVENT", "doThing"))
        if let Some(result) = self.keyof_string_hover_at(tree, offset) {
            return Some(result);
        }
        // Try annotation class/alias name hover (e.g. hovering over "osdateparam" in ---@type osdateparam)
        if let Some(result) = self.annotation_name_hover_at(tree, offset) {
            return Some(result);
        }
        None
    }

    /// Hover on a class or alias name inside an annotation comment.
    pub(super) fn annotation_name_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        let word = self.annotation_word_at(tree, offset)?;
        // Check classes (local + external)
        if let Some(&table_idx) = self.ir.classes.get(&word) {
            let table = self.table(table_idx);
            let has_fields = !table.fields.is_empty() || !table.parent_classes.is_empty();
            let prefix = if table.is_key_enum { "(enum key)" } else if table.enum_kind.is_enum() { "(enum)" } else { "(class)" };
            let type_str = if has_fields {
                format!("{} {}", prefix, self.format_type_accessible(&ValueType::Table(Some(table_idx)), None))
            } else {
                format!("{} {}", prefix, word)
            };
            let doc = self.format_see_doc(&table.see);
            return Some(HoverResult { type_str, doc });
        }
        // Check aliases (local + external)
        if let Some(vt) = self.ir.aliases.get(&word).or_else(|| self.ir.ext.aliases.get(&word)) {
            // Prefer the raw `fun(...)` form from `alias_fun_types` over the resolved
            // `ValueType::Function(None)` which renders as the bare word "function".
            // `expand_alias_fun_signature` walks `alias A = B` chains for us.
            let body = self.ir.alias_fun_types.get(&word)
                .or_else(|| self.ir.ext.alias_fun_types.get(&word))
                .and_then(|raw| self.expand_alias_fun_signature(raw))
                .unwrap_or_else(|| self.format_type(vt));
            let type_str = format!("(alias) {} = {}", word, body);
            return Some(HoverResult { type_str, doc: None });
        }
        // Check parameterized aliases (local + external)
        if let Some((type_params, body)) = self.ir.parameterized_aliases.get(&word)
            .or_else(|| self.ir.ext.parameterized_aliases.get(&word))
        {
            let params_str = type_params.join(", ");
            let body_str = crate::annotations::format_annotation_type(body);
            let type_str = format!("(alias) {}<{}> = {}", word, params_str, body_str);
            return Some(HoverResult { type_str, doc: None });
        }
        None
    }

    /// Find the CallResolution and argument index for a token inside a function/method call.
    /// Returns (arg_index, param_index, call_resolution) where param_index accounts for
    /// the implicit `self` parameter in colon calls.
    pub(super) fn call_resolution_for_arg<'a>(&'a self, token: &SyntaxToken) -> Option<(usize, usize, &'a crate::types::CallResolution)> {
        let call_node = token.ancestors()
            .find(|n| n.kind() == SyntaxKind::FunctionCall || n.kind() == SyntaxKind::MethodCall)?;

        let arg_list = call_node.children()
            .find(|n| n.kind() == SyntaxKind::ArgumentList)?;
        let tok_start = token.text_range().start();
        let mut arg_index = 0usize;
        for child in arg_list.children_with_tokens() {
            if child.text_range().start() >= tok_start {
                break;
            }
            if child.kind() == SyntaxKind::Comma {
                arg_index += 1;
            }
        }

        let call_range = (u32::from(call_node.text_range().start()), u32::from(call_node.text_range().end()));
        let call_res = self.ir.exprs.iter().enumerate()
            .find_map(|(idx, expr)| {
                if let Expr::FunctionCall { call_range: cr, .. } = expr
                    && *cr == call_range
                {
                    self.ir.call_resolutions.get(&ExprId(idx))
                } else {
                    None
                }
            })?;

        let is_colon = call_node.kind() == SyntaxKind::MethodCall
            || FunctionCall::cast(call_node)
                .is_some_and(|c| c.identifier().is_some_and(|id| id.is_call_to_self()));
        let param_index = if is_colon { arg_index + 1 } else { arg_index };

        Some((arg_index, param_index, call_res))
    }

    /// Produce hover info when the cursor is on a transparent `@accessor` token
    /// (e.g. `__private` in `Widget.__private:Method()`).
    pub(super) fn accessor_hover_at(&self, tree: &SyntaxTree, offset: u32, enclosing_class: Option<TableIndex>) -> Option<HoverResult> {
        let text_size = TextSize::from(offset);
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(left, right) => {
                if right.kind() == SyntaxKind::Name { right }
                else if left.kind() == SyntaxKind::Name { left }
                else { return None; }
            }
            TokenAtOffset::None => return None,
        };
        if token.kind() != SyntaxKind::Name {
            return None;
        }
        let parent = token.parent()?;
        if !parent.kind().is_identifier() {
            return None;
        }
        let names: Vec<_> = parent.children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::Name)
            .collect();
        let our_index = names.iter().position(|n| n.text_range() == token.text_range())?;
        if our_index == 0 {
            return None;
        }

        // Resolve root symbol to a table
        let root_name = names[0].text().to_string();
        let scope_idx = self.scope_at_offset(text_size)?;
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name), scope_idx)?;
        let ver = self.sym(symbol_idx).versions.last()?;
        let mut table_idx = Self::extract_table_idx(ver.resolved_type.as_ref()?)?;

        // Walk intermediate fields before our position
        for name_token in &names[1..our_index] {
            let name = name_token.text().to_string();
            if self.ir.has_accessor(table_idx, &name) {
                continue;
            }
            table_idx = self.resolve_field_or_g_env(table_idx, &name)?;
        }

        // Check if our token is an accessor on this table
        let vis = self.ir.get_accessor(table_idx, token.text())?;
        let kind = match vis {
            crate::annotations::Visibility::Public => "accessor",
            crate::annotations::Visibility::Private => "private accessor",
            crate::annotations::Visibility::Protected => "protected accessor",
        };
        // Format the class type the same way as hovering on the base class variable
        let table_type = ValueType::Table(Some(table_idx));
        let formatted = self.format_type_accessible(&table_type, enclosing_class);
        let mut type_str = format!("({}) {}: {}", kind, token.text(), formatted);
        let doc = self.doc_for_type(&table_type);
        let doc = self.append_call_hover(table_idx, &mut type_str, doc);
        Some(HoverResult { type_str, doc })
    }

    pub(super) fn varargs_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        let text_size = TextSize::from(offset);
        let is_vararg = |k: SyntaxKind| k == SyntaxKind::TripleDot || k == SyntaxKind::ParameterVarArgs;
        let token = match SyntaxNode::new_root(tree).token_at_offset(text_size) {
            TokenAtOffset::Single(t) => t,
            TokenAtOffset::Between(left, right) => {
                if is_vararg(right.kind()) { right }
                else if is_vararg(left.kind()) { left }
                else { return None; }
            }
            TokenAtOffset::None => return None,
        };
        if !is_vararg(token.kind()) {
            return None;
        }

        let is_param_decl = token.kind() == SyntaxKind::ParameterVarArgs;
        let func_idx = self.enclosing_function_at(offset)?;
        let func = self.func(func_idx);

        if is_param_decl {
            if let Some(ref ann) = func.vararg_annotation {
                let vararg_text = format_vararg_param(ann);
                let type_str = format!("(param) {}", vararg_text);
                return Some(HoverResult { type_str, doc: func.vararg_description.clone() });
            }
            return Some(HoverResult { type_str: "(param) ...".to_string(), doc: None });
        }

        // Expression site: check event vararg types first, then annotation
        let scope_idx = self.scope_at_offset(text_size)?;
        if let Some(types) = self.find_event_vararg_types_at_scope(scope_idx) {
            let formatted: Vec<String> = types.iter()
                .map(|vt| self.format_type(vt))
                .collect();
            let type_str = format!("(varargs) ...: {}", formatted.join(", "));
            return Some(HoverResult { type_str, doc: None });
        }

        if let Some(ref ann) = func.vararg_annotation {
            let vararg_text = format_vararg_param(ann);
            let type_str = format!("(varargs) {}", vararg_text);
            return Some(HoverResult { type_str, doc: func.vararg_description.clone() });
        }

        Some(HoverResult { type_str: "(varargs) ...: ?".to_string(), doc: None })
    }

    /// Hover for a field access whose receiver type carries the field as an
    /// inline `TableShape` member (cross-file injected-field carrier). Finds the
    /// `Expr::FieldAccess` whose field name range covers `offset`, resolves the
    /// receiver type, and returns `(field_name, field_type)` when a shape member
    /// declares the field. `None` for ordinary class/record fields (handled by
    /// the table-index field-chain path).
    fn shape_field_hover_at(&self, offset: u32) -> Option<(String, ValueType)> {
        for expr in self.ir.exprs.iter() {
            let Expr::FieldAccess { table, field, field_range: Some((s, e)) } = expr else { continue };
            if offset < *s || offset >= *e {
                continue;
            }
            let Some(recv) = self.resolve_expr_type(*table).map(|t| t.into_strip_opaque()) else { continue };
            let mut tys: Vec<ValueType> = Vec::new();
            recv.collect_shape_field_types(field, &mut tys);
            if !tys.is_empty() {
                return Some((field.clone(), ValueType::make_union(tys)));
            }
        }
        None
    }
}
