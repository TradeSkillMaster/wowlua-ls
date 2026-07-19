use super::*;

impl AnalysisResult {
    pub fn signature_help_at(&self, tree: &SyntaxTree, offset: u32) -> Option<SignatureHelpResult> {
        let text_size = TextSize::from(offset);
        let token = SyntaxNode::new_root(tree).token_at_offset(text_size).left_biased()?;

        // Walk up to find the enclosing FunctionCall/MethodCall node
        let call_node = token.ancestors()
            .find(|n| n.kind() == SyntaxKind::FunctionCall || n.kind() == SyntaxKind::MethodCall)?;
        let call = FunctionCall::cast(call_node)?;

        // Only trigger if cursor is within the argument list (at or after the open paren)
        let arg_list = call_node.children()
            .find(|n| n.kind() == SyntaxKind::ArgumentList)?;
        if text_size < arg_list.text_range().start() {
            return None;
        }
        let (active_parameter, total_commas) = {
            let mut commas_before = 0u32;
            let mut total = 0u32;
            for child in arg_list.children_with_tokens() {
                if child.kind() == SyntaxKind::Comma {
                    total += 1;
                    if child.text_range().start() < text_size {
                        commas_before += 1;
                    }
                }
            }
            (commas_before, total)
        };

        // Resolve the function being called
        let ident = call.identifier()?;
        let names = ident.names();
        if names.is_empty() {
            return None;
        }

        let scope_idx = self.scope_at_offset(text_size)?;

        // String literal method call: "str":method() or ("str"):method()
        // names will be just ["method"] with no preceding identifier to look up.
        let string_literal_method = if names.len() == 1
            && call_node.kind() == SyntaxKind::MethodCall
            && Self::resolve_literal_receiver_type(&call_node).is_some()
        {
            let method_name = &names[0];
            self.ir.first_library_table_index(&ValueType::String(None)).and_then(|table_idx| {
                let field_expr = self.get_field(table_idx, method_name)?.expr;
                let ft = self.resolve_expr_type(field_expr)?;
                match ft {
                    ValueType::Function(Some(idx)) => Some(idx),
                    _ => None,
                }
            })
        } else {
            None
        };

        let func_idx = if let Some(idx) = string_literal_method {
            idx
        } else if names.len() == 1 {
            // Simple function call: foo()
            let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
            let ver = self.sym(symbol_idx).versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            match ver {
                ValueType::Function(Some(idx)) => *idx,
                ValueType::FunctionSig(shape) => {
                    // Cross-file inline signature — no arena entry; build the
                    // signature directly from the shape and return early.
                    let shape = shape.clone();
                    let sig = self.build_signature_info_from_shape(&shape, false);
                    let param_count = sig.params.len();
                    let is_vararg = shape.is_vararg;
                    let has_args = arg_list.children().next().is_some();
                    let arg_count = if has_args { (total_commas + 1) as usize } else { 0 };
                    let active_signature = if arg_count == 0 { Some(0) }
                        else { Some(Self::best_matching_signature(&[(param_count, is_vararg)], arg_count) as u32) };
                    return Some(SignatureHelpResult { signatures: vec![sig], active_signature, active_parameter });
                }
                _ => return None,
            }
        } else {
            // Method/field call: obj.method() or obj:method()
            let root_sym = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
            let ver = self.sym(root_sym).versions.iter().rev()
                .find_map(|v| v.resolved_type.as_ref())?;
            let mut table_idx = Self::extract_table_idx(ver)?;
            // Walk intermediate names
            for name in &names[1..names.len()-1] {
                let field_expr = self.get_field(table_idx, name)?.expr;
                let ft = self.resolve_expr_type(field_expr)?;
                table_idx = Self::extract_table_idx(&ft)?;
            }
            let method_name = &names[names.len() - 1];
            let field_expr = self.get_field(table_idx, method_name)?.expr;
            let ft = self.resolve_expr_type(field_expr)?;
            match ft {
                ValueType::Function(Some(idx)) => idx,
                ValueType::FunctionSig(shape) => {
                    // Cross-file inline signature stored in a field/method — build
                    // the signature directly from the shape and return early.
                    let is_colon = ident.is_call_to_self();
                    let sig = self.build_signature_info_from_shape(&shape, is_colon);
                    let param_count = sig.params.len();
                    let is_vararg = shape.is_vararg;
                    let has_args = arg_list.children().next().is_some();
                    let arg_count = if has_args { (total_commas + 1) as usize } else { 0 };
                    let active_signature = if arg_count == 0 { Some(0) }
                        else { Some(Self::best_matching_signature(&[(param_count, is_vararg)], arg_count) as u32) };
                    return Some(SignatureHelpResult { signatures: vec![sig], active_signature, active_parameter });
                }
                _ => return None,
            }
        };

        let func = self.func(func_idx);
        let is_colon = ident.is_call_to_self();

        // Build signatures: primary + overloads
        let mut signatures = Vec::new();
        let mut param_counts: Vec<(usize, bool)> = Vec::new(); // (param_count, is_vararg)

        // Primary signature
        let primary = self.build_signature_info(func, is_colon);
        let primary_param_count = primary.params.len();
        let primary_is_vararg = func.is_vararg;
        signatures.push(primary);
        param_counts.push((primary_param_count, primary_is_vararg));

        // Overload signatures (skip return-only overloads)
        for overload in &func.overloads {
            if overload.is_return_only { continue; }
            let sig = self.build_overload_signature_info(overload, is_colon);
            let param_count = sig.params.len();
            let is_vararg = overload.is_vararg;
            signatures.push(sig);
            param_counts.push((param_count, is_vararg));
        }

        // Select best-matching signature based on total argument count at the call site.
        // Use total commas (not cursor position) so we match the full call's arity.
        // .children() yields only expression nodes (not paren/comma tokens), so
        // this checks whether any argument expressions exist.
        let has_args = arg_list.children().next().is_some();
        let arg_count = if has_args { (total_commas + 1) as usize } else { 0 };
        // When no args typed yet (empty parens), default to showing the primary signature
        let active_signature = if arg_count == 0 {
            Some(0)
        } else {
            Some(Self::best_matching_signature(&param_counts, arg_count) as u32)
        };

        Some(SignatureHelpResult {
            signatures,
            active_signature,
            active_parameter,
        })
    }

