use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken, TextSize};
use crate::types::*;
use super::{AnalysisResult, DeferredChecks};

// ── Deferred Diagnostic Checks ──────────────────────────────────────────────────

impl AnalysisResult {
    /// Run all diagnostic checks against the resolved analysis state.
    /// This is a pure function — it reads the resolved AnalysisResult and
    /// DeferredChecks, and returns collected diagnostics.
    pub fn run_diagnostics(
        &self,
        tree: &SyntaxTree,
        mut deferred: DeferredChecks,
    ) -> Vec<crate::diagnostics::WowDiagnostic> {
        if self.is_meta { return Vec::new(); }
        let mut diags = Vec::new();
        // unknown-* checks read deferred non-destructively (must run before drains)
        self.check_unknown_param_type_diagnostics(tree, &deferred, &mut diags);
        self.check_unknown_local_type_diagnostics(tree, &mut diags);
        self.check_unknown_return_type_diagnostics(&deferred, &mut diags);
        self.check_unknown_field_type_diagnostics(&mut diags);
        self.check_undefined_field_diagnostics(&mut diags);
        self.check_return_type_diagnostics(&mut deferred, &mut diags);
        self.check_field_type_diagnostics(&mut deferred, &mut diags);
        self.check_assign_type_diagnostics(&mut deferred, &mut diags);
        self.check_access_diagnostics(tree, &mut diags);
        self.check_nil_diagnostics(&mut deferred, &mut diags);
        self.check_undefined_global_diagnostics(tree, &mut diags);
        self.check_create_global_diagnostics(tree, &mut diags);
        self.check_unused_local_diagnostics(tree, &mut diags);
        self.check_duplicate_set_field_diagnostics(&mut deferred, &mut diags);
        self.check_missing_fields_diagnostics(&mut deferred, &mut diags);
        self.check_grouped_return_diagnostics(&mut deferred, &mut diags);
        self.check_missing_return_diagnostics(tree, &mut diags);
        self.check_incomplete_signature_doc_diagnostics(tree, &mut diags);
        self.check_diagnostic_codes(tree, &mut diags);
        self.check_annotation_validation_diagnostics(&mut deferred, &mut diags);
        self.check_duplicate_index_diagnostics(tree, &mut diags);
        self.check_malformed_annotations(tree, &mut diags);
        self.check_annotation_metadata_diagnostics(tree, &mut deferred, &mut diags);
        self.check_ast_diagnostics(tree, &mut diags);
        self.check_redefined_local_diagnostics(tree, &mut diags);
        self.check_return_count_diagnostics(&mut deferred, &mut diags);
        self.check_redundant_param_diagnostics(&mut deferred, &mut diags);
        self.check_missing_param_diagnostics(&mut deferred, &mut diags);
        self.check_arg_type_mismatch_diagnostics(&mut deferred, &mut diags);
        self.check_nil_callee_diagnostics(&mut diags);
        self.check_multi_return_projection_diagnostics(&mut deferred, &mut diags);
        self.check_inject_field_diagnostics(&mut deferred, &mut diags);
        self.check_discard_returns_diagnostics(&mut diags);
        self.check_wrong_flavor_api_diagnostics(&mut diags);

        // Remove undefined-doc-class / undefined-doc-name diagnostics for types
        // registered during resolution (e.g. @built-name classes discovered during
        // the fixpoint loop).
        diags.retain(|d| {
            let name_opt = if d.code == crate::diagnostics::undefined_doc_class::CODE {
                crate::diagnostics::undefined_doc_class::extract_name(&d.message)
            } else if d.code == crate::diagnostics::undefined_doc_name::CODE {
                crate::diagnostics::undefined_doc_name::extract_name(&d.message)
            } else {
                None
            };
            if let Some(name) = name_opt {
                if self.ir.classes.contains_key(name) || self.ir.ext.classes.contains_key(name) {
                    return false;
                }
                if self.ir.aliases.contains_key(name) || self.ir.ext.aliases.contains_key(name) {
                    return false;
                }
                if self.ir.parameterized_aliases.contains_key(name)
                    || self.ir.ext.parameterized_aliases.contains_key(name)
                {
                    return false;
                }
            }
            true
        });

        // Deduplicate diagnostics
        {
            let mut seen = std::collections::HashSet::new();
            diags.retain(|d| seen.insert((d.code, d.start, d.end)));
        }

        // Emit a visible diagnostic if a safety limit was hit
        if let Some(ref msg) = self.safety_limit_hit {
            diags.push(crate::diagnostics::WowDiagnostic {
                code: "safety-limit",
                message: format!("analysis incomplete: {msg}; some types and diagnostics may be missing"),
                severity: lsp_types::DiagnosticSeverity::ERROR,
                start: 0,
                end: 0,
            });
        }

        diags
    }

    fn check_undefined_field_diagnostics(&self, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        for expr in self.ir.exprs.iter() {
            let Expr::FieldAccess { table, field, field_range } = expr else { continue };
            let Some((start, end)) = field_range else { continue };
            let Some(table_type) = self.resolve_expr_type(*table) else { continue };
            if matches!(table_type, ValueType::Any) { continue; }
            let table_indices: Vec<TableIndex> = match &table_type {
                ValueType::Table(Some(idx)) => vec![*idx],
                ValueType::Union(types) => types.iter().filter_map(|t| match t {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                }).collect(),
                _ => continue,
            };
            if table_indices.is_empty() { continue; }
            // Only emit when at least one table is a @class (matches build-time gate).
            if !table_indices.iter().any(|&idx| self.table(idx).class_name.is_some()) { continue; }
            // Does the field exist directly?
            if table_indices.iter().any(|&idx| self.ir.has_field(idx, field)) { continue; }
            // Check parent classes
            if table_indices.iter().any(|&idx| {
                self.table(idx).parent_classes.iter().any(|&pi| self.ir.has_field(pi, field))
            }) { continue; }
            let first_idx = table_indices[0];
            if let Some(class_name) = self.table(first_idx).class_name.clone() {
                crate::diagnostics::undefined_field::check(
                    diags,
                    field, &class_name,
                    *start as usize, *end as usize,
                );
            }
        }
    }