    pub(super) fn build_signature_info(&self, func: &Function, skip_self: bool) -> SignatureInfo {
        let args: Vec<(String, Option<String>, Option<String>)> = func.args.iter()
            .enumerate()
            .filter(|&(_, &sym_idx)| {
                if skip_self
                    && let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id {
                        return n != "self";
                    }
                true
            })
            .map(|(i, &sym_idx)| {
                let name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let optional = func.param_optional.get(i).copied().unwrap_or(false);
                let ann_has_nil = func.param_annotations.get(i)
                    .is_some_and(crate::annotations::annotation_type_is_nullable);
                let suffix = if optional && !ann_has_nil { "?" } else { "" };
                let display_name = format!("{}{}", name, suffix);
                // Prefer raw annotation text (preserves alias names) over resolved type
                let type_str = self.param_annotation_text(func, i)
                    .or_else(|| {
                        // Use version 0 only (declaration type from @param), not a
                        // later version from type-guard narrowing in the body.
                        self.sym(sym_idx).versions.first()
                            .and_then(|v| v.resolved_type.as_ref())
                            .map(|rt| {
                                let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                                self.format_type_depth(&display_type, 1)
                            })
                    });
                let desc = func.param_descriptions.get(i).cloned().flatten();
                (display_name, type_str, desc)
            })
            .collect();

        let no_subs = HashMap::new();
        let rets: Vec<String> = if func.returns_self {
            vec![self.self_return_text(func, &no_subs)]
        } else if !func.return_annotations.is_empty() {
            func.return_annotations.iter().enumerate().map(|(i, vt)| {
                let formatted = self.format_value_type_depth(vt, 1);
                format_vararg_return(formatted, i, func)
            }).collect()
        } else {
            self.format_inferred_returns(func, 1)
        };

        let mut params: Vec<String> = args.iter().map(|(name, type_str, _)| {
            match type_str {
                Some(t) => format!("{}: {}", name, t),
                None => name.clone(),
            }
        }).collect();
        let mut param_docs: Vec<Option<String>> = args.iter().map(|(_, _, desc)| desc.clone()).collect();
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            params.push(vararg_str);
            param_docs.push(func.vararg_description.clone());
        }

        let label = if rets.is_empty() {
            format!("fun({})", params.join(", "))
        } else {
            format!("fun({}): {}", params.join(", "), join_returns(&rets))
        };

        SignatureInfo { label, params, param_docs, doc: func.doc.clone() }
    }

    pub(super) fn build_overload_signature_info(&self, overload: &ResolvedOverload, skip_self: bool) -> SignatureInfo {
        let params: Vec<String> = overload.params.iter()
            .filter(|p| !(skip_self && p.name == "self"))
            .map(|p| {
                match &p.typ {
                    Some(vt) => format!("{}: {}", p.name, self.format_value_type_depth(vt, 1)),
                    None => p.name.clone(),
                }
            }).collect();

        let rets: Vec<String> = overload.returns.iter()
            .map(|vt| self.format_value_type_depth(vt, 1))
            .collect();

        let label = if rets.is_empty() {
            format!("fun({})", params.join(", "))
        } else {
            format!("fun({}): {}", params.join(", "), join_returns(&rets))
        };

        let param_docs = vec![None; params.len()];
        SignatureInfo { label, params, param_docs, doc: None }
    }

    /// Build a [`SignatureInfo`] from an inline [`crate::types::FunctionShape`]
    /// (carried by `ValueType::FunctionSig`). Used when the callee resolves to a
    /// cross-file returned function whose signature is stored inline rather than
    /// in the per-file function arena.
    pub(super) fn build_signature_info_from_shape(
        &self,
        shape: &crate::types::FunctionShape,
        skip_self: bool,
    ) -> SignatureInfo {
        let mut params: Vec<String> = shape.params.iter()
            .filter(|p| !(skip_self && p.name == "self"))
            .map(|p| {
                let suffix = if p.optional { "?" } else { "" };
                format!("{}{}: {}", p.name, suffix, self.format_type_depth(&p.ty, 1))
            })
            .collect();
        if shape.is_vararg {
            params.push("...".to_string());
        }
        let rets: Vec<String> = shape.returns.iter()
            .map(|r| self.format_value_type_depth(r, 1))
            .collect();
        let label = if rets.is_empty() {
            format!("fun({})", params.join(", "))
        } else {
            format!("fun({}): {}", params.join(", "), join_returns(&rets))
        };
        let param_docs = vec![None; params.len()];
        SignatureInfo { label, params, param_docs, doc: None }
    }

    /// Pick the signature index whose parameter count best matches the number of
    /// arguments being typed. Prefers exact non-vararg match, then vararg match,
    /// then smallest count >= arg_count, then falls back to the largest count.
    pub(super) fn best_matching_signature(param_counts: &[(usize, bool)], arg_count: usize) -> usize {
        if param_counts.len() <= 1 {
            return 0;
        }
        let mut best = 0usize;
        let mut best_score = u32::MAX;
        for (i, &(count, is_vararg)) in param_counts.iter().enumerate() {
            let score = if is_vararg && arg_count >= count {
                // Vararg can accept extra args, but prefer exact non-vararg matches
                1
            } else if count == arg_count {
                0 // exact match
            } else if count > arg_count {
                // Can accept all args — prefer closer counts
                (count - arg_count) as u32
            } else {
                // Too few params — heavily penalize
                1000 + (arg_count - count) as u32
            };
            if score < best_score {
                best_score = score;
                best = i;
            }
        }
        best
    }
}