    fn check_return_type_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.return_type_checks);
        for ReturnTypeCheck { func_id, ret_index, rhs_expr, scope_idx, start, end } in checks {
            // Explicitly void function (e.g. inline callback with fun(x: number) annotation)
            if self.ir.functions[func_id.val()].explicit_void_return {
                crate::diagnostics::redundant_return_value::check(
                    diags,
                    0, ret_index + 1,
                    start as usize, end as usize,
                );
                continue;
            }
            let Some(expected) = self.ir.functions[func_id.val()].return_annotations.get(ret_index).cloned() else { continue };
            let Some(actual) = self.resolve_expr_type(rhs_expr) else { continue };
            // Apply narrowing from assert/if guards
            let actual = if actual.contains_nil() || matches!(&actual, ValueType::Union(ts) if ts.contains(&ValueType::Boolean(Some(false)))) {
                if let Some(sym_idx) = self.ir.find_root_symbol(rhs_expr) {
                    if self.is_symbol_falsy_narrowed(sym_idx, scope_idx) {
                        actual.strip_falsy()
                    } else if self.is_symbol_narrowed(sym_idx, scope_idx) {
                        actual.strip_nil()
                    } else if let Some((_, chain)) = self.ir.extract_field_chain(rhs_expr) {
                        if self.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                            actual.strip_nil()
                        } else { actual }
                    } else { actual }
                } else { actual }
            } else { actual };
            // If this function has return-only overloads that allow nil at this
            // ret_index, strip nil from the actual type before comparing — the
            // overload already accounts for the nil return path.
            let actual = if actual.contains_nil() && self.ir.functions[func_id.val()].return_overload_may_nil(ret_index) {
                actual.strip_nil()
            } else { actual };
            if actual.is_assignable_to(&expected) {
                continue;
            }
            if self.is_table_subtype(&actual, &expected) {
                self.check_excess_structural_fields(deferred, &actual, &expected, start as usize, end as usize);
                continue;
            }
            let expected_str = self.format_value_type_depth(&expected, 1);
            let actual_str = self.format_value_type_depth(&actual, 1);
            crate::diagnostics::return_mismatch::check(
                diags,
                &expected_str, &actual_str,
                start as usize, end as usize,
            );
        }
    }

    // ── Field type diagnostics ──────────────────────────────────────────────────

    fn check_field_type_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.field_type_checks);
        for FieldTypeCheck { expected, actual_expr, field_name, start, end, lateinit } in checks {
            let Some(actual) = self.resolve_expr_type(actual_expr) else { continue };
            // Allow nil or T|nil assignment to lateinit (T!) fields
            if lateinit {
                if matches!(actual, ValueType::Nil) { continue; }
                let stripped = actual.strip_nil();
                if stripped.is_assignable_to(&expected) { continue; }
                if self.is_table_subtype(&stripped, &expected) { continue; }
            }
            if actual.is_assignable_to(&expected) {
                continue;
            }
            if self.is_table_subtype(&actual, &expected) {
                self.check_excess_structural_fields(deferred, &actual, &expected, start as usize, end as usize);
                continue;
            }
            let expected_str = self.format_value_type_depth(&expected, 1);
            let actual_str = self.format_value_type_depth(&actual, 1);
            crate::diagnostics::field_type_mismatch::check(
                diags,
                &field_name, &expected_str, &actual_str,
                start as usize, end as usize,
            );
        }
    }

    // ── Access diagnostics ──────────────────────────────────────────────────────

    /// Walk all Identifier nodes looking for field accesses to private/protected fields.
    fn check_access_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        use crate::ast::{AstNode, Identifier};

        for ident_node in SyntaxNode::new_root(tree).descendants()
            .filter(|n| n.kind() .is_identifier()) {
            let Some(ident) = Identifier::cast(ident_node) else { continue };
            let names = ident.names();
            if names.len() < 2 { continue; }

            // For each non-root Name in the chain, check access.
            // In parser2's DotAccess tree, names are nested inside child NameRef/DotAccess nodes.
            // Recursively collect Name tokens in identifier-chain order.
            let name_tokens = Self::collect_name_tokens_recursive(ident_node);
            if name_tokens.len() < 2 { continue; }

            // Resolve the root to a table
            let root_token = &name_tokens[0];
            let root_offset = TextSize::from(u32::from(root_token.text_range().start()));
            let Some(scope_idx) = self.scope_at_offset(root_offset) else { continue };
            let Some(root_sym) = self.get_symbol(&SymbolIdentifier::Name(root_token.text().to_string()), scope_idx) else { continue };
            let Some(ver) = self.sym(root_sym).versions.last() else { continue };
            let Some(ValueType::Table(Some(start_table_idx))) = ver.resolved_type.as_ref() else { continue };
            let mut table_idx = *start_table_idx;

            for i in 1..name_tokens.len() {
                let field_name = name_tokens[i].text().to_string();

                // Skip transparent @accessor names
                if self.ir.has_accessor(table_idx, &field_name) {
                    continue;
                }

                let field_vis = self.get_field(table_idx, &field_name).map(|f| f.visibility);

                if let Some(vis) = field_vis
                    && vis != crate::annotations::Visibility::Public
                        && self.table(table_idx).class_name.is_some()
                    {
                        let enclosing_class = self.find_enclosing_class(&ident_node);
                        let same_class = enclosing_class.is_some_and(|ec| self.same_class(ec, table_idx));
                        let mut is_subclass = enclosing_class.is_some_and(|ec| self.is_subclass_of(ec, table_idx));
                        // If the root variable is a defclass-created instance in this file,
                        // allow protected access at file scope (e.g. CancelScan:OnModuleLoad()).
                        // Private access still requires being inside a colon method.
                        if !is_subclass && vis == crate::annotations::Visibility::Protected {
                            let root_name = root_token.text().to_string();
                            if let Some(&dc_table) = self.defclass_vars.get(&root_name) {
                                is_subclass = self.is_subclass_of(dc_table, table_idx);
                            }
                        }
                        let range = name_tokens[i].text_range();
                        crate::diagnostics::access::check(
                            diags, vis, same_class, is_subclass,
                            &field_name,
                            u32::from(range.start()) as usize,
                            u32::from(range.end()) as usize,
                        );
                    }

                // Walk to next table in the chain
                if i < name_tokens.len() - 1 {
                    let Some(field_expr_id) = self.get_field(table_idx, &field_name).map(|f| f.expr) else { break };
                    let Some(ValueType::Table(Some(next_idx))) = self.resolve_expr_type(field_expr_id) else { break };
                    table_idx = next_idx;
                }
            }
        }
    }

    /// Recursively collect Name tokens from an identifier node in left-to-right order.
    /// In parser2's DotAccess tree, names are nested inside child NameRef/DotAccess nodes
    /// rather than being direct children. This function walks the identifier chain to
    /// collect all Name tokens at any depth (for identifier-like nodes only).
    fn collect_name_tokens_recursive<'b>(node: SyntaxNode<'b>) -> Vec<SyntaxToken<'b>> {
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

    /// Emit inject-field diagnostics for excess fields in a table literal that
    /// structurally matched a @class type. Call after is_table_subtype succeeds.
    /// Pushes new entries into `deferred.inject_field_checks`, so this MUST run
    /// before `check_inject_field_diagnostics` drains that vec — the ordering in
    /// `run_diagnostics` is load-bearing.
    fn check_excess_structural_fields(
        &self,
        deferred: &mut DeferredChecks,
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
        let expected_names: HashSet<&str> = expected_fields.iter().map(|(n, _)| n.as_str()).collect();

        let excess: Vec<String> = self.table(actual_idx).fields.keys()
            .filter(|name| !expected_names.contains(name.as_str()))
            .cloned()
            .collect();

        for field_name in excess {
            deferred.inject_field_checks.push(InjectFieldCheck {
                table_idx: expected_idx, field_name, scope_idx: ScopeIndex(0),
                start: range_start as u32, end: range_end as u32,
                field_existed_at_build: false,
            });
        }
    }

    /// Collect all fields for a @class table, including inherited fields from parents.
    fn collect_class_fields(&self, table_idx: TableIndex) -> Vec<(String, ValueType)> {
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
        let actual_args = self.func(*actual_idx).args.clone();
        let actual_is_vararg = self.func(*actual_idx).is_vararg;
        let actual_rets = self.func(*actual_idx).return_annotations.clone();
        let expected_args = self.func(*expected_idx).args.clone();
        let expected_is_vararg = self.func(*expected_idx).is_vararg;
        let expected_rets = self.func(*expected_idx).return_annotations.clone();
        // Arity: fewer-params is fine; more-params is not. Vararg on EXPECTED
        // allows the actual to exceed expected.args.len() (any extras absorbed).
        // Vararg on ACTUAL doesn't exceed the declared params count, so still OK.
        if !expected_is_vararg && !actual_is_vararg
            && actual_args.len() > expected_args.len() {
                return false;
            }
        // Param types: for each position actual declares (skipping self for colon
        // methods — detected by param name), compare against expected's param at
        // the same position. If expected has no param at that position (actual
        // over-declares AND expected is vararg), use expected's vararg_annotation
        // resolved type if available, otherwise treat as any.
        let actual_skip_self = actual_args.first()
            .and_then(|&idx| match &self.sym(idx).id {
                SymbolIdentifier::Name(n) if n == "self" => Some(1),
                _ => None,
            })
            .unwrap_or(0);
        let expected_skip_self = expected_args.first()
            .and_then(|&idx| match &self.sym(idx).id {
                SymbolIdentifier::Name(n) if n == "self" => Some(1),
                _ => None,
            })
            .unwrap_or(0);
        let actual_params = &actual_args[actual_skip_self..];
        let expected_params = &expected_args[expected_skip_self..];
        for (pos, &actual_sym) in actual_params.iter().enumerate() {
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

    fn check_undefined_global_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            if node.kind() != SyntaxKind::NameRef { continue; }
            // Skip NameRefs in non-expression positions (assignment LHS, local-decl name list).
            if has_ancestor_of_kind(&node, &[SyntaxKind::VariableList, SyntaxKind::NameList]) { continue; }
            let Some(token) = node.children_with_tokens()
                .filter_map(|t| t.into_token())
                .find(|t| t.kind() == SyntaxKind::Name)
            else { continue };
            let name = token.text().to_string();
            if self.allowed_read_globals.contains(&name) || self.allowed_write_globals.contains(&name) {
                continue;
            }
            let r = token.text_range();
            let offset = u32::from(r.start());
            let scope_idx = self.scope_at_offset(offset).unwrap_or(ScopeIndex(0));
            if self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_none() {
                crate::diagnostics::undefined_global::check(
                    diags, &name,
                    u32::from(r.start()) as usize,
                    u32::from(r.end()) as usize,
                );
            }
        }
    }

    fn check_create_global_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        for sym in &self.ir.symbols {
            if sym.scope_idx != ScopeIndex(0) { continue; }
            let name = match &sym.id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => continue,
            };
            if self.allowed_write_globals.contains(&name) { continue; }
            if self.ir.ext.scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) { continue; }
            if self.ir.framexml_enabled
                && self.ir.ext.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) { continue; }
            if name.starts_with('_') { continue; }
            // First version's def_node is the creation site. Skip if it's a local declaration.
            let Some(first_ver) = sym.versions.first() else { continue };
            let def_start = first_ver.def_node.start;
            let def_end = first_ver.def_node.end;
            if self.is_local_declaration_site(tree, def_start) { continue; }
            let Some(range) = self.def_name_token_range(tree, def_start, def_end, &name) else { continue };
            crate::diagnostics::create_global::check(
                diags, &name,
                u32::from(range.start()) as usize,
                u32::from(range.end()) as usize,
            );
        }
    }

    fn check_unused_local_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        for (sym_idx, name, range) in self.iter_local_def_sites(tree) {
            if self.referenced_symbols.contains(&sym_idx) { continue; }
            // Skip underscore-prefixed names (Lua convention for intentionally unused)
            if name.starts_with('_') { continue; }
            let start = u32::from(range.start()) as usize;
            let end = u32::from(range.end()) as usize;
            // Emit more specific unused-function for function definitions
            let is_func = self.ir.symbols[sym_idx.val()].versions.last()
                .and_then(|v| v.type_source)
                .map(|e| matches!(self.expr(e), Expr::FunctionDef(_)))
                .unwrap_or(false);
            if is_func {
                crate::diagnostics::unused_function::check(diags, &name, start, end);
            } else {
                crate::diagnostics::unused_local::check(diags, &name, start, end);
            }
        }
    }

    /// Walk all symbols whose first version's def_node is a local declaration
    /// (excluding function parameters). Yields (sym_idx, name, name-token range).
    fn iter_local_def_sites<'a>(
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

    fn check_duplicate_set_field_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let sites = std::mem::take(&mut deferred.field_assignment_sites);
        // Track (table_idx, field_name, scope_idx) -> index in sites vec
        let mut seen: HashMap<(TableIndex, String, ScopeIndex), usize> = HashMap::new();
        for (i, site) in sites.iter().enumerate() {
            let FieldAssignmentSite { table_idx, field_name, scope_idx, block_stmt_index, start, end } = site;
            // Only check @class tables
            let class_name = match &self.table(*table_idx).class_name {
                Some(n) => n.clone(),
                None => continue,
            };
            let key = (*table_idx, field_name.clone(), *scope_idx);
            if let Some(&first_idx) = seen.get(&key) {
                // Two guards prevent false positives:
                // 1. Bracket pattern: don't flag when other fields on the same
                //    table are set between the two assignments (e.g.
                //    state.flag = true; state.other = ...; state.flag = false).
                let has_intervening = sites[first_idx + 1..i].iter().any(|s| {
                    s.table_idx == *table_idx && s.scope_idx == *scope_idx && s.field_name != *field_name
                });
                // 2. Runtime re-assignment: don't flag when there are non-field-
                //    assignment statements (function calls, control flow, etc.)
                //    between the two assignments.
                let stmt_gap = *block_stmt_index as usize - sites[first_idx].block_stmt_index as usize;
                let intervening_in_scope = sites[first_idx + 1..i].iter()
                    .filter(|s| s.scope_idx == *scope_idx)
                    .count();
                let all_intervening_are_field_assigns = stmt_gap == intervening_in_scope + 1;
                if !has_intervening && all_intervening_are_field_assigns {
                    crate::diagnostics::duplicate_set_field::check(
                        diags,
                        field_name, &class_name,
                        *start as usize, *end as usize,
                    );
                }
                seen.insert(key, i);
            } else {
                seen.insert(key, i);
            }
        }
    }

    fn check_assign_type_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.assign_type_checks);
        for AssignTypeCheck { expected, actual_expr, var_name, start, end } in checks {
            let Some(actual) = self.resolve_expr_type(actual_expr) else { continue };
            if actual.is_assignable_to(&expected) {
                continue;
            }
            if self.is_table_subtype(&actual, &expected) {
                self.check_excess_structural_fields(deferred, &actual, &expected, start as usize, end as usize);
                continue;
            }
            let expected_str = self.format_value_type_depth(&expected, 1);
            let actual_str = self.format_value_type_depth(&actual, 1);
            crate::diagnostics::assign_type_mismatch::check(
                diags,
                &var_name, &expected_str, &actual_str,
                start as usize, end as usize,
            );
        }
    }

    fn check_nil_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.nil_check_sites);
        let mut seen = HashSet::new();
        for NilCheckSite { scope_idx, table_expr: table_expr_id, start, end } in checks {
            if !seen.insert((start, end)) { continue; }
            let Some(vt) = self.resolve_expr_type(table_expr_id) else { continue };
            let is_nullable = match &vt {
                ValueType::Union(types) => types.contains(&ValueType::Nil),
                ValueType::Nil => true,
                _ => false,
            };
            if !is_nullable { continue; }

            if let Some(sym_idx) = self.ir.find_root_symbol(table_expr_id) {
                if self.is_symbol_narrowed(sym_idx, scope_idx) {
                    continue;
                }
                // Check field-level narrowing (e.g. assert(self.field) or if self.a.b then)
                if let Some((_, chain)) = self.ir.extract_field_chain(table_expr_id)
                    && self.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                        continue;
                    }
            }

            let type_str = self.format_value_type_depth(&vt, 0);
            crate::diagnostics::need_check_nil::check(
                diags,
                &type_str,
                start as usize, end as usize,
            );
        }
    }

    fn check_missing_return_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        for func_idx in 0..self.ir.functions.len() {
            let func = &self.ir.functions[func_idx];
            if func.return_annotations.is_empty() { continue; }
            // All-optional returns: falling off the end returns nil, which matches Type?
            if func.return_annotations.iter().all(|t| t.contains_nil()) { continue; }
            let func_node = if let Some(nid) = func.def_node.node_id {
                SyntaxNode { tree, id: nid }
            } else {
                // Fallback for external nodes without NodeId (should not happen for local functions)
                continue;
            };
            let Some(block) = func_node.children().find_map(Block::cast) else { continue };
            if !Self::block_ends_with_return(&block) {
                let r = func_node.text_range();
                // Highlight just the first line (function signature)
                let start = u32::from(r.start()) as usize;
                let end = std::cmp::min(start + 40, u32::from(r.end()) as usize);
                crate::diagnostics::missing_return::check(
                    diags,
                    start, end,
                );
            }
        }
    }

    fn check_annotation_validation_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.annotation_validation_checks);
        for check in checks {
            diags.push(crate::diagnostics::WowDiagnostic {
                code: check.code,
                message: check.message,
                severity: check.severity,
                start: check.start as usize,
                end: check.end as usize,
            });
        }
    }

    fn check_duplicate_index_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            if node.kind() != SyntaxKind::TableConstructor { continue; }
            let Some(tc) = TableConstructor::cast(node) else { continue };
            let mut seen: HashSet<String> = HashSet::new();
            for field in tc.fields() {
                let name = match field.kind() {
                    Some(FieldKind::Named { name, .. }) => Some(name),
                    None => extract_bracket_string_key(&field.syntax()),
                    _ => None,
                };
                let Some(name) = name else { continue };
                if !seen.insert(name.clone()) {
                    let r = field.syntax().text_range();
                    crate::diagnostics::duplicate_index::check(
                        diags,
                        &name,
                        u32::from(r.start()) as usize,
                        u32::from(r.end()) as usize,
                    );
                }
            }
        }
    }

    fn check_malformed_annotations(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        const KNOWN_TAGS: &[&str] = &[
            "class", "field", "alias", "param", "return", "type", "enum",
            "meta", "overload", "defclass", "deprecated", "nodiscard", "constructor",
            "generic", "private", "protected", "accessor", "diagnostic",
            "builds-field", "built-name", "built-extends", "type-narrows",
            "correlated", "flavor-narrows",
            "see", "vararg", "as", "cast", "operator", "module", "source",
            "version", "package", "async", "nodoc", "public",
        ];

        let mut current_class: Option<&str> = None;

        for event in SyntaxNode::new_root(tree).descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment {
                // Reset class tracking when we leave a comment block
                if tok.kind() != SyntaxKind::Whitespace && tok.kind() != SyntaxKind::Newline {
                    current_class = None;
                }
                continue;
            }
            let text = tok.text();
            let Some(after_at) = text.strip_prefix("---@") else { continue };
            // Skip @diagnostic — handled by check_diagnostic_codes
            if after_at.starts_with("diagnostic") { continue; }

            let r = tok.text_range();
            let tok_start = u32::from(r.start()) as usize;
            let tok_end = u32::from(r.end()) as usize;

            // Extract the tag name (first word after @)
            let tag = after_at.split(|c: char| c.is_whitespace()).next().unwrap_or("");
            if tag.is_empty() { continue; }

            // Check if the tag is known
            if !KNOWN_TAGS.contains(&tag) {
                // Offset of the tag within the token: "---@" is 4 bytes
                let tag_start = tok_start + 4;
                let tag_end = tag_start + tag.len();
                crate::diagnostics::malformed_annotation::check(
                    diags,
                    format!("unknown annotation '@{}'", tag),
                    tag_start, tag_end,
                );
                continue;
            }

            // Check for known tags that are missing required content
            let rest = after_at[tag.len()..].trim();

            // Track the current @class/@enum for @correlated field validation
            if (tag == "class" || tag == "enum") && !rest.is_empty() {
                let name = rest.split(|c: char| c.is_whitespace() || c == '<' || c == ':').next().unwrap_or("");
                if !name.is_empty() {
                    current_class = Some(name);
                }
            }

            let msg = match tag {
                "class" | "enum" if rest.is_empty() || rest.split_whitespace().next().is_none() =>
                    Some(format!("@{} requires a name", tag)),
                "param" if rest.is_empty() =>
                    Some("@param requires a name and type".to_string()),
                "param" if !rest.contains(char::is_whitespace) =>
                    Some("@param requires a type after the parameter name".to_string()),
                "field" => {
                    // Strip optional visibility prefix
                    let rest = rest.strip_prefix("private").map(|r| r.trim_start())
                        .or_else(|| rest.strip_prefix("protected").map(|r| r.trim_start()))
                        .or_else(|| rest.strip_prefix("public").map(|r| r.trim_start()))
                        .unwrap_or(rest);
                    if rest.is_empty() {
                        Some("@field requires a name and type".to_string())
                    } else if !rest.contains(char::is_whitespace) {
                        Some("@field requires a type after the field name".to_string())
                    } else {
                        None
                    }
                }
                "alias" if rest.is_empty() =>
                    Some("@alias requires a name and type".to_string()),
                "alias" if !rest.contains(char::is_whitespace) => {
                    // Name-only @alias is valid when followed by ---| continuation lines
                    let has_continuation = {
                        let mut next = tok.next_token();
                        let mut found = false;
                        while let Some(ref t) = next {
                            let k = t.kind();
                            if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline {
                                next = t.next_token();
                                continue;
                            }
                            if k == SyntaxKind::Comment && t.text().starts_with("---|") {
                                found = true;
                            }
                            break;
                        }
                        found
                    };
                    if has_continuation { None }
                    else { Some("@alias requires a type after the alias name".to_string()) }
                }
                "cast" if rest.is_empty() =>
                    Some("@cast requires a variable name and type".to_string()),
                "cast" if !rest.contains(char::is_whitespace) =>
                    Some("@cast requires a type after the variable name".to_string()),
                "type" if rest.is_empty() =>
                    Some("@type requires a type".to_string()),
                "return" if rest.is_empty() =>
                    Some("@return requires a type".to_string()),
                "overload" if rest.is_empty() =>
                    Some("@overload requires a 'fun(...)' signature".to_string()),
                "overload" if !rest.starts_with("fun(") =>
                    Some("@overload requires a 'fun(...)' signature".to_string()),
                "builds-field" => {
                    if rest.is_empty() {
                        Some("@builds-field requires a parameter index and type (e.g. @builds-field 1 string)".to_string())
                    } else if !rest.contains(char::is_whitespace) {
                        // Has index but no type
                        if rest.parse::<usize>().is_err() {
                            Some("@builds-field requires a numeric parameter index (e.g. @builds-field 1 string)".to_string())
                        } else {
                            Some("@builds-field requires a type after the parameter index (e.g. @builds-field 1 string)".to_string())
                        }
                    } else {
                        let idx_str = rest.split_whitespace().next().unwrap_or("");
                        if idx_str.parse::<usize>().is_err() {
                            Some("@builds-field requires a numeric parameter index (e.g. @builds-field 1 string)".to_string())
                        } else if idx_str == "0" {
                            Some("@builds-field parameter index must be >= 1 (1-based)".to_string())
                        } else {
                            None
                        }
                    }
                }
                "built-name" => {
                    if rest.is_empty() {
                        Some("@built-name requires a parameter index (e.g. @built-name 1)".to_string())
                    } else if let Ok(idx) = rest.trim().parse::<usize>() {
                        if idx == 0 {
                            Some("@built-name parameter index must be >= 1 (1-based)".to_string())
                        } else {
                            None
                        }
                    } else {
                        Some("@built-name requires a numeric parameter index (e.g. @built-name 1)".to_string())
                    }
                }
                "correlated" => {
                    if rest.is_empty() {
                        Some("@correlated requires at least two field names (e.g. @correlated field1, field2)".to_string())
                    } else {
                        let names: Vec<&str> = rest.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
                        if names.len() < 2 {
                            Some("@correlated requires at least two field names (e.g. @correlated field1, field2)".to_string())
                        } else {
                            None
                        }
                    }
                }
                "flavor-narrows" => {
                    if rest.is_empty() {
                        Some("@flavor-narrows requires one or more flavor names (e.g. @flavor-narrows retail, classic)".to_string())
                    } else {
                        let unknown: Vec<&str> = rest.split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty() && crate::flavor::parse_flavor_name(s).is_none())
                            .collect();
                        if !unknown.is_empty() {
                            Some(format!("@flavor-narrows has unknown flavor name(s): {}", unknown.join(", ")))
                        } else {
                            None
                        }
                    }
                }
                _ => None,
            };
            if let Some(message) = msg {
                // Underline the annotation tag
                let tag_start = tok_start + 4; // "---@"
                let tag_end = tag_start + tag.len();
                crate::diagnostics::malformed_annotation::check(
                    diags,
                    message,
                    tag_start, std::cmp::min(tag_end, tok_end),
                );
            } else if tag == "correlated" {
                // Validate @correlated field names against the post-resolve class table,
                // which includes builder-pattern fields and inherited fields.
                if let Some(class_name) = current_class
                    && let Some(&table_idx) = self.ir.classes.get(class_name) {
                        // Compute base offset of `rest` within the token
                        let rest_offset = tok_start + 4 + tag.len() + (after_at[tag.len()..].len() - rest.len());
                        for segment in rest.split(',') {
                            let field_name = segment.trim();
                            if field_name.is_empty() { continue; }
                            if !self.class_has_field(table_idx, field_name) {
                                // Offset of this segment within rest, plus trim offset within segment
                                let seg_start_in_rest = segment.as_ptr() as usize - rest.as_ptr() as usize;
                                let trim_offset = segment.len() - segment.trim_start().len();
                                let field_start = rest_offset + seg_start_in_rest + trim_offset;
                                let field_end = field_start + field_name.len();
                                crate::diagnostics::malformed_annotation::check(
                                    diags,
                                    format!("@correlated references unknown field '{}' on class '{}'", field_name, class_name),
                                    field_start, field_end,
                                );
                            }
                        }
                    }
            }
        }
    }

    /// Check if a field with an annotation exists on a class table, its built table, or parents.
    fn class_has_annotated_field(&self, table_idx: TableIndex, field_name: &str) -> bool {
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

    fn check_diagnostic_codes(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        use crate::diagnostics::KNOWN_CODES;
        for event in SyntaxNode::new_root(tree).descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            let Some(rest) = text.strip_prefix("---@diagnostic") else { continue };
            let rest = rest.trim();
            // Find codes after the colon
            let Some((_keyword, codes_str)) = rest.split_once(':') else {
                // No colon — warn if it looks like codes follow the keyword
                if let Some(space_pos) = rest.find(|c: char| c.is_whitespace()) {
                    let kw = rest[..space_pos].trim();
                    if matches!(kw, "disable" | "enable" | "disable-line" | "disable-next-line") {
                        let r = tok.text_range();
                        let tok_start = u32::from(r.start()) as usize;
                        // Point at the space where the colon should be
                        let directive_offset = text.find("@diagnostic").unwrap_or(0) + "@diagnostic".len();
                        let colon_pos = text[directive_offset..].find(kw).map(|p| directive_offset + p + kw.len());
                        if let Some(pos) = colon_pos {
                            let start = tok_start + pos;
                            crate::diagnostics::malformed_annotation::check(
                                diags,
                                format!("Missing ':' after @diagnostic {kw}"),
                                start, start + 1,
                            );
                        }
                    }
                }
                continue;
            };
            let r = tok.text_range();
            let tok_start = u32::from(r.start()) as usize;
            let tok_text = text;
            for code in codes_str.split(',') {
                let code = code.trim();
                if code.is_empty() { continue; }
                if !KNOWN_CODES.contains(&code) {
                    // Find the byte offset of this code within the token
                    if let Some(offset) = tok_text.find(code) {
                        let start = tok_start + offset;
                        let end = start + code.len();
                        crate::diagnostics::unknown_diag_code::check(
                            diags, code, start, end,
                        );
                    }
                }
            }
        }
    }

    fn check_missing_fields_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.missing_fields_checks);
        for MissingFieldsCheck { class_table_idx, provided_fields, start, end } in checks {
            let table = self.table(class_table_idx);
            let class_name = match &table.class_name {
                Some(n) => n.clone(),
                None => continue,
            };
            // Collect required annotated fields (non-optional, non-function)
            let mut missing: Vec<&str> = Vec::new();
            let field_snapshot: Vec<(String, Option<ValueType>)> = table.fields.iter()
                .map(|(k, v)| (k.clone(), v.annotation.clone()))
                .collect();
            for (field_name, annotation) in &field_snapshot {
                let Some(ann) = annotation else { continue };
                // Optional fields: type includes nil
                let is_nullable = match ann {
                    ValueType::Nil => true,
                    ValueType::Union(types) => types.contains(&ValueType::Nil),
                    _ => false,
                };
                if is_nullable { continue; }
                // Skip function-typed fields (methods)
                if matches!(ann, ValueType::Function(_)) { continue; }
                // Check if this field was provided in the constructor
                if !provided_fields.iter().any(|p| p == field_name) {
                    missing.push(field_name);
                }
            }
            if !missing.is_empty() {
                missing.sort();
                let missing_refs: Vec<&str> = missing.into_iter().collect();
                crate::diagnostics::missing_fields::check(
                    diags,
                    &class_name, &missing_refs,
                    start as usize, end as usize,
                );
            }
        }
    }

    fn check_grouped_return_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.grouped_return_checks);
        for GroupedReturnCheck { func_id, return_exprs, start, end } in checks {
            let return_only_overloads: Vec<_> = self.ir.func(func_id).overloads.iter()
                .filter(|o| o.is_return_only)
                .cloned()
                .collect();
            if return_only_overloads.is_empty() { continue; }

            // Resolve the actual return types
            let actual_types: Vec<Option<ValueType>> = return_exprs.iter()
                .map(|&expr_id| self.resolve_expr_type(expr_id))
                .collect();

            // Check if the return values match ANY return-only overload
            let matches_any = return_only_overloads.iter().any(|overload| {
                // Overload with empty returns matches bare return / all nil
                if overload.returns.is_empty() {
                    return actual_types.iter().all(|t| {
                        matches!(t, None | Some(ValueType::Nil))
                    });
                }
                if overload.returns.len() == 1 && overload.returns[0] == ValueType::Nil {
                    return actual_types.iter().all(|t| {
                        matches!(t, None | Some(ValueType::Nil))
                    });
                }
                // Check each position matches the overload's type.
                // Vararg-tail overloads accept any length ≥ (declared - 1): the
                // trailing `...T` can match zero or more actual values, and
                // positions past the declared tail are compared against T.
                if overload.has_vararg_tail && !overload.returns.is_empty() {
                    let fixed = overload.returns.len() - 1;
                    if actual_types.len() < fixed { return false; }
                    let vararg_ty = &overload.returns[fixed];
                    return actual_types.iter().enumerate().all(|(i, actual)| {
                        let expected = if i < fixed { &overload.returns[i] } else { vararg_ty };
                        match actual {
                            Some(actual) => actual.is_assignable_to(expected) || self.is_table_subtype(actual, expected),
                            None => true,
                        }
                    });
                }
                if actual_types.len() != overload.returns.len() { return false; }
                actual_types.iter().zip(overload.returns.iter()).all(|(actual, expected)| {
                    match actual {
                        Some(actual) => actual.is_assignable_to(expected) || self.is_table_subtype(actual, expected),
                        None => true, // unresolved — don't warn
                    }
                })
            });

            if !matches_any {
                // If the return delegates to a function call whose callee also has
                // return-only overloads, suppress the diagnostic — the callee is
                // responsible for enforcing its own grouped-return constraints, and
                // the caller just passes through whatever the callee returns.
                if return_exprs.len() == 1
                    && let Expr::FunctionCall { func, ret_index: 0, .. } = self.expr(return_exprs[0]).clone()
                        && let Some(func_type) = self.resolve_expr_type(func) {
                            let callee_func_idx = match func_type {
                                ValueType::Function(Some(idx)) => Some(idx),
                                ValueType::Table(Some(table_idx)) => self.table(table_idx).call_func,
                                _ => None,
                            };
                            if let Some(callee_idx) = callee_func_idx
                                && self.func(callee_idx).overloads.iter().any(|o| o.is_return_only) {
                                    continue;
                                }
                        }

                let overload_desc: Vec<String> = return_only_overloads.iter()
                    .map(|o| {
                        if o.returns.is_empty() || (o.returns.len() == 1 && o.returns[0] == ValueType::Nil) {
                            "nil".to_string()
                        } else {
                            o.returns.iter()
                                .map(|vt| self.format_value_type_depth(vt, 1))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    })
                    .collect();
                let desc = overload_desc.join(" | ");
                crate::diagnostics::grouped_return_mismatch::check(
                    diags,
                    &desc,
                    start as usize, end as usize,
                );
            }
        }
    }

    fn check_incomplete_signature_doc_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        if self.is_meta { return; }

        let sentinel = crate::annotations::AnnotationType::Simple(String::new());

        for func_idx in 0..self.ir.functions.len() {
            let func = &self.ir.functions[func_idx];
            let Some(nid) = func.def_node.node_id else { continue };

            let has_return_ann = !func.return_annotations.is_empty()
                || func.returns_self
                || func.returns_built;
            let has_param_ann = func.param_annotations.iter().any(|a| a != &sentinel)
                || func.vararg_annotation.is_some();
            if !has_param_ann && !has_return_ann { continue; }

            let func_node = SyntaxNode { tree, id: nid };
            let Some(func_def) = FunctionDefinition::cast(func_node) else { continue };
            let Some(params_node) = func_def.params() else { continue };

            // Collect source-level parameter tokens with ranges.
            let mut src_params: Vec<(String, u32, u32)> = Vec::new();
            let mut vararg_range: Option<(u32, u32)> = None;
            for child in params_node.syntax().children_with_tokens() {
                if let NodeOrToken::Token(t) = child {
                    let r = t.text_range();
                    let start = u32::from(r.start());
                    let end = u32::from(r.end());
                    match t.kind() {
                        SyntaxKind::Parameter => src_params.push((t.text().to_string(), start, end)),
                        SyntaxKind::ParameterVarArgs => vararg_range = Some((start, end)),
                        _ => {}
                    }
                }
            }

            // Determine whether `self` was implicitly injected (colon-defined method).
            let self_injected = func.args.len() == src_params.len() + 1
                && matches!(&self.ir.symbols[func.args[0].val()].id,
                    SymbolIdentifier::Name(n) if n == "self");

            // Emit missing-@param diagnostics for each source param with no annotation.
            let arg_offset = if self_injected { 1 } else { 0 };
            for (i, (name, pstart, pend)) in src_params.iter().enumerate() {
                let arg_i = i + arg_offset;
                if arg_i >= func.args.len() { break; }
                let annotated = func.param_annotations.get(arg_i)
                    .is_some_and(|a| a != &sentinel);
                if !annotated {
                    crate::diagnostics::incomplete_signature_doc::push_missing_param(
                        diags, name,
                        *pstart as usize, *pend as usize,
                    );
                }
            }
            if let Some((vstart, vend)) = vararg_range
                && func.vararg_annotation.is_none() {
                    crate::diagnostics::incomplete_signature_doc::push_missing_param(
                        diags, "...",
                        vstart as usize, vend as usize,
                    );
                }

            // Emit missing-@return when the body returns a value but no @return exists.
            if !has_return_ann {
                let body_returns_value = func_def.block().is_some_and(|block| {
                    block.syntax().descendants().any(|desc| {
                        let Some(ret) = Return::cast(desc) else { return false };
                        let in_nested_fn = ret.syntax().ancestors().any(|anc| {
                            anc.kind() == SyntaxKind::FunctionDefinition && anc.id != nid
                        });
                        if in_nested_fn { return false; }
                        let Some(expr_list) = ret.expression_list() else { return false };
                        !expr_list.expressions().is_empty()
                    })
                });
                if body_returns_value {
                    // Span the `function` keyword — stable and ends on a token boundary.
                    let kw_range = func_def.syntax().children_with_tokens().find_map(|c| {
                        if let NodeOrToken::Token(t) = c
                            && t.kind() == SyntaxKind::FunctionKeyword {
                                return Some(t.text_range());
                            }
                        None
                    }).unwrap_or_else(|| func_def.syntax().text_range());
                    let start = u32::from(kw_range.start()) as usize;
                    let end = u32::from(kw_range.end()) as usize;
                    crate::diagnostics::incomplete_signature_doc::push_missing_return(
                        diags, start, end,
                    );
                }
            }
        }
    }

    // ── Unknown-type diagnostics (strict mode, default-off HINTs) ──────────────
    //
    // Fire when a site's `resolved_type` is `None` — i.e. the resolver could not
    // infer a type. `Some(Any)` is treated as "the author explicitly wrote
    // `@type any`/`@type unknown`" and skipped, since both resolve to
    // `ValueType::Any` and there's no user-level distinction worth flagging.

    fn check_unknown_param_type_diagnostics(&self, tree: &SyntaxTree, _deferred: &DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        if self.is_meta { return; }
        let sentinel = crate::annotations::AnnotationType::Simple(String::new());
        let mut emissions: Vec<(String, u32, u32)> = Vec::new();
        for func_idx in 0..self.ir.functions.len() {
            let func = &self.ir.functions[func_idx];
            let Some(nid) = func.def_node.node_id else { continue };
            let func_node = SyntaxNode { tree, id: nid };
            let Some(func_def) = FunctionDefinition::cast(func_node) else { continue };
            let Some(params_node) = func_def.params() else { continue };

            let src_params: Vec<(String, u32, u32)> = params_node.syntax().children_with_tokens()
                .filter_map(|c| match c {
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::Parameter => {
                        let r = t.text_range();
                        Some((t.text().to_string(), u32::from(r.start()), u32::from(r.end())))
                    }
                    _ => None,
                })
                .collect();

            let self_injected = func.args.len() == src_params.len() + 1
                && matches!(&self.ir.symbols[func.args[0].val()].id,
                    SymbolIdentifier::Name(n) if n == "self");
            let arg_offset = if self_injected { 1 } else { 0 };

            for (i, (name, pstart, pend)) in src_params.iter().enumerate() {
                let arg_i = i + arg_offset;
                if arg_i >= func.args.len() { break; }
                let sym_idx = func.args[arg_i];
                if sym_idx.is_external() { continue; }
                if name == "self" { continue; }
                let annotated = func.param_annotations.get(arg_i)
                    .is_some_and(|a| a != &sentinel);
                if annotated { continue; }
                let resolved = self.ir.symbols[sym_idx.val()].versions.first()
                    .and_then(|v| v.resolved_type.as_ref());
                if resolved.is_some() { continue; }
                emissions.push((name.clone(), *pstart, *pend));
            }
        }
        for (name, start, end) in emissions {
            crate::diagnostics::unknown_param_type::check(
                diags, &name, start as usize, end as usize,
            );
        }
    }

    fn check_unknown_local_type_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        if self.is_meta { return; }
        for (sym_idx, name, range) in self.iter_local_def_sites(tree) {
            let sym = &self.ir.symbols[sym_idx.val()];
            let Some(ver) = sym.versions.first() else { continue };
            if ver.resolved_type.is_some() { continue; }
            crate::diagnostics::unknown_local_type::check(
                diags, &name,
                u32::from(range.start()) as usize,
                u32::from(range.end()) as usize,
            );
        }
    }

    fn check_unknown_return_type_diagnostics(&self, deferred: &DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        if self.is_meta { return; }
        let checks: Vec<ReturnTypeCheck> = deferred.return_type_checks.clone();
        let mut emissions: Vec<(u32, u32)> = Vec::new();
        for check in checks {
            let func = &self.ir.functions[check.func_id.val()];
            if func.explicit_void_return { continue; }
            // Skip when the function declares a @return at this index — the
            // annotation is the author's source of truth. Body mismatches are
            // return-type-mismatch territory.
            if check.ret_index < func.return_annotations.len() { continue; }
            // `@return self` and `@return built` are implicit return-type
            // declarations (receiver / accumulated built-table) that aren't
            // recorded in `return_annotations`.
            if func.returns_self || func.returns_built { continue; }
            if self.resolve_expr_type(check.rhs_expr).is_some() { continue; }
            emissions.push((check.start, check.end));
        }
        for (start, end) in emissions {
            crate::diagnostics::unknown_return_type::check(
                diags, start as usize, end as usize,
            );
        }
    }

    fn check_unknown_field_type_diagnostics(&self, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        if self.is_meta { return; }
        let mut pending: Vec<(String, String, ExprId, u32, u32)> = Vec::new();

        for table_idx in 0..self.ir.tables.len() {
            let table = self.table(TableIndex(table_idx));
            let Some(class_name) = table.class_name.clone() else { continue };
            for (field_name, fi) in &table.fields {
                if fi.annotation_type_raw.is_some() { continue; }
                let Some((start, end)) = fi.def_range else { continue };
                pending.push((field_name.clone(), class_name.clone(), fi.expr, start, end));
            }
        }

        // Overlay fields (runtime assignments onto external @class tables).
        // Clone each FieldInfo because the resolve_expr_type call below reads
        // `&self`, so we can't hold a borrow into `ir.overlay_fields`
        // across it. The non-overlay branch avoids this by only borrowing
        // `table.fields` during the collect pass.
        let overlay_tables: Vec<TableIndex> = self.ir.overlay_fields.keys().copied().collect();
        for table_idx in overlay_tables {
            let Some(class_name) = self.table(table_idx).class_name.clone() else { continue };
            let fields: Vec<(String, FieldInfo)> = self.ir.overlay_fields.get(&table_idx)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            for (field_name, fi) in fields {
                if fi.annotation_type_raw.is_some() { continue; }
                let Some((start, end)) = fi.def_range else { continue };
                pending.push((field_name, class_name.clone(), fi.expr, start, end));
            }
        }

        let mut emissions: Vec<(String, String, u32, u32)> = Vec::new();
        for (field_name, class_name, expr_id, start, end) in pending {
            if self.resolve_expr_type(expr_id).is_some() { continue; }
            emissions.push((field_name, class_name, start, end));
        }
        for (field_name, class_name, start, end) in emissions {
            crate::diagnostics::unknown_field_type::check(
                diags, &field_name, &class_name,
                start as usize, end as usize,
            );
        }
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

    // ── Annotation metadata diagnostics (post-resolution) ──────────────────────

    fn check_annotation_metadata_diagnostics(&self, tree: &SyntaxTree, _deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);

        // ── Part 1: Comment-level checks ──────────────────────────────
        // Walk all comment tokens to detect annotation-level duplicates:
        //   duplicate_constructor, duplicate_doc_alias, duplicate_doc_field
        let mut current_class: Option<String> = None;
        let mut class_constructor_count: HashMap<String, u32> = HashMap::new();
        let mut class_field_names: HashMap<String, HashSet<String>> = HashMap::new();
        let mut seen_aliases: HashSet<String> = HashSet::new();

        for event in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment {
                if tok.kind() != SyntaxKind::Whitespace && tok.kind() != SyntaxKind::Newline {
                    current_class = None;
                }
                continue;
            }
            let text = tok.text();

            let after = text.strip_prefix("---@class ").or_else(|| text.strip_prefix("---@enum "));
            if let Some(after) = after {
                let name = after.split(|c: char| c.is_whitespace() || c == '<' || c == ':')
                    .next().unwrap_or("");
                if !name.is_empty() {
                    current_class = Some(name.to_string());
                }
                continue;
            }

            // duplicate_constructor
            if let Some(rest) = text.strip_prefix("---@constructor") {
                let rest = rest.trim();
                if !rest.is_empty()
                    && let Some(ref class_name) = current_class
                {
                    let count = class_constructor_count.entry(class_name.clone()).or_insert(0);
                    *count += 1;
                    if *count > 1 {
                        let r = tok.text_range();
                        crate::diagnostics::duplicate_constructor::check(
                            diags, class_name,
                            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                        );
                    }
                }
                continue;
            }

            // duplicate_doc_alias
            if let Some(rest) = text.strip_prefix("---@alias ") {
                let name = rest.split(|c: char| c.is_whitespace() || c == '<' || c == ':')
                    .next().unwrap_or("");
                if !name.is_empty() && !seen_aliases.insert(name.to_string()) {
                    let r = tok.text_range();
                    crate::diagnostics::duplicate_doc_alias::check(
                        diags, name,
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
                continue;
            }

            // duplicate_doc_field
            if let Some(rest) = text.strip_prefix("---@field ") {
                if let Some(ref class_name) = current_class {
                    let rest = rest.strip_prefix("private ").or_else(|| rest.strip_prefix("protected "))
                        .or_else(|| rest.strip_prefix("public ")).unwrap_or(rest);
                    let raw_name = rest.split_whitespace().next().unwrap_or("");
                    if raw_name.starts_with('[') { continue; }
                    let field_name = raw_name.trim_end_matches('?');
                    if !field_name.is_empty() {
                        let fields = class_field_names.entry(class_name.clone()).or_default();
                        if !fields.insert(field_name.to_string())
                            && let Some((start, end)) = super::Analysis::find_field_comment_range(root, class_name, field_name, true)
                        {
                            crate::diagnostics::duplicate_doc_field::check(
                                diags, field_name,
                                start as usize, end as usize,
                            );
                        }
                    }
                }
                continue;
            }
        }

        // ── Part 2: Function-level annotation checks ──────────────────
        // Walk FunctionDefinition nodes, re-extract annotations.
        //   duplicate_doc_param, undefined_doc_param, builds_field_not_self,
        //   constructor_return
        let func_by_start: HashMap<u32, usize> = self.ir.functions.iter()
            .enumerate()
            .filter(|(_, f)| f.def_node != DefNode::DUMMY)
            .map(|(i, f)| (f.def_node.start, i))
            .collect();

        for node in root.descendants() {
            if node.kind() != SyntaxKind::FunctionDefinition { continue; }
            let node_start = u32::from(node.text_range().start());
            let Some(&func_idx) = func_by_start.get(&node_start) else { continue; };
            let func = &self.ir.functions[func_idx];

            let annotations = crate::annotations::extract_annotations(node);

            // duplicate_doc_param + undefined_doc_param
            if !annotations.params.is_empty() {
                let arg_names: HashSet<String> = func.args.iter()
                    .filter_map(|&sym_idx| match &self.ir.symbols[sym_idx.val()].id {
                        SymbolIdentifier::Name(n) => Some(n.clone()),
                        _ => None,
                    })
                    .collect();

                let comment_ranges = super::Analysis::collect_preceding_annotation_ranges(node);
                let func_start = node_start as usize;
                let func_end = func_start + "function".len();

                let mut seen_params: HashSet<String> = HashSet::new();
                for p in &annotations.params {
                    let (s, e) = comment_ranges.iter()
                        .find(|(text, _, _)| text.starts_with("---@param") && text.contains(&p.name))
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_end));
                    if !seen_params.insert(p.name.clone()) {
                        crate::diagnostics::duplicate_doc_param::check(
                            diags, &p.name,
                            s, e,
                        );
                    } else if !arg_names.contains(&p.name) && p.name != "self"
                        && !(p.name == "..." && func.is_vararg)
                    {
                        crate::diagnostics::undefined_doc_param::check(
                            diags, &p.name,
                            s, e,
                        );
                    }
                }
            }

            // constructor_return (explicit @constructor)
            if func.constructor && !func.return_annotations.is_empty() {
                let r = node.text_range();
                crate::diagnostics::constructor_return::check(
                    diags,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }

            // constructor_return (inherited constructor)
            let func_index = FunctionIndex(func_idx);
            if self.inherited_constructors.contains(&func_index)
                && !func.constructor
                && !func.return_annotations.is_empty()
            {
                let r = node.text_range();
                crate::diagnostics::constructor_return::check(
                    diags,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }

            // builds_field_not_self
            if func.builds_field.is_some()
                && let Some(class_name) = self.function_owner_class.get(&func_index)
            {
                let returns_own_class = annotations.returns.iter().any(|rt| {
                    matches!(rt, crate::annotations::AnnotationType::Simple(s) if s == class_name)
                });
                if returns_own_class {
                    let r = node.text_range();
                    crate::diagnostics::builds_field_not_self::check(
                        diags, class_name,
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            }

            // return_self_class_name
            if func.builds_field.is_none()
                && let Some(class_name) = self.function_owner_class.get(&func_index)
            {
                let returns_own_class = annotations.returns.iter().any(|rt| {
                    matches!(rt, crate::annotations::AnnotationType::Simple(s) if s == class_name)
                });
                if returns_own_class {
                    let func_node_id = node.id;
                    let any_returns_bare_self = FunctionDefinition::cast(node).and_then(|f| f.block()).is_some_and(|block| {
                        block.syntax().descendants().any(|desc| {
                            let Some(ret) = Return::cast(desc) else { return false };
                            let in_nested_fn = ret.syntax().ancestors().any(|anc| {
                                anc.kind() == SyntaxKind::FunctionDefinition && anc.id != func_node_id
                            });
                            if in_nested_fn { return false; }
                            let Some(expr_list) = ret.expression_list() else { return false };
                            let exprs = expr_list.expressions();
                            exprs.first().is_some_and(|expr| {
                                if let Expression::Identifier(ident) = expr {
                                    ident.syntax().kind() == SyntaxKind::NameRef
                                        && ident.syntax().text().0 == "self"
                                } else {
                                    false
                                }
                            })
                        })
                    });
                    if any_returns_bare_self {
                        let r = node.text_range();
                        crate::diagnostics::return_self_class_name::check(
                            diags, class_name,
                            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                        );
                    }
                }
            }
        }

        // ── Part 3: Deprecated call-site checks ──────────────────────
        // Walk all FunctionCall expressions to check for deprecated functions.
        for expr in self.ir.exprs.iter() {
            let Expr::FunctionCall { func: callee, call_range, .. } = expr else { continue };
            let callee = *callee;
            let call_range = *call_range;
            let Some(callee_type) = self.resolve_expr_type(callee) else { continue };
            let func_idx = match callee_type {
                ValueType::Function(Some(idx)) => idx,
                _ => continue,
            };
            if !self.func(func_idx).deprecated { continue; }
            let name = self.function_name(func_idx).unwrap_or_else(|| "?".to_string());
            crate::diagnostics::deprecated::check(
                diags,
                &name, call_range.0 as usize, call_range.1 as usize,
            );
        }
    }

    // ── AST-only diagnostics (no resolved types needed) ────────────────────────

    fn check_ast_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        Self::walk_ast_diagnostics(diags, root, self.is_meta);
        Self::check_orphan_fields(diags, SyntaxNode::new_root(tree));
        crate::diagnostics::trailing_space::check(diags, tree.source());
    }

    fn walk_ast_diagnostics(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        node: SyntaxNode<'_>,
        is_meta: bool,
    ) {
        match node.kind() {
            SyntaxKind::Block => {
                if let Some(block) = Block::cast(node) {
                    Self::check_block_diagnostics(diags, block, is_meta);
                }
                return;
            }
            SyntaxKind::BinaryExpression => {
                if let Some(bin) = BinaryExpression::cast(node) {
                    Self::check_not_precedence(diags, bin);
                }
            }
            SyntaxKind::FunctionDefinition => {
                if let Some(func) = FunctionDefinition::cast(node) {
                    Self::check_unused_vararg(diags, func, is_meta);
                }
            }
            SyntaxKind::LocalAssignStatement => {
                if let Some(assign) = LocalAssign::cast(node) {
                    Self::check_assignment_balance_local(diags, assign);
                }
            }
            SyntaxKind::AssignStatement => {
                if let Some(assign) = Assign::cast(node) {
                    Self::check_assignment_balance_nonlocal(diags, assign);
                }
            }
            SyntaxKind::WhileLoop | SyntaxKind::RepeatUntilLoop => {
                if let Some(block) = node.children().find_map(Block::cast)
                    && Self::block_is_empty(&block)
                {
                    let r = node.text_range();
                    crate::diagnostics::empty_block::check(
                        diags,
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            }
            SyntaxKind::ForCountLoop => {
                if let Some(for_loop) = ForCountLoop::cast(node) {
                    Self::check_for_count_loop(diags, for_loop);
                }
            }
            SyntaxKind::ForInLoop => {
                if let Some(for_in) = ForInLoop::cast(node)
                    && let Some(block) = for_in.block()
                    && Self::block_is_empty(&block)
                {
                    let r = for_in.syntax().text_range();
                    crate::diagnostics::empty_block::check(
                        diags,
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            }
            SyntaxKind::IfChain => {
                if let Some(if_chain) = IfChain::cast(node) {
                    Self::check_if_chain_empty_blocks(diags, if_chain);
                }
            }
            _ => {}
        }
        for child in node.children() {
            Self::walk_ast_diagnostics(diags, child, is_meta);
        }
    }

    fn check_orphan_fields(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        root: SyntaxNode<'_>,
    ) {
        let mut group_has_class = false;
        let mut field_tokens: Vec<(u32, u32)> = Vec::new();
        let mut prev_was_newline = false;

        for event in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            let kind = tok.kind();
            if kind == SyntaxKind::Comment {
                let text = tok.text();
                if text.starts_with("---@") || text.starts_with("--- @") {
                    let content = text.trim_start_matches('-').trim();
                    if content.starts_with("@class") || content.starts_with("@enum") {
                        group_has_class = true;
                    } else if content.starts_with("@field") {
                        let r = tok.text_range();
                        field_tokens.push((u32::from(r.start()), u32::from(r.end())));
                    }
                }
                prev_was_newline = false;
            } else if kind == SyntaxKind::Newline {
                if prev_was_newline && (!field_tokens.is_empty() || group_has_class) {
                    if !group_has_class {
                        for (start, end) in &field_tokens {
                            crate::diagnostics::doc_field_no_class::check(
                                diags, *start as usize, *end as usize,
                            );
                        }
                    }
                    group_has_class = false;
                    field_tokens.clear();
                }
                prev_was_newline = true;
            } else if kind == SyntaxKind::Whitespace {
                // don't change state
            } else {
                if !group_has_class {
                    for (start, end) in &field_tokens {
                        crate::diagnostics::doc_field_no_class::check(
                            diags, *start as usize, *end as usize,
                        );
                    }
                }
                group_has_class = false;
                field_tokens.clear();
                prev_was_newline = false;
            }
        }
        if !group_has_class {
            for (start, end) in &field_tokens {
                crate::diagnostics::doc_field_no_class::check(
                    diags, *start as usize, *end as usize,
                );
            }
        }
    }

    fn check_block_diagnostics(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        block: Block<'_>,
        is_meta: bool,
    ) {
        let block_node = block.syntax();
        let statements = block.statements();

        // code-after-break
        let mut saw_break = false;
        for child in block_node.children_with_tokens() {
            if let NodeOrToken::Token(tok) = &child {
                if tok.kind() == SyntaxKind::BreakKeyword {
                    saw_break = true;
                }
            } else if let NodeOrToken::Node(ref n) = child
                && saw_break && Statement::cast(*n).is_some() {
                    let r = n.text_range();
                    crate::diagnostics::code_after_break::check(
                        diags,
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                    break;
                }
        }

        for (i, stmt) in statements.iter().enumerate() {
            // unreachable-code
            if matches!(stmt, Statement::Return(_)) && i + 1 < statements.len() {
                let next_stmt = &statements[i + 1];
                let r = next_stmt.syntax().text_range();
                crate::diagnostics::unreachable_code::check(
                    diags,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }

            // redundant-return
            if i + 1 == statements.len()
                && let Statement::Return(ret) = stmt
            {
                let has_values = ret.expression_list()
                    .is_some_and(|el| !el.expressions().is_empty());
                let is_fn_top_block = block_node.parent()
                    .is_some_and(|p| p.kind() == SyntaxKind::FunctionDefinition);
                if !has_values && is_fn_top_block {
                    let r = ret.syntax().text_range();
                    crate::diagnostics::redundant_return::check(
                        diags,
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            }
        }

        for child in block_node.children() {
            Self::walk_ast_diagnostics(diags, child, is_meta);
        }
    }

    fn check_not_precedence(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        bin: BinaryExpression<'_>,
    ) {
        if !bin.kind().is_comparison() { return; }
        let terms = bin.get_terms();
        let [lhs, rhs] = terms.as_slice() else { return };
        let Expression::UnaryExpression(unary) = lhs else { return };
        if unary.kind() != Operator::Not { return; }
        let op_kind = bin.kind();
        if matches!(op_kind, Operator::Equals | Operator::NotEquals)
            && let Expression::UnaryExpression(rhs_unary) = rhs
            && rhs_unary.kind() == Operator::Not
        {
            return;
        }
        let op = match op_kind {
            Operator::Equals => "==",
            Operator::NotEquals => "~=",
            Operator::LessThan => "<",
            Operator::LessThanOrEquals => "<=",
            Operator::GreaterThan => ">",
            Operator::GreaterThanOrEquals => ">=",
            _ => return,
        };
        let br = bin.syntax().text_range();
        crate::diagnostics::not_precedence::check(
            diags,
            op,
            u32::from(br.start()) as usize,
            u32::from(br.end()) as usize,
        );
    }

    fn check_unused_vararg(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        func: FunctionDefinition<'_>,
        is_meta: bool,
    ) {
        if is_meta { return; }
        let Some(params) = func.params() else { return };
        if !params.ellipsis() { return; }
        let Some(body) = func.block() else { return; };
        if Self::body_uses_varargs(body.syntax()) { return; }
        let vararg_range = params.syntax().children_with_tokens()
            .find_map(|c| match c {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::ParameterVarArgs => Some(t.text_range()),
                _ => None,
            });
        let Some(vararg_range) = vararg_range else { return };
        let name = func.identifier()
            .and_then(|id| id.names().last().cloned())
            .or_else(|| func.name());
        crate::diagnostics::unused_vararg::check(
            diags,
            name.as_deref(),
            u32::from(vararg_range.start()) as usize,
            u32::from(vararg_range.end()) as usize,
        );
    }

    fn body_uses_varargs(body: SyntaxNode<'_>) -> bool {
        for child in body.children_with_tokens() {
            match child {
                NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::TripleDot {
                        return true;
                    }
                }
                NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::FunctionDefinition {
                        continue;
                    }
                    if Self::body_uses_varargs(n) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn check_assignment_balance_local(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        assign: LocalAssign<'_>,
    ) {
        let Some(name_list) = assign.name_list() else { return };
        let names = name_list.names();
        let expressions = assign
            .expression_list()
            .map(|el| el.expressions())
            .unwrap_or_default();
        let last_is_multi = matches!(
            expressions.last(),
            Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
        );
        if !last_is_multi && !expressions.is_empty() {
            if expressions.len() > names.len() {
                if let Some(extra) = expressions.get(names.len()) {
                    let r = extra.syntax().text_range();
                    crate::diagnostics::redundant_value::check(
                        diags,
                        names.len(), expressions.len(),
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            } else if names.len() > expressions.len() {
                let r = assign.syntax().text_range();
                crate::diagnostics::unbalanced_assignments::check(
                    diags,
                    names.len(), expressions.len(),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
    }

    fn check_assignment_balance_nonlocal(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        assign: Assign<'_>,
    ) {
        let Some(var_list) = assign.variable_list() else { return };
        let identifiers = var_list.identifiers();
        let expressions = assign
            .expression_list()
            .map(|el| el.expressions())
            .unwrap_or_default();
        let last_is_multi = matches!(
            expressions.last(),
            Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
        );
        if !last_is_multi && !expressions.is_empty() {
            if expressions.len() > identifiers.len() {
                if let Some(extra) = expressions.get(identifiers.len()) {
                    let r = extra.syntax().text_range();
                    crate::diagnostics::redundant_value::check(
                        diags,
                        identifiers.len(), expressions.len(),
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            } else if identifiers.len() > expressions.len() {
                let r = assign.syntax().text_range();
                crate::diagnostics::unbalanced_assignments::check(
                    diags,
                    identifiers.len(), expressions.len(),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
    }

    fn check_for_count_loop(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        for_loop: ForCountLoop<'_>,
    ) {
        // empty-block check
        if let Some(block) = for_loop.block()
            && Self::block_is_empty(&block)
        {
            let r = for_loop.syntax().text_range();
            crate::diagnostics::empty_block::check(
                diags,
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }

        // count-down-loop check
        let Some(expr_list) = for_loop.expression_list() else { return };
        let exprs = expr_list.expressions();
        if exprs.len() < 2 { return; }
        let start_val = Self::expr_literal_number(&exprs[0]);
        let end_val = Self::expr_literal_number(&exprs[1]);
        let step_val = if exprs.len() >= 3 {
            Self::expr_literal_number(&exprs[2])
        } else {
            None
        };
        let (Some(sv), Some(ev)) = (start_val, end_val) else { return };
        let step = step_val.unwrap_or(1.0);
        let should_warn = if step == 0.0 {
            step_val.is_some() && sv != ev
        } else {
            let counting_down = sv > ev;
            let step_positive = step > 0.0;
            (counting_down && step_positive) || (!counting_down && sv != ev && !step_positive)
        };
        if !should_warn { return; }
        let msg = if step_val.is_none() {
            format!("loop from {} to {} will not execute (implicit step is 1; use -1)", sv, ev)
        } else if step == 0.0 {
            format!("loop from {} to {} with step 0 will loop forever", sv, ev)
        } else {
            format!("loop from {} to {} with step {} will not execute", sv, ev, step)
        };
        let br = for_loop.syntax().text_range();
        crate::diagnostics::count_down_loop::check(
            diags,
            u32::from(br.start()) as usize,
            u32::from(br.end()) as usize,
            msg,
        );
    }

    fn check_if_chain_empty_blocks(
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
        if_chain: IfChain<'_>,
    ) {
        for branch in if_chain.if_branches() {
            if let Some(inner_block) = branch.block()
                && Self::block_is_empty(&inner_block)
            {
                let r = branch.syntax().text_range();
                crate::diagnostics::empty_block::check(
                    diags,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
        if let Some(else_branch) = if_chain.else_branch()
            && let Some(inner_block) = else_branch.block()
            && Self::block_is_empty(&inner_block)
        {
            let r = else_branch.syntax().text_range();
            crate::diagnostics::empty_block::check(
                diags,
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }
    }

    fn block_is_empty(block: &Block<'_>) -> bool {
        if !block.statements().is_empty() { return false; }
        for child in block.syntax().children_with_tokens() {
            if let NodeOrToken::Token(tok) = &child
                && (tok.kind() == SyntaxKind::BreakKeyword || tok.kind() == SyntaxKind::Comment)
            {
                return false;
            }
        }
        true
    }

    fn expr_literal_number(expr: &Expression<'_>) -> Option<f64> {
        match expr {
            Expression::Literal(lit) => {
                lit.get_number().and_then(|s| s.trim().parse::<f64>().ok())
            }
            Expression::UnaryExpression(unary) => {
                if unary.kind() == Operator::Subtract {
                    let terms = unary.get_terms();
                    if let Some(Expression::Literal(lit)) = terms.first() {
                        return lit.get_number().and_then(|s| s.trim().parse::<f64>().ok()).map(|v| -v);
                    }
                }
                None
            }
            _ => None,
        }
    }

    // ── Simple state-capture diagnostics (Phase III) ──────────────────────────

    fn check_redefined_local_diagnostics(&self, tree: &SyntaxTree, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        // Walk symbols: any version after the first that is itself a LocalAssign
        // declaration (i.e. `local x` re-declared in the same scope) is a redefinition.
        let root = SyntaxNode::new_root(tree);
        for sym in &self.ir.symbols {
            let name = match &sym.id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => continue,
            };
            if name.starts_with('_') { continue; }
            if sym.versions.len() < 2 { continue; }
            // Skip if version[0] is not itself a local declaration (params, function args,
            // or assignment-created symbols can't be redefined-local sources).
            let first_def = sym.versions[0].def_node.start;
            if !is_in_local_assign_statement(&root, first_def) { continue; }
            for ver in &sym.versions[1..] {
                let def_start = ver.def_node.start;
                if !is_in_local_assign_statement(&root, def_start) { continue; }
                let Some(range) = self.def_name_token_range(tree, def_start, ver.def_node.end, &name) else { continue };
                crate::diagnostics::redefined_local::check(
                    diags, &name,
                    u32::from(range.start()) as usize,
                    u32::from(range.end()) as usize,
                );
            }
        }
    }

    fn check_return_count_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.return_count_checks);
        for site in &checks {
            let func = &self.ir.functions[site.func_id.val()];
            let expected_count = func.return_annotations.len();
            let has_nil_overload = func.overloads.iter().any(|o| {
                o.is_return_only
                    && (o.returns.is_empty()
                        || o.returns.iter().all(|t| t == &ValueType::Nil))
            });
            let effective_expected = if func.has_vararg_return && expected_count > 0 {
                expected_count - 1
            } else {
                expected_count
            };
            if site.expr_count < effective_expected && !site.last_is_multi && !has_nil_overload {
                let omitted_all_optional = func.return_annotations[site.expr_count..effective_expected]
                    .iter().all(|t| t.contains_nil());
                let all_returns_nullable = site.expr_count == 0 && omitted_all_optional;
                if all_returns_nullable {
                    crate::diagnostics::implicit_nil_return::check(
                        diags,
                        effective_expected,
                        site.start as usize, site.end as usize,
                    );
                } else if !omitted_all_optional {
                    crate::diagnostics::missing_return_value::check(
                        diags,
                        effective_expected, site.expr_count,
                        site.start as usize, site.end as usize,
                    );
                }
            }

            if expected_count > 0 && site.expr_count > expected_count && !func.has_vararg_return
                && site.extra_expr_start != 0
            {
                crate::diagnostics::redundant_return_value::check(
                    diags,
                    expected_count, site.expr_count,
                    site.extra_expr_start as usize, site.extra_expr_end as usize,
                );
            }
        }
    }

    fn check_inject_field_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.inject_field_checks);
        for site in &checks {
            if site.field_existed_at_build { continue; }
            // If the field was declared during resolution (e.g. @builds-field), suppress
            if self.class_has_annotated_field(site.table_idx, &site.field_name) { continue; }
            let table = self.table(site.table_idx);
            let has_annotations = table.fields.values().any(|f| f.annotation.is_some());
            let Some(ref class_name) = table.class_name else { continue };
            if !has_annotations { continue; }
            let class_name = class_name.clone();
            // Re-lookup via ir.classes — Phase 2 may have updated the class to point
            // to a different table (e.g. @built-name) that has the field.
            if let Some(&class_table_idx) = self.ir.classes.get(&class_name)
                && self.class_has_annotated_field(class_table_idx, &site.field_name) { continue; }
            if self.suppress_inject_field_on_g(&class_name, &site.field_name, site.scope_idx) { continue; }
            crate::diagnostics::inject_field::check(
                diags,
                &site.field_name, &class_name,
                site.start as usize, site.end as usize,
            );
        }
    }

    fn check_discard_returns_diagnostics(&self, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        for expr in self.ir.exprs.iter() {
            let Expr::FunctionCall { func: callee, ret_index, call_range, discarded, .. } = expr else { continue };
            if *ret_index != 0 { continue; }
            if !*discarded { continue; }
            let Some(ValueType::Function(Some(func_idx))) = self.resolve_expr_type(*callee) else { continue };
            if !self.func(func_idx).nodiscard { continue; }
            let name = self.function_name(func_idx).unwrap_or_else(|| "?".to_string());
            crate::diagnostics::discard_returns::check(
                diags,
                &name, call_range.0 as usize, call_range.1 as usize,
            );
        }
    }

    fn check_wrong_flavor_api_diagnostics(&self, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        if self.project_flavors == 0 { return; }
        for expr in self.ir.exprs.iter() {
            let Expr::FunctionCall { func: callee, ret_index, call_range, .. } = expr else { continue };
            if *ret_index != 0 { continue; }
            let Some(ValueType::Function(Some(func_idx))) = self.resolve_expr_type(*callee) else { continue };
            let call_mask = self.func(func_idx).flavors;
            if call_mask == 0 { continue; }
            let scope_idx = self.ir.scope_at_offset(call_range.0).unwrap_or(ScopeIndex(0));
            let active = self.active_flavors_at(scope_idx);
            let missing = crate::flavor::unsupported_flavors(active, call_mask);
            if missing != 0 {
                let name = self.function_name(func_idx).unwrap_or_else(|| "?".to_string());
                crate::diagnostics::wrong_flavor_api::check(
                    diags,
                    &name, missing, call_mask,
                    call_range.0 as usize, call_range.1 as usize,
                );
            }
        }
    }

    // Pass-through: deferred for architectural consistency with the other resolve-call
    // diagnostics, not because it needs post-resolution re-evaluation.
    fn check_redundant_param_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.redundant_param_checks);
        for check in checks {
            crate::diagnostics::redundant_param::check(
                diags, check.expected_count, check.actual_count,
                check.start as usize, check.end as usize,
            );
        }
    }

    // Pass-through: deferred for architectural consistency (see check_redundant_param above).
    fn check_missing_param_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.missing_param_checks);
        for check in checks {
            crate::diagnostics::missing_param::check(
                diags, &check.param_name,
                check.start as usize, check.end as usize,
            );
        }
    }

    // Temporal asymmetry: `expected_type` is captured at resolution time (from annotations,
    // which are stable), while `arg_type` is re-resolved here post-fixpoint to pick up the
    // final converged type. The fixpoint loop may push duplicate deferred entries across
    // iterations; these are deduplicated by the (code, start, end) retain at the end of
    // resolve_types().
    fn check_arg_type_mismatch_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.arg_type_mismatch_checks);
        for check in checks {
            let Some(mut arg_type) = self.resolve_expr_type(check.arg_expr) else { continue };
            if let Some(sym_idx) = self.ir.find_root_symbol(check.arg_expr)
                && let Some(scope_idx) = self.scope_at_offset(check.start) {
                    if !self.is_narrowing_overridden(sym_idx, scope_idx) {
                        if let Some(narrowed_vt) = self.get_type_narrowing(sym_idx, scope_idx) {
                            if !arg_type.is_assignable_to(narrowed_vt) {
                                arg_type = narrowed_vt.clone();
                            }
                        } else if let Some(guard_vt) = self.get_type_filtering(sym_idx, scope_idx) {
                            arg_type = arg_type.filter_type_with(guard_vt, &|idx| self.table(idx).is_enum);
                        }
                        if let Some(stripped_vt) = self.get_type_stripping(sym_idx, scope_idx) {
                            arg_type = arg_type.strip_type_with(stripped_vt, &|idx| self.table(idx).is_enum);
                        }
                    }
                    if self.is_symbol_falsy_narrowed(sym_idx, scope_idx) {
                        arg_type = arg_type.strip_falsy();
                    } else if self.is_symbol_narrowed(sym_idx, scope_idx) {
                        arg_type = arg_type.strip_nil();
                    }
                    if let Some((_, chain)) = self.ir.extract_field_chain(check.arg_expr) {
                        if let Some(narrowed_vt) = self.get_field_type_narrowing(sym_idx, &chain, scope_idx) {
                            arg_type = narrowed_vt.clone();
                        } else if self.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                            arg_type = arg_type.strip_nil();
                            if matches!(arg_type, ValueType::Nil) {
                                continue;
                            }
                        }
                    }
                }
            if arg_type.contains_type_variable() { continue; }
            if check.skip_if_nil && matches!(arg_type, ValueType::Nil) { continue; }
            let structurally_matched = !arg_type.is_assignable_to(&check.expected_type)
                && self.is_table_subtype(&arg_type, &check.expected_type);
            if structurally_matched {
                self.check_excess_structural_fields(
                    deferred, &arg_type, &check.expected_type,
                    check.start as usize, check.end as usize,
                );
            }
            if (!arg_type.is_assignable_to(&check.expected_type) && !structurally_matched)
                || !self.is_function_compatible(&arg_type, &check.expected_type) {
                let is_nil_union_compatible = matches!(&arg_type, ValueType::Union(types) if types.iter().any(|t| matches!(t, ValueType::Nil))) && {
                    let stripped = arg_type.strip_nil();
                    stripped.is_assignable_to(&check.expected_type)
                        && self.is_function_compatible(&stripped, &check.expected_type)
                };
                let expected_str = self.format_value_type_depth(&check.expected_type, 1);
                let actual_str = self.format_value_type_depth(&arg_type, 1);
                if is_nil_union_compatible {
                    crate::diagnostics::need_check_nil::check_param(
                        diags, &check.param_name,
                        &expected_str, &actual_str,
                        check.start as usize, check.end as usize,
                    );
                } else {
                    crate::diagnostics::type_mismatch::check(
                        diags, &check.param_name,
                        &expected_str, &actual_str,
                        check.start as usize, check.end as usize,
                    );
                }
            }
        }
    }

    // Pass-through: deferred for architectural consistency (see check_redundant_param above).
    fn check_multi_return_projection_diagnostics(&self, deferred: &mut DeferredChecks, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        let checks = std::mem::take(&mut deferred.multi_return_projection_checks);
        for check in checks {
            crate::diagnostics::multi_return_projection::check(
                diags,
                check.start as usize, check.end as usize,
            );
        }
    }

    // Re-resolves the callee type post-fixpoint. This intentionally suppresses the diagnostic
    // when narrowing resolved after the call was first seen (e.g. a nil guard later in the
    // fixpoint), which is more correct than the prior inline emission.
    fn check_nil_callee_diagnostics(&self, diags: &mut Vec<crate::diagnostics::WowDiagnostic>) {
        for expr in self.ir.exprs.iter() {
            let Expr::FunctionCall { func: callee, call_range, .. } = expr else { continue };
            let callee = *callee;
            let call_range = *call_range;
            let Some(func_type) = self.resolve_expr_type(callee) else { continue };
            let has_nil = match &func_type {
                ValueType::Union(types) => types.iter().any(|t| matches!(t, ValueType::Nil)),
                _ => false,
            };
            let has_func = match &func_type {
                ValueType::Union(types) => types.iter().any(|t| matches!(t, ValueType::Function(_))),
                _ => false,
            };
            if !has_nil || !has_func { continue; }
            let mut suppressed = self.and_guarded_call_exprs.contains(&callee);
            if !suppressed
                && let Some(scope_idx) = self.scope_at_offset(call_range.0)
                    && let Some(sym_idx) = self.ir.find_root_symbol(callee) {
                        if self.is_symbol_narrowed(sym_idx, scope_idx) {
                            suppressed = true;
                        } else if let Some((_, chain)) = self.ir.extract_field_chain(callee)
                            && self.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                                suppressed = true;
                            }
                    }
            if !suppressed {
                let type_str = self.format_value_type_depth(&func_type, 0);
                crate::diagnostics::need_check_nil::check_call(
                    diags,
                    &type_str,
                    call_range.0 as usize, call_range.1 as usize,
                );
            }
        }
    }
}

/// True when any ancestor of `node` matches one of `kinds`.
fn has_ancestor_of_kind(node: &SyntaxNode, kinds: &[SyntaxKind]) -> bool {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if kinds.contains(&n.kind()) { return true; }
        cur = n.parent();
    }
    false
}

/// True when the byte offset `def_start` falls inside a `LocalAssignStatement`
/// (i.e. `local x = ...`). Mirrors the build-time check that gated redefined-local.
fn is_in_local_assign_statement(root: &SyntaxNode, def_start: u32) -> bool {
    let Some(token) = root.token_at_offset(TextSize::from(def_start)).right_biased() else { return false };
    let mut node = token.parent();
    while let Some(n) = node {
        match n.kind() {
            SyntaxKind::LocalAssignStatement => return true,
            SyntaxKind::Block | SyntaxKind::FunctionDefinition => return false,
            _ => node = n.parent(),
        }
    }
    false
}

/// Extract a string-literal key from a bracket-keyed table field's syntax node.
/// Mirrors the `string_literals` build-time logic: trims surrounding `"`/`'` quotes.
fn extract_bracket_string_key(field_node: &SyntaxNode) -> Option<String> {
    let key_expr = field_node.children().find_map(Expression::cast)?;
    let lit = match key_expr {
        Expression::Literal(l) => l,
        _ => return None,
    };
    let raw = lit.get_string()?;
    Some(raw.trim_matches(|c| c == '"' || c == '\'').to_string())
}
