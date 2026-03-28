use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::annotations::{AnnotationType, CastMode, extract_annotations};
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr};
use crate::types::*;
use super::Analysis;

// ── IR Building (Phase 1) ──────────────────────────────────────────────────────

/// Returns the end byte offset of a syntax node, excluding trailing whitespace/newlines.
/// The parser may include trailing trivia in expression nodes; this trims it so that
/// diagnostic ranges don't bleed into the next line.
fn trimmed_node_end(node: &SyntaxNode) -> u32 {
    let mut tok = node.last_token();
    let node_range = node.text_range();
    while let Some(t) = tok {
        // Stop if the token is outside this node
        if t.text_range().end() <= node_range.start() {
            break;
        }
        let kind = t.kind();
        if kind != SyntaxKind::Whitespace && kind != SyntaxKind::Newline {
            return u32::from(t.text_range().end());
        }
        tok = t.prev_token();
    }
    u32::from(node_range.end())
}

/// What a single `or` term narrows a symbol to in the then-branch.
enum OrTermEffect {
    /// `x == nil` — value is nil
    IsNil,
    /// `type(x) == "number"` — value is a specific type
    TypeIs(ValueType),
}

/// How an `and`/`or` LHS guard narrows a symbol for the RHS.
enum GuardNarrow {
    /// Nil comparison (`x ~= nil and ...`): strip only nil
    StripNil,
    /// Bare truthiness (`x and ...`): strip nil and false
    StripFalsy,
    /// Type guard (`type(x) == "string" and ...`): filter union to matching types
    FilterTo(ValueType),
}

impl Analysis {
    pub(super) fn build_ir(&mut self) {
        self.ir.scopes.push(Scope {
            parent: None,
            symbols: HashMap::new(),
        });

        /// Tracks an if/elseif/else chain where all branches may assign to a variable.
        struct PendingBranchMerge {
            parent_scope: ScopeIndex,
            branch_scopes: Vec<ScopeIndex>,
            /// True when there is no explicit `else` block — the implicit else path
            /// contributes the pre-if version to the merge.
            has_implicit_else: bool,
        }

        #[derive(Clone)]
        struct Frame {
            block: Block,
            next_stmt: usize,
            scope_idx: ScopeIndex,
            func_id: Option<FunctionIndex>,
            constructor_of: Option<TableIndex>,
        }

        let mut pending_branch_merges: Vec<PendingBranchMerge> = Vec::new();

        let root_block = Block::cast(self.root.clone()).expect("everything starts with a block");
        let mut stack = vec![Frame {
            block: root_block,
            next_stmt: 0,
            scope_idx: 0,
            func_id: None,
            constructor_of: None,
        }];

        while let Some(frame) = stack.last_mut() {
            let scope_idx = frame.scope_idx;
            let func_id = frame.func_id;
            let constructor_of = frame.constructor_of;
            self.current_func_id = func_id;
            if frame.next_stmt == 0 {
                self.ir.block_scopes.push((frame.block.syntax().text_range(), scope_idx));
            }
            let statements = frame.block.statements();
            if frame.next_stmt >= statements.len() {
                // D6: code-after-break — scan block for break followed by statements
                let block_node = frame.block.syntax().clone();
                stack.pop();
                let mut saw_break = false;
                for child in block_node.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(tok) = &child {
                        if tok.kind() == SyntaxKind::BreakKeyword {
                            saw_break = true;
                        }
                    } else if let rowan::NodeOrToken::Node(node) = &child {
                        if saw_break && Statement::cast(node.clone()).is_some() {
                            let r = node.text_range();
                            crate::diagnostics::code_after_break::check(
                                &mut self.diagnostics,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                            break;
                        }
                    }
                }
                continue;
            }

            // Process pending branch merges for this scope.
            // When an if/elseif/else chain is processed, branch frames are pushed onto the
            // stack. After all branch frames complete and the parent frame resumes, we create
            // merged versions for variables assigned (or narrowed) in all branches so that
            // code after the chain sees the union type instead of the pre-chain nil.
            {
                let mut mi = 0;
                while mi < pending_branch_merges.len() {
                    if pending_branch_merges[mi].parent_scope == scope_idx {
                        let merge = pending_branch_merges.swap_remove(mi);
                        let branch_scopes = &merge.branch_scopes;
                        // Collect symbols assigned in branch scopes: sym_idx → [(scope, ver_idx)]
                        let mut sym_branch_vers: HashMap<SymbolIndex, Vec<(ScopeIndex, usize)>> = HashMap::new();
                        for (sym_idx, sym) in self.ir.symbols.iter().enumerate() {
                            if sym_idx >= EXT_BASE { break; }
                            for (ver_idx, ver) in sym.versions.iter().enumerate() {
                                if branch_scopes.contains(&ver.created_in_scope) {
                                    sym_branch_vers.entry(sym_idx)
                                        .or_default()
                                        .push((ver.created_in_scope, ver_idx));
                                }
                            }
                        }

                        for (sym_idx, branch_vers) in &sym_branch_vers {
                            let assigned_scopes: HashSet<ScopeIndex> = branch_vers.iter().map(|(s, _)| *s).collect();
                            // Each explicit branch must either assign to the variable or narrow it
                            let all_covered = branch_scopes.iter().all(|bs| {
                                assigned_scopes.contains(bs)
                                    || self.is_symbol_narrowed(*sym_idx, *bs)
                                    || self.is_symbol_falsy_narrowed(*sym_idx, *bs)
                            });
                            if !all_covered { continue; }

                            let pre_ver = if merge.has_implicit_else {
                                // For if-without-else, find the pre-if version
                                // excluding child scope versions
                                self.ir.version_for_scope_ancestors_only(*sym_idx, scope_idx)
                            } else {
                                self.ir.version_for_scope(*sym_idx, scope_idx)
                            };
                            let mut merge_exprs = Vec::new();
                            for &bs in branch_scopes {
                                if let Some(&(_, ver_idx)) = branch_vers.iter().filter(|(s, _)| *s == bs).last() {
                                    // Branch assigned: reference the branch version
                                    let sym_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, ver_idx));
                                    merge_exprs.push(sym_ref);
                                } else {
                                    // Branch narrowed: strip nil from pre-chain version
                                    let pre_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, pre_ver));
                                    let stripped = self.ir.push_expr(Expr::StripNil(pre_ref));
                                    merge_exprs.push(stripped);
                                }
                            }
                            // Implicit else: when there's no explicit else block,
                            // the path where all conditions were false keeps the
                            // pre-if version of the variable.
                            if merge.has_implicit_else {
                                let pre_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, pre_ver));
                                merge_exprs.push(pre_ref);
                            }

                            let merge_expr = self.ir.push_expr(Expr::BranchMerge(merge_exprs));
                            let node = self.ir.symbols[*sym_idx].versions[pre_ver].def_node;
                            self.ir.symbols[*sym_idx].versions.push(SymbolVersion {
                                def_node: node,
                                type_source: Some(merge_expr),
                                resolved_type: None,
                                type_args: Vec::new(),
                                created_in_scope: scope_idx,
                            });
                        }
                    } else {
                        mi += 1;
                    }
                }
            }

            let stmt_index = frame.next_stmt;
            frame.next_stmt += 1;
            // Apply @cast annotations from comments preceding this statement
            self.scan_cast_annotations(statements[stmt_index].syntax(), scope_idx);
            match &statements[stmt_index] {
                Statement::LocalAssign(assign) => {
                    let node = SyntaxNodePtr::new(assign.syntax());
                    let name_list = assign
                        .name_list()
                        .expect("LocalAssign should have a name_list");
                    let names = name_list.names();
                    let name_tokens = name_list.name_tokens();
                    let expressions = assign
                        .expression_list()
                        .map(|el| el.expressions())
                        .unwrap_or_default();

                    // D7: redundant-value / unbalanced-assignments
                    let last_is_multi = matches!(
                        expressions.last(),
                        Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
                    );
                    if !last_is_multi && !expressions.is_empty() {
                        if expressions.len() > names.len() {
                            if let Some(extra) = expressions.get(names.len()) {
                                let r = extra.syntax().text_range();
                                crate::diagnostics::redundant_value::check(
                                    &mut self.diagnostics,
                                    names.len(), expressions.len(),
                                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                );
                            }
                        } else if names.len() > expressions.len() {
                            let r = assign.syntax().text_range();
                            crate::diagnostics::unbalanced_assignments::check(
                                &mut self.diagnostics,
                                names.len(), expressions.len(),
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                        }
                    }

                    // Collect multi-return siblings for return-only overload narrowing
                    let mut multi_return_group: Vec<(usize, SymbolIndex)> = Vec::new();

                    for (index, name) in names.iter().enumerate() {
                        let expression = expressions.get(index);

                        // D1: redefined-local — check if name already exists in current scope
                        if !name.starts_with('_') {
                            let id = SymbolIdentifier::Name(name.clone());
                            if let Some(&existing_idx) = self.ir.scopes[scope_idx].symbols.get(&id) {
                                if self.ir.symbols[existing_idx].scope_idx == scope_idx {
                                    if let Some(tok) = name_tokens.get(index) {
                                        let r = tok.text_range();
                                        crate::diagnostics::redefined_local::check(
                                            &mut self.diagnostics, name,
                                            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                        );
                                    }
                                }
                            }
                        }

                        if let Some(Expression::Function(func)) = expression {
                            // Function: insert symbol first (so function can be recursive),
                            // then create function scope
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            if let Some(tok) = name_tokens.get(index) {
                                let r = tok.text_range();
                                self.deferred.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = self.ir.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, assign.syntax());
                            let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                            self.ir.set_type_source(symbol_idx, expr_id);
                            if let Some(inner_block) = func.block() {
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                    constructor_of: None,
                                });
                            }
                        } else {
                            // Non-function: lower RHS BEFORE insert_symbol so that
                            // `local x = x + 1` resolves the old `x`, not the new one
                            let type_source = if let Some(expr) = expression {
                                if let Some(n) = crate::annotations::is_select_varargs(expr) {
                                    // select(2, ...) → treat as addon namespace table
                                    if n == 2 {
                                        let table_idx = self.ir.tables.len();
                                        let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                                            self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
                                        } else {
                                            HashMap::new()
                                        };
                                        self.ir.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                                        Some(self.ir.push_expr(Expr::TableConstructor(table_idx)))
                                    } else if n == 1 {
                                        Some(self.ir.push_expr(Expr::VarArgs(0, func_id.is_none())))
                                    } else {
                                        Some(self.lower_expression(expr, scope_idx))
                                    }
                                } else {
                                    Some(self.lower_expression(expr, scope_idx))
                                }
                            } else if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                if index >= expressions.len() {
                                    // Multi-return: this name gets a later return value
                                    let ret_index = index - (expressions.len() - 1);
                                    Some(self.lower_function_call(call, scope_idx, ret_index, false))
                                } else {
                                    None
                                }
                            } else if matches!(expressions.last(), Some(Expression::VarArgs(_))) {
                                if index >= expressions.len() {
                                    // Multi-value varargs: this name gets a later vararg value
                                    let ret_index = index - (expressions.len() - 1);
                                    if func_id.is_none() && ret_index == 1 {
                                        // WoW passes (addonName, addonTable) at file scope
                                        let table_idx = self.ir.tables.len();
                                        let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                                            self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
                                        } else {
                                            HashMap::new()
                                        };
                                        self.ir.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                                        Some(self.ir.push_expr(Expr::TableConstructor(table_idx)))
                                    } else {
                                        Some(self.ir.push_expr(Expr::VarArgs(ret_index, func_id.is_none())))
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            if let Some(tok) = name_tokens.get(index) {
                                let r = tok.text_range();
                                self.deferred.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
                            if let Some(expr_id) = type_source {
                                self.ir.set_type_source(symbol_idx, expr_id);
                                // Track multi-return siblings from function calls
                                if let Expr::FunctionCall { ret_index, .. } = self.ir.expr(expr_id) {
                                    multi_return_group.push((*ret_index, symbol_idx));
                                }
                            }
                            // Track `local t = type(x)` as a type-of alias
                            if let Some(Expression::FunctionCall(call)) = expression {
                                if let Some(target_sym) = self.extract_type_call_target(&call, scope_idx) {
                                    self.type_of_aliases.insert(symbol_idx, target_sym);
                                }
                            }
                            // Apply @type and @class annotations (first variable only)
                            if index == 0 {
                                let annotations = extract_annotations(assign.syntax());
                                if let Some(ref at) = annotations.var_type {
                                    if let Some(vt) = self.resolve_annotation_type_mut_gen(at, &[]) {
                                        // Check for missing/excess fields when @type points to a class and RHS is a table constructor
                                        if let ValueType::Table(Some(class_table_idx)) = &vt {
                                            let class_table_idx = *class_table_idx;
                                            if self.ir.table(class_table_idx).class_name.is_some() {
                                                if let Some(rhs_expr_id) = self.ir.symbols[symbol_idx]
                                                    .versions.last()
                                                    .and_then(|v| v.type_source)
                                                {
                                                    if let Some(rhs_table_idx) = self.ir.find_table_index(rhs_expr_id) {
                                                        let provided: Vec<String> = self.ir.table(rhs_table_idx)
                                                            .fields.keys().cloned().collect();
                                                        if !provided.is_empty() {
                                                            if let Some(&(s, e)) = self.ir.table_ranges.iter()
                                                                .find(|(_, idx)| **idx == rhs_table_idx)
                                                                .map(|(range, _)| range)
                                                            {
                                                                self.deferred.missing_fields_checks.push(MissingFieldsCheck {
                                                                    class_table_idx,
                                                                    provided_fields: provided,
                                                                    start: s,
                                                                    end: e,
                                                                });
                                                                // Also check for excess fields via assign-type-mismatch path
                                                                self.deferred.assign_type_checks.push(AssignTypeCheck {
                                                                    expected: vt.clone(),
                                                                    actual_expr: rhs_expr_id,
                                                                    var_name: name.clone(),
                                                                    start: s,
                                                                    end: e,
                                                                });
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        // Store resolved type args for parameterized class annotations
                                        // (e.g. @type Future<number> → type_args = [Number])
                                        if let crate::annotations::AnnotationType::Parameterized(_, type_arg_annotations) = at {
                                            let type_args: Vec<ValueType> = type_arg_annotations.iter()
                                                .filter_map(|ta| self.resolve_annotation_type_mut_gen(ta, &[]))
                                                .collect();
                                            if !type_args.is_empty() {
                                                if let Some(ver) = self.ir.symbols[symbol_idx].versions.last_mut() {
                                                    ver.type_args = type_args;
                                                }
                                            }
                                        }
                                        // D2: track annotation for assign-type-mismatch
                                        self.symbol_type_annotations.insert(symbol_idx, vt);
                                    }
                                    // Check for undefined class references in @type
                                    // Use the @type comment token range so the diagnostic appears on the annotation
                                    let comment_ranges = Self::collect_preceding_annotation_ranges(assign.syntax());
                                    let (type_start, type_end) = comment_ranges.iter()
                                        .find(|(text, _, _)| text.starts_with("---@type"))
                                        .map(|(_, s, e)| (*s, *e))
                                        .unwrap_or_else(|| {
                                            let s = u32::from(assign.syntax().text_range().start()) as usize;
                                            (s, s + name.len())
                                        });
                                    let no_generics: Vec<(String, Option<String>)> = Vec::new();
                                    let mut diags = Vec::new();
                                    self.check_annotation_type_names(at, &no_generics, type_start, type_end, &mut diags);
                                    self.diagnostics.extend(diags);
                                }
                                // Check preceding annotations, then fall back to inline ---@class comment
                                // (only on the same line — stop at first newline)
                                let effective_class = annotations.class.clone().or_else(|| {
                                    let mut past_newline = false;
                                    for token in assign.syntax().descendants_with_tokens() {
                                        if let rowan::NodeOrToken::Token(t) = token {
                                            if t.kind() == SyntaxKind::Newline {
                                                past_newline = true;
                                            } else if past_newline {
                                                break;
                                            } else if t.kind() == SyntaxKind::Comment {
                                                let text = t.text();
                                                let content = text.trim_start_matches('-').trim();
                                                if let Some(rest) = content.strip_prefix("@class") {
                                                    return rest.trim().split_whitespace().next()
                                                        .map(|s| s.trim_end_matches(':').to_string());
                                                }
                                            }
                                        }
                                    }
                                    None
                                });
                                if let Some(ref class_name) = effective_class {
                                    if let Some(&class_table_idx) = self.ir.classes.get(class_name) {
                                        // Merge runtime table fields into the class table.
                                        // Skip merge for external tables (>= EXT_BASE) as they are immutable.
                                        if class_table_idx < EXT_BASE {
                                            if let Some(rhs_expr_id) = self.ir.symbols[symbol_idx]
                                                .versions.last()
                                                .and_then(|v| v.type_source)
                                            {
                                                if let Some(rhs_table_idx) = self.ir.find_table_index(rhs_expr_id) {
                                                    if rhs_table_idx != class_table_idx && rhs_table_idx < EXT_BASE {
                                                        // Capture provided field names before draining
                                                        let provided: Vec<String> = self.ir.tables[rhs_table_idx]
                                                            .fields.keys().cloned().collect();
                                                        let runtime_fields: Vec<(String, FieldInfo)> =
                                                            self.ir.tables[rhs_table_idx].fields.drain().collect();
                                                        for (name, field_info) in runtime_fields {
                                                            self.ir.tables[class_table_idx].fields
                                                                .entry(name).or_insert(field_info);
                                                        }
                                                        // Record missing-fields check if constructor has fields
                                                        if !provided.is_empty() {
                                                            if let Some(&(s, e)) = self.ir.table_ranges.iter()
                                                                .find(|(_, idx)| **idx == rhs_table_idx)
                                                                .map(|(range, _)| range)
                                                            {
                                                                self.deferred.missing_fields_checks.push(MissingFieldsCheck {
                                                                    class_table_idx,
                                                                    provided_fields: provided,
                                                                    start: s,
                                                                    end: e,
                                                                });
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        let expr_id = self.ir.push_expr(Expr::Literal(
                                            ValueType::Table(Some(class_table_idx))
                                        ));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                    }
                                }
                                // @defclass: if this variable was identified as a defclass target,
                                // eagerly set its type to the auto-created class table
                                // Inline ---@type on expression (e.g. `local x = {} ---@type Foo`)
                                // Also checks inside table constructor opening: `{ ---@type Foo ... }`
                                if annotations.var_type.is_none() && effective_class.is_none() {
                                    if let Some(expr) = expression {
                                        let inline_at = Self::extract_inline_type(expr.syntax())
                                            .or_else(|| {
                                                if let Expression::TableConstructor(tc) = expr {
                                                    Self::extract_table_constructor_type(tc.syntax())
                                                } else {
                                                    None
                                                }
                                            });
                                        if let Some(inline_at) = inline_at {
                                            if let Some(vt) = self.resolve_annotation_type_mut_gen(&inline_at, &[]) {
                                                let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                                                self.ir.set_type_source(symbol_idx, expr_id);
                                                self.symbol_type_annotations.insert(symbol_idx, vt);
                                            }
                                        }
                                    }
                                }
                                if annotations.var_type.is_none() && effective_class.is_none() {
                                    if let Some(&defclass_table_idx) = self.defclass_vars.get(name) {
                                        // Merge table literal argument fields into the defclass table,
                                        // replacing prescan placeholders with real lowered expressions.
                                        // Skip merge for external tables (>= EXT_BASE) as they are immutable.
                                        if defclass_table_idx < EXT_BASE {
                                            if let Some(call_expr_id) = type_source {
                                                if let Expr::FunctionCall { args, .. } = self.ir.expr(call_expr_id).clone() {
                                                    for &arg_expr_id in &args {
                                                        if let Expr::TableConstructor(tc_idx) = self.ir.expr(arg_expr_id) {
                                                            let tc_idx = *tc_idx;
                                                            let tc_fields: Vec<(String, FieldInfo)> =
                                                                self.ir.tables[tc_idx].fields.iter()
                                                                    .map(|(k, v)| (k.clone(), v.clone()))
                                                                    .collect();
                                                            for (fname, finfo) in tc_fields {
                                                                self.ir.tables[defclass_table_idx].fields
                                                                    .insert(fname, finfo);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        let expr_id = self.ir.push_expr(Expr::Literal(
                                            ValueType::Table(Some(defclass_table_idx))
                                        ));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                    }
                                }
                            }
                        }
                    }

                    // Register multi-return sibling groups (2+ returns from same call)
                    if multi_return_group.len() >= 2 {
                        for &(_, sym_idx) in &multi_return_group {
                            self.multi_return_siblings.insert(sym_idx, multi_return_group.clone());
                        }
                    }
                },
                Statement::Do(group) => {
                    if let Some(inner_block) = group.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                        });
                    }
                },
                Statement::While(while_loop) => {
                    if let Some(cond) = while_loop.condition() {
                        self.lower_expression(&cond, scope_idx);
                    }
                    if let Some(inner_block) = while_loop.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        if let Some(cond) = while_loop.condition() {
                            self.analyze_nil_guard(&cond, scope_idx, new_scope_idx, true);
                        }
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                        });
                    }
                },
                Statement::Repeat(repeat_loop) => {
                    if let Some(cond) = repeat_loop.condition() {
                        self.lower_expression(&cond, scope_idx);
                    }
                    if let Some(inner_block) = repeat_loop.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                        });
                    }
                },
                Statement::If(if_chain) => {
                    let branches = if_chain.if_branches();
                    let mut branch_scopes: Vec<ScopeIndex> = Vec::new();
                    for (i, branch) in branches.iter().enumerate() {
                        if i == 0 {
                            // First branch: lower condition in parent scope
                            if let Some(cond) = branch.expression() {
                                self.lower_expression(&cond, scope_idx);
                            }
                        }
                        if let Some(inner_block) = branch.block() {
                            let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                            branch_scopes.push(new_scope_idx);
                            // elseif branches: apply inverse narrowing from ALL preceding
                            // branches' conditions since they must have been false to reach
                            // here, then lower the elseif condition in the narrowed scope
                            // so that NilCheckSites from the condition see the narrowing.
                            if i > 0 {
                                for prev in &branches[..i] {
                                    if let Some(prev_cond) = prev.expression() {
                                        self.analyze_nil_guard(&prev_cond, scope_idx, new_scope_idx, false);
                                    }
                                }
                                if let Some(cond) = branch.expression() {
                                    self.lower_expression(&cond, new_scope_idx);
                                }
                            }
                            if let Some(cond) = branch.expression() {
                                self.analyze_nil_guard(&cond, scope_idx, new_scope_idx, true);
                            }
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                                constructor_of,
                            });
                        }
                    }
                    let has_else = if_chain.else_branch().is_some();
                    if let Some(else_branch) = if_chain.else_branch() {
                        if let Some(inner_block) = else_branch.block() {
                            let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                            branch_scopes.push(new_scope_idx);
                            // Apply inverse narrowing from ALL branches' conditions
                            for branch in &branches {
                                if let Some(cond) = branch.expression() {
                                    self.analyze_nil_guard(&cond, scope_idx, new_scope_idx, false);
                                }
                            }
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                                constructor_of,
                            });
                        }
                    }
                    // Early-exit narrowing: for each prefix of branches that all
                    // always exit, apply inverse narrowing from their conditions.
                    // E.g. `if not x and c then return elseif not x then return end`
                    // narrows x as non-nil after the chain since both conditions were false.
                    let mut first_branch_exits = false;
                    for (bi, branch) in branches.iter().enumerate() {
                        let Some(inner_block) = branch.block() else { break };
                        if !Self::block_always_exits(&inner_block) { break; }
                        if bi == 0 { first_branch_exits = true; }
                        if let Some(cond) = branch.expression() {
                            self.analyze_early_exit_guard(&cond, scope_idx);
                        }
                    }
                    // Ensure-initialized: `if not x.f then x.f = val end`
                    // Only for single-branch if without else.
                    if branches.len() == 1 && !has_else {
                        if let Some(inner_block) = branches[0].block() {
                            if let Some(cond) = branches[0].expression() {
                                self.analyze_ensure_initialized(&cond, &inner_block, scope_idx);
                            }
                        }
                    }
                    // Record for post-branch merge: when all branches assign/narrow
                    // a variable, create a merged version in the parent scope.
                    // For if-without-else (when the block doesn't always exit),
                    // the implicit else contributes the pre-if version to the merge.
                    if has_else || (!first_branch_exits && !branch_scopes.is_empty()) {
                        pending_branch_merges.push(PendingBranchMerge {
                            parent_scope: scope_idx,
                            branch_scopes,
                            has_implicit_else: !has_else,
                        });
                    }
                },
                Statement::ForCountLoop(for_loop) => {
                    if let Some(expr_list) = for_loop.expression_list() {
                        for expr in expr_list.expressions() {
                            self.lower_expression(&expr, scope_idx);
                        }
                    }
                    if let Some(inner_block) = for_loop.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        // Register scope for entire for-loop so variable names in the header resolve
                        self.ir.block_scopes.push((for_loop.syntax().text_range(), new_scope_idx));
                        if let Some(name) = for_loop.name() {
                            let node = SyntaxNodePtr::new(for_loop.syntax());
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name), new_scope_idx, node);
                            let expr_id = self.ir.push_expr(Expr::Literal(ValueType::Number));
                            self.ir.set_type_source(symbol_idx, expr_id);
                        }
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                        });
                    }
                },
                Statement::ForInLoop(for_in) => {
                    let mut first_expr_id = None;
                    if let Some(expr_list) = for_in.expression_list() {
                        for (i, expr) in expr_list.expressions().iter().enumerate() {
                            let eid = self.lower_expression(expr, scope_idx);
                            if i == 0 { first_expr_id = Some(eid); }
                        }
                    }
                    if let Some(inner_block) = for_in.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        // Register scope for entire for-loop so variable names in the header resolve
                        self.ir.block_scopes.push((for_in.syntax().text_range(), new_scope_idx));
                        if let Some(name_list) = for_in.name_list() {
                            let node = SyntaxNodePtr::new(for_in.syntax());
                            for (i, name) in name_list.names().iter().enumerate() {
                                let sym_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), new_scope_idx, node);
                                if let Some(iter_eid) = first_expr_id {
                                    let forin_expr = self.ir.push_expr(Expr::ForInVar {
                                        iterator_call: iter_eid,
                                        var_index: i,
                                    });
                                    self.ir.set_type_source(sym_idx, forin_expr);
                                }
                            }
                        }
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                        });
                    }
                },
                Statement::FunctionDefinition(func) => {
                    let node = SyntaxNodePtr::new(func.syntax());
                    if let Some(name) = func.name() {
                        // Simple name: function foo() / local function foo()
                        if !func.is_local() && self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_none() {
                            if let Some(name_tok) = func.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name)
                            {
                                let r = name_tok.text_range();
                                self.deferred.created_globals.push(CreatedGlobal {
                                    name: name.clone(),
                                    start: u32::from(r.start()),
                                    end: u32::from(r.end()),
                                });
                            }
                        }
                        let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name), scope_idx, node);
                        if func.is_local() {
                            // Find name token for position
                            if let Some(name_tok) = func.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name)
                            {
                                let r = name_tok.text_range();
                                self.deferred.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
                        }
                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                        let func_idx = self.ir.functions.len() - 1;
                        self.apply_annotations(func_idx, scope_idx, func.syntax());
                        let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                        self.ir.set_type_source(symbol_idx, expr_id);
                        if let Some(inner_block) = func.block() {
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id: Some(func_idx),
                                constructor_of: None,
                            });
                        }
                    } else if let Some(ident) = func.identifier() {
                        let names = ident.names();
                        if names.len() == 1 {
                            // Global function with Identifier wrapper: function foo()
                            let name = &names[0];
                            if self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_none() {
                                if let Some(name_tok) = ident.syntax().children_with_tokens()
                                    .filter_map(|c| c.into_token())
                                    .find(|t| t.kind() == SyntaxKind::Name)
                                {
                                    let r = name_tok.text_range();
                                    self.deferred.created_globals.push(CreatedGlobal {
                                        name: name.clone(),
                                        start: u32::from(r.start()),
                                        end: u32::from(r.end()),
                                    });
                                }
                            }
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = self.ir.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, func.syntax());
                            let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                            self.ir.set_type_source(symbol_idx, expr_id);
                            if let Some(inner_block) = func.block() {
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                    constructor_of: None,
                                });
                            }
                        } else if names.len() >= 2 {
                            let root_name = &names[0];
                            let field_name = &names[names.len() - 1];
                            let is_method = ident.is_call_to_self();
                            let method_visibility = extract_annotations(func.syntax()).visibility;

                            let new_scope_idx = self.insert_function_definition(func, scope_idx, is_method);
                            let func_idx = self.ir.functions.len() - 1;
                            // For methods on a class, pass the class name so @return ClassName
                            // is treated as @return self (needed for builder pattern)
                            let owner_class = if is_method && (self.ir.classes.contains_key(root_name) || self.ir.ext.classes.contains_key(root_name)) {
                                Some(root_name.as_str())
                            } else {
                                None
                            };
                            self.apply_annotations_with_owner(func_idx, scope_idx, func.syntax(), owner_class);
                            let func_def_expr = self.ir.push_expr(Expr::FunctionDef(func_idx));

                            // Mark root symbol as referenced (e.g. `Container` in `function Container:Foo()`)
                            if let Some(root_sym_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                                self.referenced_symbols.insert(root_sym_idx);

                                // Give `self` a type pointing to the table
                                if is_method {
                                    let self_sym_idx = self.ir.functions[func_idx].args[0];
                                    let ver_idx = self.ir.version_for_scope(root_sym_idx, scope_idx);
                                    let self_expr = self.ir.push_expr(Expr::SymbolRef(root_sym_idx, ver_idx));
                                    self.ir.set_type_source(self_sym_idx, self_expr);
                                }
                            }

                            // Record as field on the table, walking intermediate names for 3+ level paths
                            if let Some(mut table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                let mut resolved = true;
                                let mut accessor_visibility: Option<crate::annotations::Visibility> = None;
                                for intermediate in &names[1..names.len()-1] {
                                    // Check for transparent @accessor on the current table
                                    if let Some(vis) = self.ir.get_accessor(table_idx, intermediate.as_str()) {
                                        accessor_visibility = Some(vis);
                                        continue;
                                    }
                                    if let Some(field) = self.ir.get_field(table_idx, intermediate) {
                                        let field_expr = field.expr;
                                        if let Some(sub_idx) = self.ir.find_table_index(field_expr) {
                                            table_idx = sub_idx;
                                        } else {
                                            resolved = false;
                                            break;
                                        }
                                    } else {
                                        resolved = false;
                                        break;
                                    }
                                }
                                if resolved {
                                    let final_visibility = accessor_visibility.unwrap_or(method_visibility);
                                    let fi = FieldInfo {
                                        expr: func_def_expr,
                                        visibility: final_visibility,
                                        annotation: None,
                                        annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                                        extra_exprs: Vec::new(),
                                    };
                                    if table_idx < EXT_BASE {
                                        self.ir.tables[table_idx].fields.insert(field_name.clone(), fi);
                                    } else {
                                        self.ir.insert_overlay_field(table_idx, field_name.clone(), fi);
                                    }
                                }
                            }

                            if let Some(inner_block) = func.block() {
                                // Detect constructor methods: either annotated with @constructor
                                // or overriding a constructor inherited from a parent class
                                let is_constructor = if is_method {
                                    if self.ir.functions[func_idx].constructor {
                                        // Explicitly annotated — also register on the table
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                            if table_idx < EXT_BASE {
                                                self.ir.tables[table_idx].constructors.insert(field_name.clone());
                                            }
                                            Some(table_idx)
                                        } else { None }
                                    } else if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                        // Check if this method name is a constructor on this table,
                                        // inherited from a parent class, or globally declared via
                                        // @constructor on any class (e.g. Class<S> declares __init)
                                        if self.table(table_idx).constructors.contains(field_name.as_str()) {
                                            Some(table_idx)
                                        } else if self.table(table_idx).parent_classes.iter().any(|&pi| {
                                            self.table(pi).constructors.contains(field_name.as_str())
                                        }) {
                                            Some(table_idx)
                                        } else if self.ir.ext.constructor_method_names.contains(field_name.as_str())
                                            || self.ir.tables.iter().any(|t| t.constructors.contains(field_name.as_str()))
                                        {
                                            Some(table_idx)
                                        } else { None }
                                    } else { None }
                                } else { None };
                                // Constructor return check for inherited constructors
                                // (explicit @constructor is checked in apply_annotations)
                                if is_constructor.is_some()
                                    && !self.ir.functions[func_idx].constructor
                                    && !self.ir.functions[func_idx].return_annotations.is_empty()
                                {
                                    let r = func.syntax().text_range();
                                    crate::diagnostics::constructor_return::check(
                                        &mut self.diagnostics,
                                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                    );
                                }
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                    constructor_of: is_constructor,
                                });
                            }
                        }
                    }
                },
                Statement::Return(ret) => {
                    if let Some(func_id) = func_id {
                        self.functions_with_returns.insert(func_id);

                        let expr_count = ret.expression_list()
                            .map(|el| el.expressions().len())
                            .unwrap_or(0);
                        let expected_count = self.ir.functions[func_id].return_annotations.len();

                        // D3: missing-return-value — return has fewer values than @return declares
                        // Skip if last expression is a function call or varargs, since
                        // those can expand to fill multiple return slots at runtime.
                        let last_is_multi = ret.expression_list()
                            .map(|el| matches!(
                                el.expressions().last(),
                                Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
                            ))
                            .unwrap_or(false);
                        // Suppress for functions with return-only overloads that include a nil/empty variant
                        let has_nil_overload = self.ir.functions[func_id].overloads.iter().any(|o| {
                            o.is_return_only && (o.returns.is_empty() || (o.returns.len() == 1 && o.returns[0] == ValueType::Nil))
                        });
                        if expr_count < expected_count && !last_is_multi && !has_nil_overload {
                            let r = ret.syntax().text_range();
                            let end = trimmed_node_end(ret.syntax()) as usize;
                            // All omitted return positions are optional → suppress warning
                            let omitted_all_optional = self.ir.functions[func_id].return_annotations[expr_count..]
                                .iter().all(|t| t.contains_nil());
                            // Bare return with all-optional return types → hint instead of warning
                            let all_returns_nullable = expr_count == 0 && omitted_all_optional;
                            if all_returns_nullable {
                                crate::diagnostics::implicit_nil_return::check(
                                    &mut self.diagnostics,
                                    expected_count,
                                    u32::from(r.start()) as usize, end,
                                );
                            } else if !omitted_all_optional {
                                crate::diagnostics::missing_return_value::check(
                                    &mut self.diagnostics,
                                    expected_count, expr_count,
                                    u32::from(r.start()) as usize, end,
                                );
                            }
                        }

                        // D3b: redundant-return-value — return has more values than @return declares
                        if expected_count > 0 && expr_count > expected_count {
                            if let Some(el) = ret.expression_list() {
                                let exprs = el.expressions();
                                if let Some(extra) = exprs.get(expected_count) {
                                    let r = extra.syntax().text_range();
                                    crate::diagnostics::redundant_return_value::check(
                                        &mut self.diagnostics,
                                        expected_count, expr_count,
                                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                    );
                                }
                            }
                        }

                        if let Some(expr_list) = ret.expression_list() {
                            let node = SyntaxNodePtr::new(ret.syntax());
                            let expressions = expr_list.expressions();
                            let mut return_exprs = Vec::new();
                            for (index, expr) in expressions.iter().enumerate() {
                                let r = expr.syntax().text_range();
                                let expr_id = self.lower_expression(expr, scope_idx);
                                return_exprs.push(expr_id);
                                self.deferred.return_type_checks.push(ReturnTypeCheck {
                                    func_id, ret_index: index, rhs_expr: expr_id,
                                    scope_idx,
                                    start: u32::from(r.start()), end: trimmed_node_end(expr.syntax()),
                                });
                                let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                self.ir.set_type_source(symbol_idx, expr_id);
                                let func = self.ir.functions.get_mut(func_id).unwrap();
                                if !func.rets.contains(&symbol_idx) {
                                    func.rets.push(symbol_idx);
                                }
                            }
                            // Expand multi-return: when the last expression is a function
                            // call or varargs, it can fill additional return slots beyond
                            // the explicit expression count.
                            if expressions.len() < expected_count {
                                if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                    let r = call.syntax().text_range();
                                    let end = trimmed_node_end(call.syntax());
                                    for index in expressions.len()..expected_count {
                                        let ret_index = index - (expressions.len() - 1);
                                        let expr_id = self.lower_function_call(call, scope_idx, ret_index, false);
                                        self.deferred.return_type_checks.push(ReturnTypeCheck {
                                            func_id, ret_index: index, rhs_expr: expr_id,
                                            scope_idx,
                                            start: u32::from(r.start()), end,
                                        });
                                        let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        let func = self.ir.functions.get_mut(func_id).unwrap();
                                        if !func.rets.contains(&symbol_idx) {
                                            func.rets.push(symbol_idx);
                                        }
                                    }
                                } else if matches!(expressions.last(), Some(Expression::VarArgs(_))) {
                                    let last_expr = expressions.last().unwrap();
                                    let r = last_expr.syntax().text_range();
                                    let end = trimmed_node_end(last_expr.syntax());
                                    for index in expressions.len()..expected_count {
                                        let ret_index = index - (expressions.len() - 1);
                                        let expr_id = self.ir.push_expr(Expr::VarArgs(ret_index, false));
                                        self.deferred.return_type_checks.push(ReturnTypeCheck {
                                            func_id, ret_index: index, rhs_expr: expr_id,
                                            scope_idx,
                                            start: u32::from(r.start()), end,
                                        });
                                        let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        let func = self.ir.functions.get_mut(func_id).unwrap();
                                        if !func.rets.contains(&symbol_idx) {
                                            func.rets.push(symbol_idx);
                                        }
                                    }
                                }
                            }
                            // Record grouped-return check if function has return-only overloads
                            if self.ir.functions[func_id].overloads.iter().any(|o| o.is_return_only) {
                                let r = ret.syntax().text_range();
                                self.deferred.grouped_return_checks.push(GroupedReturnCheck {
                                    func_id,
                                    return_exprs,
                                    start: u32::from(r.start()),
                                    end: u32::from(r.end()),
                                });
                            }
                        }
                    }
                },
                Statement::Assign(assign) => {
                    let node = SyntaxNodePtr::new(assign.syntax());
                    if let Some(var_list) = assign.variable_list() {
                        let identifiers = var_list.identifiers();
                        let expressions = assign
                            .expression_list()
                            .map(|el| el.expressions())
                            .unwrap_or_default();
                        // D7: redundant-value / unbalanced-assignments (non-local)
                        let last_is_multi = matches!(
                            expressions.last(),
                            Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
                        );
                        if !last_is_multi && !expressions.is_empty() {
                            if expressions.len() > identifiers.len() {
                                if let Some(extra) = expressions.get(identifiers.len()) {
                                    let r = extra.syntax().text_range();
                                    crate::diagnostics::redundant_value::check(
                                        &mut self.diagnostics,
                                        identifiers.len(), expressions.len(),
                                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                    );
                                }
                            } else if identifiers.len() > expressions.len() {
                                let r = assign.syntax().text_range();
                                crate::diagnostics::unbalanced_assignments::check(
                                    &mut self.diagnostics,
                                    identifiers.len(), expressions.len(),
                                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                );
                            }
                        }

                        // Collect multi-return siblings for return-only overload narrowing
                        let mut multi_return_group: Vec<(usize, SymbolIndex)> = Vec::new();

                        for (index, ident) in identifiers.iter().enumerate() {
                            let names = ident.names();
                            // Lower bracket index expressions on the LHS (e.g. t[x] = v,
                            // info[part].width = w, global.tbl[k1][k2] = v)
                            // Recursively walk the entire Identifier subtree to find
                            // Expression nodes (bracket keys) at any nesting depth.
                            {
                                let mut id_stack: Vec<SyntaxNode> = vec![ident.syntax().clone()];
                                while let Some(node) = id_stack.pop() {
                                    for child in node.children() {
                                        if child.kind() == SyntaxKind::Expression {
                                            if let Some(expr) = Expression::cast(child) {
                                                self.lower_expression(&expr, scope_idx);
                                            }
                                        } else if child.kind() == SyntaxKind::Identifier {
                                            id_stack.push(child);
                                        }
                                    }
                                }
                                // Find the root name by walking down the Identifier chain
                                let mut cur = ident.syntax().clone();
                                loop {
                                    let name = cur.children_with_tokens().find_map(|c| {
                                        if let rowan::NodeOrToken::Token(t) = c {
                                            if t.kind() == SyntaxKind::Name { return Some(t.text().to_string()); }
                                        }
                                        None
                                    });
                                    if let Some(name) = name {
                                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(name), scope_idx) {
                                            self.referenced_symbols.insert(sym_idx);
                                        }
                                        break;
                                    }
                                    if let Some(child) = cur.children().find(|c| c.kind() == SyntaxKind::Identifier) {
                                        cur = child;
                                    } else {
                                        break;
                                    }
                                }
                            }
                            // When names is empty (complex LHS with nested Identifiers
                            // e.g. info[part].width, settings.profs[name].link), lower
                            // the RHS expression directly and skip the normal handler.
                            if names.is_empty() && ident.syntax().children().any(|c| c.kind() == SyntaxKind::Identifier) {
                                if let Some(expr) = expressions.get(index) {
                                    self.lower_expression(expr, scope_idx);
                                }
                                continue;
                            }
                            if let Some(root_name) = names.first() {
                                let expression = expressions.get(index);

                                if names.len() > 1 {
                                    // Dotted assignment: t.x = expr
                                    let field_name = &names[names.len() - 1];

                                    // Record nil-check site for the root symbol
                                    if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                                        self.referenced_symbols.insert(sym_idx);
                                        let sym_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, self.ir.version_for_scope(sym_idx, scope_idx)));
                                        // Use the field name token's range for the diagnostic
                                        let name_tokens: Vec<_> = ident.syntax().children_with_tokens()
                                            .filter_map(|t| t.into_token())
                                            .filter(|t| t.kind() == SyntaxKind::Name)
                                            .collect();
                                        if let Some(field_token) = name_tokens.get(1) {
                                            let r = field_token.text_range();
                                            self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: sym_ref, start: u32::from(r.start()), end: u32::from(r.end()) });
                                        }
                                    }

                                    if let Some(Expression::Function(func)) = expression {
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.ir.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let func_def_expr = self.ir.push_expr(Expr::FunctionDef(func_idx));
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                            if names.len() > 2 {
                                                // Deep chain (e.g. self._plot.method = function ...):
                                                // defer to post-fixpoint resolution
                                                self.deferred.deep_field_injections.push(DeepFieldInjection {
                                                    root_name: root_name.clone(),
                                                    intermediates: names[1..names.len()-1].to_vec(),
                                                    field_name: field_name.clone(),
                                                    expr_id: func_def_expr,
                                                    scope_idx,
                                                });
                                            } else {
                                                let field_lateinit = self.ir.get_field(table_idx, field_name).map_or(false, |f| f.lateinit);
                                                if let Some(expected_vt) = self.ir.get_field(table_idx, field_name).and_then(|f| f.annotation.clone()) {
                                                    let r = func.syntax().text_range();
                                                    self.deferred.field_type_checks.push(FieldTypeCheck {
                                                        expected: expected_vt, actual_expr: func_def_expr, field_name: field_name.clone(),
                                                        start: u32::from(r.start()), end: u32::from(r.end()),
                                                        lateinit: field_lateinit,
                                                    });
                                                }
                                                let fi = FieldInfo {
                                                    expr: func_def_expr,
                                                    visibility: crate::annotations::Visibility::Public,
                                                    annotation: None,
                                                    annotation_text: None,
                                                    annotation_type_raw: None,
                                                    lateinit: false,
                                                    extra_exprs: Vec::new(),
                                                };
                                                if table_idx < EXT_BASE {
                                                    self.ir.tables[table_idx].fields.insert(field_name.clone(), fi);
                                                } else {
                                                    self.ir.insert_overlay_field(table_idx, field_name.clone(), fi);
                                                }
                                                let r = ident.syntax().text_range();
                                                self.deferred.field_assignment_sites.push(FieldAssignmentSite {
                                                    table_idx, field_name: field_name.clone(), scope_idx,
                                                    block_stmt_index: stmt_index as u32,
                                                    start: u32::from(r.start()), end: u32::from(r.end()),
                                                });
                                            }
                                        } else if names.len() == 2 {
                                            // Table not found during Phase 1 (e.g. type comes from
                                            // function return) — defer to post-fixpoint resolution.
                                            let r = ident.syntax().text_range();
                                            self.deferred.deferred_field_assignments.push(DeferredFieldAssignment {
                                                root_name: root_name.clone(),
                                                field_name: field_name.clone(),
                                                expr_id: func_def_expr,
                                                scope_idx,
                                                ident_start: u32::from(r.start()),
                                                ident_end: u32::from(r.end()),
                                            });
                                        }
                                        if let Some(inner_block) = func.block() {
                                            stack.push(Frame {
                                                block: inner_block,
                                                next_stmt: 0,
                                                scope_idx: new_scope_idx,
                                                func_id: Some(func_idx),
                                                constructor_of: None,
                                            });
                                        }
                                    } else if let Some(expr) = expression {
                                        let expr_id = self.lower_expression(expr, scope_idx);
                                        // Check for inline ---@type annotation after the expression
                                        let inline_type = Self::extract_inline_type(expr.syntax());
                                        let inline_is_lateinit = inline_type.as_ref().map_or(false, |at| matches!(at, AnnotationType::NonNil(_)));
                                        let inline_annotation_text = inline_type.as_ref()
                                            .map(|at| crate::annotations::format_annotation_type(at));
                                        let inline_annotation = inline_type
                                            .and_then(|at| self.resolve_annotation_type_mut_gen(&at, &[]));
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                          if names.len() > 2 {
                                            // Deep chain (e.g. self._plot.dot = expr):
                                            // defer to post-fixpoint resolution
                                            self.deferred.deep_field_injections.push(DeepFieldInjection {
                                                root_name: root_name.clone(),
                                                intermediates: names[1..names.len()-1].to_vec(),
                                                field_name: field_name.clone(),
                                                expr_id,
                                                scope_idx,
                                            });
                                          } else {
                                            let field_lateinit = self.ir.get_field(table_idx, field_name).map_or(false, |f| f.lateinit);
                                            if let Some(expected_vt) = self.ir.get_field(table_idx, field_name).and_then(|f| f.annotation.clone()) {
                                                let r = expr.syntax().text_range();
                                                self.deferred.field_type_checks.push(FieldTypeCheck {
                                                    expected: expected_vt, actual_expr: expr_id, field_name: field_name.clone(),
                                                    start: u32::from(r.start()), end: trimmed_node_end(expr.syntax()),
                                                    lateinit: field_lateinit,
                                                });
                                            } else if inline_annotation.is_none() {
                                                // D7: inject-field — setting undeclared field on @class
                                                let field_already_exists = self.ir.get_field(table_idx, field_name).is_some();
                                                if !field_already_exists {
                                                    let table = self.table(table_idx);
                                                    let has_annotations = table.fields.values().any(|f| f.annotation.is_some());
                                                    let is_static_field = func_id.is_none() && table_idx >= EXT_BASE;
                                                    if table.class_name.is_some() && has_annotations && constructor_of != Some(table_idx) && !is_static_field {
                                                        let parent_has = table.parent_classes.iter().any(|&pi| {
                                                            self.ir.get_field(pi, field_name).and_then(|f| f.annotation.as_ref()).is_some()
                                                        });
                                                        if !parent_has {
                                                            let class_name = table.class_name.clone().unwrap_or_default();
                                                            let ident_node = ident.syntax();
                                                            let r = ident_node.text_range();
                                                            crate::diagnostics::inject_field::check(
                                                                &mut self.diagnostics,
                                                                field_name, &class_name,
                                                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                            if table_idx < EXT_BASE {
                                                let existing_vis = self.ir.tables[table_idx].fields.get(field_name).map(|f| f.visibility).unwrap_or_else(|| crate::annotations::default_visibility_for_name(field_name));
                                                if let Some(field_info) = self.ir.tables[table_idx].fields.get_mut(field_name) {
                                                    field_info.extra_exprs.push(expr_id);
                                                    field_info.visibility = existing_vis;
                                                    if field_info.annotation.is_none() {
                                                        if let Some(ref ann) = inline_annotation {
                                                            field_info.annotation = Some(ann.clone());
                                                        }
                                                        if inline_annotation_text.is_some() {
                                                            field_info.annotation_text = inline_annotation_text.clone();
                                                        }
                                                    }
                                                    if inline_is_lateinit { field_info.lateinit = true; }
                                                } else {
                                                    self.ir.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                                                        expr: expr_id,
                                                        extra_exprs: Vec::new(),
                                                        visibility: existing_vis,
                                                        annotation: inline_annotation.clone(),
                                                        annotation_text: inline_annotation_text.clone(),
                                                        annotation_type_raw: None,
                                                        lateinit: inline_is_lateinit,
                                                    });
                                                }
                                            } else {
                                                // External table: store in per-file overlay
                                                if let Some(overlay_fi) = self.ir.get_overlay_field_mut(table_idx, field_name) {
                                                    overlay_fi.extra_exprs.push(expr_id);
                                                    if overlay_fi.annotation.is_none() {
                                                        if let Some(ref ann) = inline_annotation {
                                                            overlay_fi.annotation = Some(ann.clone());
                                                        }
                                                        if inline_annotation_text.is_some() {
                                                            overlay_fi.annotation_text = inline_annotation_text.clone();
                                                        }
                                                    }
                                                    if inline_is_lateinit { overlay_fi.lateinit = true; }
                                                } else {
                                                    self.ir.insert_overlay_field(table_idx, field_name.clone(), FieldInfo {
                                                        expr: expr_id,
                                                        extra_exprs: Vec::new(),
                                                        visibility: crate::annotations::default_visibility_for_name(field_name),
                                                        annotation: inline_annotation.clone(),
                                                        annotation_text: inline_annotation_text.clone(),
                                                        annotation_type_raw: None,
                                                        lateinit: inline_is_lateinit,
                                                    });
                                                }
                                            }
                                            let r = ident.syntax().text_range();
                                            self.deferred.field_assignment_sites.push(FieldAssignmentSite {
                                                table_idx, field_name: field_name.clone(), scope_idx,
                                                block_stmt_index: stmt_index as u32,
                                                start: u32::from(r.start()), end: u32::from(r.end()),
                                            });
                                          }
                                        } else if names.len() == 2 {
                                            // Table not found during Phase 1 (e.g. type comes from
                                            // function return) — defer to post-fixpoint resolution.
                                            let r = ident.syntax().text_range();
                                            self.deferred.deferred_field_assignments.push(DeferredFieldAssignment {
                                                root_name: root_name.clone(),
                                                field_name: field_name.clone(),
                                                expr_id,
                                                scope_idx,
                                                ident_start: u32::from(r.start()),
                                                ident_end: u32::from(r.end()),
                                            });
                                        }
                                    }
                                    // Narrow the field after assignment so subsequent
                                    // accesses don't warn about nil (skip literal nil).
                                    let is_nil_literal = matches!(expression, Some(Expression::Literal(lit)) if lit.is_nil());
                                    if !is_nil_literal {
                                        self.try_narrow_field(&names, scope_idx);
                                    }
                                } else {
                                    // Simple assignment: x = expr
                                    // Record create-global if this name doesn't exist in any scope
                                    if self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx).is_none() {
                                        let name_tokens: Vec<_> = ident.syntax().children_with_tokens()
                                            .filter_map(|t| t.into_token())
                                            .filter(|t| t.kind() == SyntaxKind::Name)
                                            .collect();
                                        if let Some(tok) = name_tokens.first() {
                                            let r = tok.text_range();
                                            self.deferred.created_globals.push(CreatedGlobal {
                                                name: root_name.clone(),
                                                start: u32::from(r.start()),
                                                end: u32::from(r.end()),
                                            });
                                        }
                                    }
                                    if let Some(Expression::Function(func)) = expression {
                                        let symbol_idx = self.ir.insert_or_version_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        // Mark narrowing as overridden if this symbol has active narrowing
                                        if self.get_type_narrowing(symbol_idx, scope_idx).is_some()
                                            || self.get_type_filtering(symbol_idx, scope_idx).is_some() {
                                            self.narrowing_overridden.entry(scope_idx).or_default().insert(symbol_idx);
                                        }
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.ir.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        if let Some(inner_block) = func.block() {
                                            stack.push(Frame {
                                                block: inner_block,
                                                next_stmt: 0,
                                                scope_idx: new_scope_idx,
                                                func_id: Some(func_idx),
                                                constructor_of: None,
                                            });
                                        }
                                    } else {
                                        let type_source = if let Some(expr) = expression {
                                            Some(self.lower_expression(expr, scope_idx))
                                        } else if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                            if index >= expressions.len() {
                                                let ret_index = index - (expressions.len() - 1);
                                                Some(self.lower_function_call(call, scope_idx, ret_index, false))
                                            } else {
                                                None
                                            }
                                        } else if matches!(expressions.last(), Some(Expression::VarArgs(_))) {
                                            if index >= expressions.len() {
                                                let ret_index = index - (expressions.len() - 1);
                                                if func_id.is_none() && ret_index == 1 {
                                                    // WoW passes (addonName, addonTable) at file scope
                                                    let table_idx = self.ir.tables.len();
                                                    let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                                                        self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
                                                    } else {
                                                        HashMap::new()
                                                    };
                                                    self.ir.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                                                    Some(self.ir.push_expr(Expr::TableConstructor(table_idx)))
                                                } else {
                                                    Some(self.ir.push_expr(Expr::VarArgs(ret_index, func_id.is_none())))
                                                }
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };
                                        let symbol_idx = self.ir.insert_or_version_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        // Mark narrowing as overridden if this symbol has active narrowing
                                        if self.get_type_narrowing(symbol_idx, scope_idx).is_some()
                                            || self.get_type_filtering(symbol_idx, scope_idx).is_some() {
                                            self.narrowing_overridden.entry(scope_idx).or_default().insert(symbol_idx);
                                        }
                                        if let Some(expr_id) = type_source {
                                            self.ir.set_type_source(symbol_idx, expr_id);
                                            // Track multi-return siblings from function calls
                                            if let Expr::FunctionCall { ret_index, .. } = self.ir.expr(expr_id) {
                                                multi_return_group.push((*ret_index, symbol_idx));
                                            }
                                            // D2: assign-type-mismatch — check reassignment against @type
                                            if let Some(expected) = self.symbol_type_annotations.get(&symbol_idx).cloned() {
                                                if let Some(expr) = expression {
                                                    let r = expr.syntax().text_range();
                                                    self.deferred.assign_type_checks.push(AssignTypeCheck {
                                                        expected, actual_expr: expr_id, var_name: root_name.clone(),
                                                        start: u32::from(r.start()), end: trimmed_node_end(expr.syntax()),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if ident.is_indexed_expression() {
                                // Bracket-indexed assignment with no direct name tokens
                                // (e.g. tbl[1] = expr): still lower the RHS so that
                                // symbol references are marked as used.
                                if let Some(expr) = expressions.get(index) {
                                    self.lower_expression(expr, scope_idx);
                                }
                            }
                        }

                        // Register multi-return sibling groups (2+ returns from same call)
                        if multi_return_group.len() >= 2 {
                            for &(_, sym_idx) in &multi_return_group {
                                self.multi_return_siblings.insert(sym_idx, multi_return_group.clone());
                            }
                        }
                    }
                },
                Statement::FunctionCall(call) => {
                    self.lower_function_call(&call, scope_idx, 0, true);
                    // Narrow first argument after assert() calls
                    if let Some(ident) = call.identifier() {
                        let names = ident.names();
                        if names.len() == 1 && names[0] == "assert" {
                            if let Some(args) = call.arguments() {
                                let exprs = args.expressions();
                                if let Some(first_arg) = exprs.first() {
                                    self.narrow_assert_expr(first_arg, scope_idx);
                                }
                            }
                        }
                    }
                },
            }

            // Drain any inline function bodies queued by lower_expression
            for (block, block_scope, block_func_id) in self.pending_blocks.drain(..).collect::<Vec<_>>() {
                stack.push(Frame {
                    block,
                    next_stmt: 0,
                    scope_idx: block_scope,
                    func_id: block_func_id,
                    constructor_of: None,
                });
            }

            // D5: unreachable-code — check for statements after return
            if matches!(&statements[stmt_index], Statement::Return(_)) && stmt_index + 1 < statements.len() {
                let next_stmt = &statements[stmt_index + 1];
                let r = next_stmt.syntax().text_range();
                crate::diagnostics::unreachable_code::check(
                    &mut self.diagnostics,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
    }

    pub(super) fn lower_expression(&mut self, expression: &Expression, scope_idx: ScopeIndex) -> ExprId {
        let expr_id = self.lower_expression_inner(expression, scope_idx);
        // Check for trailing --[[@as Type]] annotation
        if let Some(as_type) = Self::extract_inline_as(expression.syntax()) {
            if let Some(vt) = self.resolve_annotation_type_mut_gen(&as_type, &[]) {
                return self.ir.push_expr(Expr::Literal(vt));
            }
        }
        expr_id
    }

    fn lower_expression_inner(&mut self, expression: &Expression, scope_idx: ScopeIndex) -> ExprId {
        match expression {
            Expression::Literal(l) => {
                let string_raw = l.get_string();
                let vt = if string_raw.is_some() {
                    ValueType::String(None)
                } else if let Some(bool_value) = l.get_bool() {
                    ValueType::Boolean(Some(bool_value))
                } else if l.get_number().is_some() {
                    ValueType::Number
                } else if l.is_nil() {
                    ValueType::Nil
                } else {
                    return self.ir.push_expr(Expr::Unknown);
                };
                let expr_id = self.ir.push_expr(Expr::Literal(vt));
                if let Some(raw) = string_raw {
                    let stripped = raw.trim_matches(|c| c == '"' || c == '\'');
                    self.ir.string_literals.insert(expr_id, stripped.to_string());
                }
                if let Some(num) = l.get_number() {
                    self.ir.number_literals.insert(expr_id, num);
                }
                expr_id
            }
            Expression::Identifier(ident) => {
                // Check for child FunctionCall and Identifier nodes
                let child_call = ident.syntax().children().find_map(FunctionCall::cast);
                let child_ident = ident.syntax().children()
                    .find_map(Identifier::cast);
                let name_tokens: Vec<_> = ident.syntax().children_with_tokens()
                    .filter_map(|t| t.into_token())
                    .filter(|t| t.kind() == SyntaxKind::Name)
                    .collect();
                let child_grouped = ident.syntax().children().find_map(GroupedExpression::cast);
                if let Some(ref grouped) = child_grouped {
                    // Identifier with a grouped expression prefix: (expr).field
                    let grouped_expr = Expression::GroupedExpression(grouped.clone());
                    let mut current = self.lower_expression(&grouped_expr, scope_idx);
                    // Chain field accesses from direct Name tokens
                    for field_token in name_tokens.iter() {
                        let r = field_token.text_range();
                        let table_for_check = current;
                        current = self.ir.push_expr(Expr::FieldAccess {
                            table: current,
                            field: field_token.text().to_string(),
                            field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                        });
                        self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: table_for_check, start: u32::from(r.start()), end: u32::from(r.end()) });
                    }
                    // Chain field accesses from child Identifier names
                    if let Some(ref child) = child_ident {
                        let child_tokens: Vec<_> = child.syntax().children_with_tokens()
                            .filter_map(|t| t.into_token())
                            .filter(|t| t.kind() == SyntaxKind::Name)
                            .collect();
                        for field_token in child_tokens.iter() {
                            let r = field_token.text_range();
                            let table_for_check = current;
                            current = self.ir.push_expr(Expr::FieldAccess {
                                table: current,
                                field: field_token.text().to_string(),
                                field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                            });
                            self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: table_for_check, start: u32::from(r.start()), end: u32::from(r.end()) });
                        }
                    }
                    current
                } else if let Some(ref call) = child_call {
                    // Identifier with a child FunctionCall (e.g. select(2, ...).X, funcall():method)
                    let call_expr = Expression::FunctionCall(call.clone());
                    let mut current = if let Some(2) = crate::annotations::is_select_varargs(&call_expr) {
                        // select(2, ...).field → treat base as addon namespace table
                        let table_idx = self.ir.tables.len();
                        let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                            self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
                        } else {
                            HashMap::new()
                        };
                        self.ir.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                        self.ir.push_expr(Expr::TableConstructor(table_idx))
                    } else {
                        self.lower_function_call(call, scope_idx, 0, false)
                    };
                    // Chain field accesses from direct Name tokens
                    for field_token in name_tokens.iter() {
                        let r = field_token.text_range();
                        let table_for_check = current;
                        current = self.ir.push_expr(Expr::FieldAccess {
                            table: current,
                            field: field_token.text().to_string(),
                            field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                        });
                        self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: table_for_check, start: u32::from(r.start()), end: u32::from(r.end()) });
                    }
                    // Chain field accesses from child Identifier names (e.g. select(2, ...).LibTSMApp)
                    if let Some(ref child) = child_ident {
                        let child_tokens: Vec<_> = child.syntax().children_with_tokens()
                            .filter_map(|t| t.into_token())
                            .filter(|t| t.kind() == SyntaxKind::Name)
                            .collect();
                        for field_token in child_tokens.iter() {
                            let r = field_token.text_range();
                            let table_for_check = current;
                            current = self.ir.push_expr(Expr::FieldAccess {
                                table: current,
                                field: field_token.text().to_string(),
                                field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                            });
                            self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: table_for_check, start: u32::from(r.start()), end: u32::from(r.end()) });
                        }
                    }
                    current
                } else if let Some(child) = child_ident {
                    // Complex identifier (bracket index or similar): lower child as base,
                    // handle bracket indexing, then chain remaining Name tokens as field accesses
                    let mut current = self.lower_expression(&Expression::Identifier(child), scope_idx);
                    // Check for bracket indexing [expr] on this Identifier
                    let has_bracket = ident.syntax().children_with_tokens()
                        .any(|t| t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::LeftSquareBracket));
                    if has_bracket {
                        if let Some(key_expr) = ident.syntax().children()
                            .filter(|n| n.kind() == SyntaxKind::Expression)
                            .find_map(Expression::cast) {
                            let key_id = self.lower_expression(&key_expr, scope_idx);
                            current = self.ir.push_expr(Expr::BracketIndex { table: current, key: key_id });
                        }
                    }
                    for field_token in name_tokens.iter() {
                        let r = field_token.text_range();
                        let table_for_check = current;
                        current = self.ir.push_expr(Expr::FieldAccess {
                            table: current,
                            field: field_token.text().to_string(),
                            field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                        });
                        self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: table_for_check, start: u32::from(r.start()), end: u32::from(r.end()) });
                    }
                    // Chain field accesses from remaining child Identifiers after the first
                    // (which was consumed as the bracket-index base). This handles patterns
                    // like t[key].field1.field2 where .field1.field2 live in a sibling Identifier node.
                    for remaining_child in ident.syntax().children()
                        .filter_map(Identifier::cast)
                        .skip(1)
                    {
                        for field_token in remaining_child.syntax().children_with_tokens()
                            .filter_map(|t| t.into_token())
                            .filter(|t| t.kind() == SyntaxKind::Name)
                        {
                            let r = field_token.text_range();
                            let table_for_check = current;
                            current = self.ir.push_expr(Expr::FieldAccess {
                                table: current,
                                field: field_token.text().to_string(),
                                field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                            });
                            self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: table_for_check, start: u32::from(r.start()), end: u32::from(r.end()) });
                        }
                    }
                    current
                } else if let Some(first_token) = name_tokens.first() {
                    let name = first_token.text().to_string();
                    let base = if let Some(symbol_idx) = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx) {
                        // Check for scope-level type narrowing (from @type-narrows or type() guards).
                        // If present, lazily push a narrowed version so assignments capture the narrowed type.
                        // Skip narrowing if the symbol was reassigned after narrowing in this scope.
                        let version_idx = if !self.is_narrowing_overridden(symbol_idx, scope_idx) {
                            let narrowed = self.get_type_narrowing(symbol_idx, scope_idx).cloned();
                            let filtered = self.get_type_filtering(symbol_idx, scope_idx).cloned();
                            match (narrowed, filtered) {
                                (Some(narrowed), Some(guard)) => {
                                    // Both type-narrowed (e.g. from outer `or`) and type-filtered
                                    // (e.g. from inner `type()` guard) apply — combine them by
                                    // filtering the narrowed type to the guard.
                                    let cache_key = (scope_idx, symbol_idx);
                                    if let Some(&cached_ver) = self.type_narrows_version_cache.get(&cache_key) {
                                        cached_ver
                                    } else {
                                        let combined = narrowed.filter_type(&guard);
                                        self.push_type_narrowed_version(symbol_idx, combined, scope_idx);
                                        let ver = self.sym(symbol_idx).versions.len() - 1;
                                        self.type_narrows_version_cache.insert(cache_key, ver);
                                        ver
                                    }
                                }
                                (Some(narrowed), None) => {
                                    let cache_key = (scope_idx, symbol_idx);
                                    if let Some(&cached_ver) = self.type_narrows_version_cache.get(&cache_key) {
                                        cached_ver
                                    } else {
                                        self.push_type_narrowed_version(symbol_idx, narrowed, scope_idx);
                                        let ver = self.sym(symbol_idx).versions.len() - 1;
                                        self.type_narrows_version_cache.insert(cache_key, ver);
                                        ver
                                    }
                                }
                                (None, Some(guard)) => {
                                    let cache_key = (scope_idx, symbol_idx);
                                    if let Some(&cached_ver) = self.type_narrows_version_cache.get(&cache_key) {
                                        cached_ver
                                    } else {
                                        self.push_type_filter_version(symbol_idx, guard, scope_idx);
                                        let ver = self.sym(symbol_idx).versions.len() - 1;
                                        self.type_narrows_version_cache.insert(cache_key, ver);
                                        ver
                                    }
                                }
                                (None, None) => {
                                    self.ir.version_for_scope(symbol_idx, scope_idx)
                                }
                            }
                        } else {
                            self.ir.version_for_scope(symbol_idx, scope_idx)
                        };
                        self.referenced_symbols.insert(symbol_idx);
                        self.symbol_version_at.insert(u32::from(first_token.text_range().start()), version_idx);
                        let sym_ref = self.ir.push_expr(Expr::SymbolRef(symbol_idx, version_idx));
                        // Wrap in StripFalsy/StripNil if the symbol is narrowed in this scope.
                        // This applies narrowing to expressions (e.g. `local x = value` inside
                        // `if value then`) without pushing permanent symbol versions.
                        if self.is_symbol_falsy_narrowed(symbol_idx, scope_idx) {
                            self.ir.push_expr(Expr::StripFalsy(sym_ref))
                        } else if self.is_symbol_narrowed(symbol_idx, scope_idx) {
                            self.ir.push_expr(Expr::StripNil(sym_ref))
                        } else {
                            sym_ref
                        }
                    } else {
                        // Record unresolved single-name references for undefined-global check
                        if name_tokens.len() == 1 {
                            let r = first_token.text_range();
                            self.deferred.unresolved_globals.push(UnresolvedGlobal { name: name.clone(), scope_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                        }
                        self.ir.push_expr(Expr::Unknown)
                    };
                    // Check for bracket indexing [expr] on this Identifier (e.g. tbl[var])
                    let mut current = base;
                    let has_bracket = ident.syntax().children_with_tokens()
                        .any(|t| t.as_token().map_or(false, |tok| tok.kind() == SyntaxKind::LeftSquareBracket));
                    if has_bracket {
                        if let Some(key_expr) = ident.syntax().children()
                            .filter(|n| n.kind() == SyntaxKind::Expression)
                            .find_map(Expression::cast) {
                            let key_id = self.lower_expression(&key_expr, scope_idx);
                            current = self.ir.push_expr(Expr::BracketIndex { table: current, key: key_id });
                        }
                    }
                    // Chain field accesses for dotted names (t.x.y)
                    for field_token in name_tokens.iter().skip(1) {
                        let r = field_token.text_range();
                        let table_for_check = current;
                        current = self.ir.push_expr(Expr::FieldAccess {
                            table: current,
                            field: field_token.text().to_string(),
                            field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                        });
                        self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: table_for_check, start: u32::from(r.start()), end: u32::from(r.end()) });
                    }
                    current
                } else {
                    self.ir.push_expr(Expr::Unknown)
                }
            }
            Expression::BinaryExpression(b) => {
                let terms = b.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let lhs_id = self.lower_expression(lhs, scope_idx);
                    let op = b.kind();
                    // For short-circuit `and`, narrow nil/type guards from LHS before lowering RHS.
                    // Push a temporary StripNil version so RHS references see the narrowed type,
                    // then pop it after lowering RHS so later code sees the original type.
                    // The parser produces two shapes depending on the RHS:
                    //   `a == b and c`     → BinaryExpr(And, [BinaryExpr(==), c])
                    //   `a == b and c == d` → BinaryExpr(None, [BinaryExpr(==), BinaryExpr(And+==)])
                    // For short-circuit `and`, temporarily narrow nil/type guards from
                    // LHS so RHS references see the narrowed type. After lowering RHS,
                    // restore the original version so later code sees the un-narrowed type.
                    // For `and` chains, collect ALL guard symbols from the LHS
                    // so `a and b and c and func(a, b, c)` narrows a, b, AND c.
                    let is_and_chain = matches!(op, Operator::And) || (matches!(op, Operator::None) && matches!(rhs, Expression::BinaryExpression(rb) if matches!(rb.kind(), Operator::And)));
                    let extra_chain_guards: Vec<(SymbolIndex, GuardNarrow)> = if is_and_chain {
                        self.collect_and_chain_guards(lhs, scope_idx)
                    } else {
                        Vec::new()
                    };
                    let guard_result = if is_and_chain {
                        self.detect_and_lhs_guard(lhs, scope_idx)
                    } else if matches!(op, Operator::Or) {
                        self.detect_or_lhs_guard(lhs, scope_idx)
                    } else if matches!(op, Operator::None) {
                        if let Expression::BinaryExpression(rhs_bin) = rhs {
                            if matches!(rhs_bin.kind(), Operator::Or) {
                                self.detect_or_lhs_guard(lhs, scope_idx)
                            } else { None }
                        } else { None }
                    } else { None };
                    let guard_sym = guard_result.as_ref().map(|(si, _)| *si);
                    // Save the pre-narrowing version index so we can restore after RHS
                    let pre_narrow_ver = guard_result.map(|(si, narrow_kind)| {
                        let v = self.ir.version_for_scope(si, scope_idx);
                        match narrow_kind {
                            GuardNarrow::FilterTo(vt) => self.push_type_filter_version(si, vt, scope_idx),
                            GuardNarrow::StripNil => self.push_strip_nil_version(si, scope_idx),
                            GuardNarrow::StripFalsy => self.push_strip_falsy_version(si, scope_idx),
                        }
                        v
                    });
                    // Narrow extra chain guards (intermediate `and` operands beyond the first)
                    let extra_pre_narrow: Vec<(SymbolIndex, usize)> = extra_chain_guards.into_iter()
                        .filter(|(si, _)| guard_sym != Some(*si)) // skip the primary guard (already narrowed)
                        .filter_map(|(si, narrow_kind)| {
                            let v = self.ir.version_for_scope(si, scope_idx);
                            match narrow_kind {
                                GuardNarrow::FilterTo(vt) => self.push_type_filter_version(si, vt, scope_idx),
                                GuardNarrow::StripNil => self.push_strip_nil_version(si, scope_idx),
                                GuardNarrow::StripFalsy => self.push_strip_falsy_version(si, scope_idx),
                            }
                            Some((si, v))
                        })
                        .collect();
                    // Field-level narrowing for `self.field and ...` / `not self.field or ...` patterns
                    let field_guard = if matches!(op, Operator::And) {
                        self.detect_and_lhs_field_guard(lhs, scope_idx)
                    } else if matches!(op, Operator::Or) {
                        self.detect_or_lhs_field_guard(lhs, scope_idx)
                    } else if matches!(op, Operator::None) {
                        if let Expression::BinaryExpression(rhs_bin) = rhs {
                            if matches!(rhs_bin.kind(), Operator::And) {
                                self.detect_and_lhs_field_guard(lhs, scope_idx)
                            } else if matches!(rhs_bin.kind(), Operator::Or) {
                                self.detect_or_lhs_field_guard(lhs, scope_idx)
                            } else { None }
                        } else { None }
                    } else { None };
                    // Temporarily suppress scope-level type narrowing metadata for
                    // the guard symbol so the RHS name lookup uses version_for_scope
                    // (which picks up the just-pushed filtered/stripped version) instead
                    // of the cached type_narrowed version from an outer `or` condition.
                    let saved_narrowing = guard_sym.and_then(|si| {
                        let cache_key = (scope_idx, si);
                        let cached_ver = self.type_narrows_version_cache.remove(&cache_key);
                        let narrowed = self.type_narrowed_symbols.get_mut(&scope_idx)
                            .and_then(|m| m.remove(&si));
                        if cached_ver.is_some() || narrowed.is_some() {
                            Some((cached_ver, narrowed))
                        } else {
                            None
                        }
                    });
                    let nil_check_start = self.deferred.nil_check_sites.len();
                    let rhs_id = self.lower_expression(rhs, scope_idx);
                    // Restore the suppressed narrowing metadata
                    if let (Some(sym_idx), Some((cached_ver, narrowed))) = (guard_sym, saved_narrowing) {
                        let cache_key = (scope_idx, sym_idx);
                        if let Some(v) = cached_ver {
                            self.type_narrows_version_cache.insert(cache_key, v);
                        }
                        if let Some(n) = narrowed {
                            self.type_narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx, n);
                        }
                    }
                    // Remove NilCheckSites covered by the field guard
                    if let Some((guard_sym, ref guard_fields)) = field_guard {
                        let mut i = nil_check_start;
                        while i < self.deferred.nil_check_sites.len() {
                            let table_expr = self.deferred.nil_check_sites[i].table_expr;
                            let matches = self.ir.extract_field_chain(table_expr)
                                .map_or(false, |(sym, chain)| sym == guard_sym && chain == *guard_fields);
                            if matches {
                                self.deferred.nil_check_sites.swap_remove(i);
                            } else {
                                i += 1;
                            }
                        }
                    }
                    // Remove NilCheckSites where the base symbol matches the bare-name guard.
                    // This handles external symbols (>= EXT_BASE) where push_strip_*_version
                    // is a no-op, and chained `and` patterns like `x and x.a ~= "" and x.b`.
                    if let Some(guard_sym_idx) = guard_sym {
                        let mut i = nil_check_start;
                        while i < self.deferred.nil_check_sites.len() {
                            let table_expr = self.deferred.nil_check_sites[i].table_expr;
                            let matches = self.ir.extract_field_chain(table_expr)
                                .map_or(false, |(sym, _chain)| sym == guard_sym_idx);
                            if matches {
                                self.deferred.nil_check_sites.swap_remove(i);
                            } else {
                                i += 1;
                            }
                        }
                    }
                    // Ternary idiom: `(x and ...) or z` — suppress nil-checks on x in z.
                    // In `x and x.a or x.b`, the programmer assumes x is non-nil throughout.
                    if matches!(op, Operator::Or) {
                        if let Some(and_guard_sym) = Self::extract_and_lhs_symbol(lhs, |name| self.get_symbol(&SymbolIdentifier::Name(name), scope_idx)) {
                            let mut i = nil_check_start;
                            while i < self.deferred.nil_check_sites.len() {
                                let table_expr = self.deferred.nil_check_sites[i].table_expr;
                                let matches = self.ir.extract_field_chain(table_expr)
                                    .map_or(false, |(sym, _chain)| sym == and_guard_sym);
                                if matches {
                                    self.deferred.nil_check_sites.swap_remove(i);
                                } else {
                                    i += 1;
                                }
                            }
                        }
                    }
                    // Restore original versions so code after `and` sees the un-narrowed types
                    // Restore extra chain guards first (reverse order)
                    for (sym_idx, ver) in extra_pre_narrow.iter().rev() {
                        if *sym_idx < EXT_BASE {
                            let node = self.ir.symbols[*sym_idx].versions[*ver].def_node;
                            let ref_expr = self.ir.push_expr(Expr::SymbolRef(*sym_idx, *ver));
                            self.ir.symbols[*sym_idx].versions.push(SymbolVersion {
                                def_node: node,
                                type_source: Some(ref_expr),
                                resolved_type: None,
                                type_args: Vec::new(),
                                created_in_scope: scope_idx,
                            });
                        }
                    }
                    // Restore primary guard
                    if let (Some(sym_idx), Some(ver)) = (guard_sym, pre_narrow_ver) {
                        if sym_idx < EXT_BASE {
                            let node = self.ir.symbols[sym_idx].versions[ver].def_node;
                            let ref_expr = self.ir.push_expr(Expr::SymbolRef(sym_idx, ver));
                            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                                def_node: node,
                                type_source: Some(ref_expr),
                                resolved_type: None,
                                type_args: Vec::new(),
                                created_in_scope: scope_idx,
                            });
                        }
                    }
                    self.ir.push_expr(Expr::BinaryOp { op, lhs: lhs_id, rhs: rhs_id })
                } else {
                    self.ir.push_expr(Expr::Unknown)
                }
            }
            Expression::UnaryExpression(u) => {
                let terms = u.get_terms();
                if let Some(operand) = terms.first() {
                    let operand_id = self.lower_expression(operand, scope_idx);
                    let op = u.kind();
                    self.ir.push_expr(Expr::UnaryOp { op, operand: operand_id })
                } else {
                    self.ir.push_expr(Expr::Unknown)
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    let inner_id = self.lower_expression(&inner, scope_idx);
                    self.ir.push_expr(Expr::Grouped(inner_id))
                } else {
                    self.ir.push_expr(Expr::Unknown)
                }
            }
            Expression::FunctionCall(call) => {
                self.lower_function_call(call, scope_idx, 0, false)
            }
            Expression::Function(func) => {
                let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                let func_idx = self.ir.functions.len() - 1;
                self.apply_annotations(func_idx, scope_idx, func.syntax());
                let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                if let Some(inner_block) = func.block() {
                    self.pending_blocks.push((inner_block, new_scope_idx, Some(func_idx)));
                }
                expr_id
            }
            Expression::TableConstructor(tc) => {
                let mut fields: HashMap<String, FieldInfo> = HashMap::new();
                let mut array_fields = Vec::new();
                for field in tc.fields() {
                    match field.kind() {
                        Some(FieldKind::Named { name, value }) => {
                            if fields.contains_key(&name) {
                                let r = field.syntax().text_range();
                                crate::diagnostics::duplicate_index::check(
                                    &mut self.diagnostics, &name,
                                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                );
                            }
                            let expr_id = self.lower_expression(&value, scope_idx);
                            // Check for inline ---@type annotation after the field
                            let inline_type = Self::extract_inline_type(field.syntax());
                            let annotation_text = inline_type.as_ref()
                                .map(|at| crate::annotations::format_annotation_type(at));
                            let annotation = inline_type
                                .and_then(|at| self.resolve_annotation_type_mut_gen(&at, &[]));
                            let vis = crate::annotations::default_visibility_for_name(&name);
                            fields.insert(name, FieldInfo {
                                expr: expr_id,
                                extra_exprs: Vec::new(),
                                visibility: vis,
                                annotation,
                                annotation_text,
                                annotation_type_raw: None,
                                lateinit: false,
                            });
                        }
                        Some(FieldKind::Positional(value)) => {
                            let expr_id = self.lower_expression(&value, scope_idx);
                            array_fields.push(expr_id);
                        }
                        None => {
                            // Bracket-keyed field: [expr] = value
                            // Lower all Expression and Identifier children so
                            // variable references in both key and value are tracked.
                            // Simple keys like [key] produce a bare Identifier child,
                            // while complex keys like [tbl.x] are in an Expression.
                            for child in field.syntax().children() {
                                if child.kind() == SyntaxKind::Expression {
                                    if let Some(expr) = Expression::cast(child) {
                                        self.lower_expression(&expr, scope_idx);
                                    }
                                } else if child.kind() == SyntaxKind::Identifier {
                                    if let Some(ident_node) = Identifier::cast(child) {
                                        self.lower_expression(&Expression::Identifier(ident_node), scope_idx);
                                    }
                                }
                            }
                        }
                    }
                }
                let table_idx = self.ir.tables.len();
                self.ir.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields, key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false });
                let r = tc.syntax().text_range();
                self.ir.table_ranges.insert((u32::from(r.start()), u32::from(r.end())), table_idx);
                self.ir.push_expr(Expr::TableConstructor(table_idx))
            }
            Expression::VarArgs(_) => {
                // VarArgs at ret_index 0; multi-value handled at assignment level
                self.ir.push_expr(Expr::VarArgs(0, self.current_func_id.is_none()))
            }
        }
    }

    fn analyze_nil_guard(&mut self, cond: &Expression, parent_scope: ScopeIndex, target_scope: ScopeIndex, is_then_branch: bool) {
        match cond {
            // `if x then` or `if self.field then` — bare truthiness guard
            Expression::Identifier(ident) => {
                if is_then_branch {
                    let names = ident.names();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                            self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                            self.falsy_narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                            self.narrow_siblings(sym_idx, target_scope);
                        }
                    } else {
                        self.try_narrow_field(&names, target_scope);
                    }
                }
            }
            // `if x ~= nil then` or `if x == nil then`
            // `if type(x) == "string" then` (any non-nil type literal)
            // `if a and b then` — recurse into both sides
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                // `a and b` — both conditions hold in the then-branch.
                // Also handle Operator::None which the parser produces for the outer
                // grouping node of chained binary expressions like `a == b and c == d`.
                if matches!(op, Operator::And | Operator::None) && is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.analyze_nil_guard(term, parent_scope, target_scope, true);
                        }
                        return;
                    }
                }
                // `a or b` in then-branch: at least one is true.
                // If all terms narrow the same symbol, the result is the union of
                // what each term narrows to. E.g. `x == nil or type(x) == "number"`
                // narrows x to `nil | number`.
                if matches!(op, Operator::Or) && is_then_branch {
                    let terms = Self::flatten_or_terms(&Expression::BinaryExpression(bin.clone()));
                    if terms.len() >= 2 {
                        self.try_or_then_narrowing(&terms, parent_scope, target_scope);
                        return;
                    }
                }
                // `a or b` in else-branch: NOT (a OR b) = NOT a AND NOT b
                // Both conditions are false, so apply inverse narrowing to both.
                if matches!(op, Operator::Or) && !is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.analyze_nil_guard(term, parent_scope, target_scope, false);
                        }
                        return;
                    }
                }
                let is_neq = matches!(op, Operator::NotEquals);
                let is_eq = matches!(op, Operator::Equals);
                if !is_neq && !is_eq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Check for nil comparison: `x ~= nil` / `x == nil`
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        let should_narrow = (is_neq && is_then_branch) || (is_eq && !is_then_branch);
                        if should_narrow {
                            if names.len() == 1 {
                                if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                                    self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                    self.narrow_siblings(sym_idx, target_scope);
                                }
                            } else {
                                self.try_narrow_field(&names, target_scope);
                            }
                        }
                    }
                    // Check for type() guard: `type(x) == "string"` etc.
                    // Also handles cached pattern: `local t = type(x); if t == "string"`
                    let is_positive_type_guard = (is_eq && is_then_branch) || (is_neq && !is_then_branch);
                    let is_inverse_type_guard = (is_eq && !is_then_branch) || (is_neq && is_then_branch);
                    if is_positive_type_guard || is_inverse_type_guard {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, parent_scope)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, parent_scope));
                        if let Some(sym_idx) = guard_sym {
                            if let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    if is_positive_type_guard {
                                        self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                        self.narrow_siblings(sym_idx, target_scope);
                                        self.type_filtered_symbols.entry(target_scope).or_default()
                                            .insert(sym_idx, vt);
                                    } else {
                                        self.add_type_stripped(target_scope, sym_idx, vt.clone());
                                        self.push_strip_type_version(sym_idx, vt, target_scope);
                                    }
                                }
                            } else if is_positive_type_guard {
                                // No type name literal but still a type guard (shouldn't happen, but keep existing behavior)
                                self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                self.narrow_siblings(sym_idx, target_scope);
                            }
                        }
                    }
                }
            }
            // Unwrap grouping: `if (x) then`
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.analyze_nil_guard(&inner, parent_scope, target_scope, is_then_branch);
                }
            }
            // Custom type guard: `if IsType(x, "Foo") then`
            Expression::FunctionCall(call) => {
                if is_then_branch {
                    if let Some((sym_idx, class_name)) = self.extract_type_narrows_guard(call, parent_scope) {
                        self.apply_type_narrows(sym_idx, &class_name, target_scope);
                    }
                }
            }
            // `not expr` flips the branch sense
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                if let Some(inner) = u.get_terms().into_iter().next() {
                    self.analyze_nil_guard(&inner, parent_scope, target_scope, !is_then_branch);
                }
            }
            _ => {}
        }
    }

    /// For `a or b` in then-branch, try to narrow if all terms constrain the same
    /// symbol. The narrowed type is the union of each term's effect.
    fn try_or_then_narrowing(&mut self, terms: &[Expression], parent_scope: ScopeIndex, target_scope: ScopeIndex) {
        // Collect what each term narrows
        let mut effects: Vec<(SymbolIndex, OrTermEffect)> = Vec::new();
        for term in terms {
            if let Some(effect) = self.extract_or_term_effect(term, parent_scope) {
                effects.push(effect);
            } else {
                return; // A term doesn't narrow any symbol — can't narrow overall
            }
        }
        // Check all terms narrow the same symbol
        let target_sym = effects[0].0;
        if !effects.iter().all(|(s, _)| *s == target_sym) {
            return;
        }
        // Build union of narrowed types
        let mut union_types: Vec<ValueType> = Vec::new();
        for (_, effect) in &effects {
            match effect {
                OrTermEffect::IsNil => {
                    if !union_types.contains(&ValueType::Nil) {
                        union_types.push(ValueType::Nil);
                    }
                }
                OrTermEffect::TypeIs(vt) => {
                    if !union_types.contains(vt) {
                        union_types.push(vt.clone());
                    }
                }
            }
        }
        if union_types.is_empty() { return; }
        let combined = if union_types.len() == 1 {
            union_types.into_iter().next().unwrap()
        } else {
            ValueType::Union(union_types)
        };
        let has_nil = matches!(&combined, ValueType::Nil)
            || matches!(&combined, ValueType::Union(ts) if ts.contains(&ValueType::Nil));
        self.type_narrowed_symbols.entry(target_scope).or_default()
            .insert(target_sym, combined);
        if !has_nil {
            self.narrowed_symbols.entry(target_scope).or_default().insert(target_sym);
        }
        self.narrow_siblings(target_sym, target_scope);
    }

    /// Extract the narrowing effect of a single comparison term in an `or` chain
    /// (then-branch context). Returns the symbol and what it's narrowed to.
    fn extract_or_term_effect(&self, term: &Expression, parent_scope: ScopeIndex) -> Option<(SymbolIndex, OrTermEffect)> {
        match term {
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return None; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // `x == nil` → IsNil
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                                if is_eq {
                                    return Some((sym_idx, OrTermEffect::IsNil));
                                }
                                // x ~= nil in an or-then context doesn't produce a useful positive constraint
                                return None;
                            }
                        }
                    }
                    // `type(x) == "number"` → TypeIs(Number)
                    if is_eq {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, parent_scope)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, parent_scope));
                        if let Some(sym_idx) = guard_sym {
                            if let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    return Some((sym_idx, OrTermEffect::TypeIs(vt)));
                                }
                            }
                        }
                    }
                }
                None
            }
            Expression::GroupedExpression(g) => {
                g.get_expression().and_then(|inner| self.extract_or_term_effect(&inner, parent_scope))
            }
            _ => None,
        }
    }

    /// Flatten nested `or` binary expressions into a flat list of leaf terms.
    /// `(a or b) or c` → `[a, b, c]`
    fn flatten_or_terms(expr: &Expression) -> Vec<Expression> {
        match expr {
            Expression::BinaryExpression(bin) if matches!(bin.kind(), Operator::Or) => {
                bin.get_terms().iter().flat_map(|t| Self::flatten_or_terms(&t)).collect()
            }
            other => {
                vec![Expression::cast(other.syntax().clone()).unwrap()]
            }
        }
    }

    /// Early-exit narrowing: if the then-branch always exits and the condition
    /// implies the variable is nil/falsy, narrow it as non-nil in the parent scope.
    /// Patterns: `if not x then error() end`, `if x == nil then return end`
    fn analyze_early_exit_guard(&mut self, cond: &Expression, scope_idx: ScopeIndex) {
        match cond {
            // `if not x then error()/return end` → x is truthy after (strip nil + false)
            // `if not IsType(x, "Foo") then return end` → x IS Foo after
            Expression::UnaryExpression(unary) => {
                if !matches!(unary.kind(), Operator::Not) { return; }
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                            self.narrow_symbol_strip_falsy(sym_idx, scope_idx);
                        }
                    } else {
                        self.try_narrow_field(&names, scope_idx);
                    }
                } else if let Some(Expression::FunctionCall(call)) = terms.first() {
                    if let Some((sym_idx, class_name)) = self.extract_type_narrows_guard(call, scope_idx) {
                        self.apply_type_narrows(sym_idx, &class_name, scope_idx);
                    }
                }
            }
            // `if x == nil then error()/return end` → x is non-nil after
            // `if type(x) == "boolean" then return end` → x has boolean stripped after
            // `if a or b then return end` → both a and b are false after
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                // `a or b` in early-exit: NOT (a OR b) = NOT a AND NOT b
                if matches!(op, Operator::Or) {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.analyze_early_exit_guard(term, scope_idx);
                        }
                        return;
                    }
                }
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Nil comparison: `x == nil then return end` → strip nil
                    if is_eq {
                        let ident_expr = if Self::is_nil_literal(rhs) {
                            Some(lhs)
                        } else if Self::is_nil_literal(lhs) {
                            Some(rhs)
                        } else {
                            None
                        };
                        if let Some(Expression::Identifier(ident)) = ident_expr {
                            let names = ident.names();
                            if names.len() == 1 {
                                if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                                    self.narrow_symbol_strip_nil(sym_idx, scope_idx);
                                }
                            } else {
                                self.try_narrow_field(&names, scope_idx);
                            }
                        }
                    }
                    // Type guard early exit: `if type(x) == "boolean" then return end`
                    // → strip boolean from x in parent scope (inverse of then-branch)
                    let strip_type_guard = is_eq;
                    let narrow_type_guard = is_neq;
                    if strip_type_guard || narrow_type_guard {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, scope_idx)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, scope_idx));
                        if let Some(sym_idx) = guard_sym {
                            if let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    if strip_type_guard {
                                        self.add_type_stripped(scope_idx, sym_idx, vt.clone());
                                        self.push_strip_type_version(sym_idx, vt.clone(), scope_idx);
                                    } else {
                                        self.type_filtered_symbols.entry(scope_idx).or_default()
                                            .insert(sym_idx, vt);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.analyze_early_exit_guard(&inner, scope_idx);
                }
            }
            _ => {}
        }
    }

    /// Ensure-initialized narrowing: detects `if not FIELD then FIELD = val end`
    /// and narrows FIELD as non-nil in the parent scope.
    /// Also handles `if FIELD == nil then FIELD = val end`.
    fn analyze_ensure_initialized(&mut self, cond: &Expression, block: &Block, scope_idx: ScopeIndex) {
        let guarded_names = self.extract_nil_guard_field(cond);
        if guarded_names.len() < 2 { return; }
        // Check if the then-block assigns to the same field
        if Self::block_assigns_field(block, &guarded_names) {
            self.try_narrow_field(&guarded_names, scope_idx);
        }
    }

    /// Extract the field chain from a negated nil-guard condition.
    /// Returns the names for `not self.field` or `self.field == nil`, empty vec otherwise.
    fn extract_nil_guard_field(&self, cond: &Expression) -> Vec<String> {
        match cond {
            // `not self.field`
            Expression::UnaryExpression(unary) => {
                if !matches!(unary.kind(), Operator::Not) { return vec![]; }
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() >= 2 && !ident.is_indexed_expression() {
                        return names;
                    }
                }
                vec![]
            }
            // `self.field == nil`
            Expression::BinaryExpression(bin) => {
                if !matches!(bin.kind(), Operator::Equals) { return vec![]; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() >= 2 && !ident.is_indexed_expression() {
                            return names;
                        }
                    }
                }
                vec![]
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    return self.extract_nil_guard_field(&inner);
                }
                vec![]
            }
            _ => vec![],
        }
    }

    /// Check if a block contains an assignment to the given dotted field name.
    /// Only checks top-level statements (not nested blocks).
    fn block_assigns_field(block: &Block, target_names: &[String]) -> bool {
        for stmt in block.statements() {
            if let Statement::Assign(assign) = &stmt {
                if let Some(var_list) = assign.variable_list() {
                    for ident in var_list.identifiers() {
                        if ident.names() == target_names && !ident.is_indexed_expression() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Mark a symbol as narrowed (non-nil) in the given scope, and create a new
    /// symbol version with nil stripped so type-mismatch checks see the narrowed type.
    fn narrow_symbol_strip_nil(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.push_strip_nil_version(sym_idx, scope_idx);
        self.narrow_siblings(sym_idx, scope_idx);
    }

    /// Like narrow_symbol_strip_nil but also strips false (truthiness narrowing).
    fn narrow_symbol_strip_falsy(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.falsy_narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.push_strip_falsy_version(sym_idx, scope_idx);
        self.narrow_siblings(sym_idx, scope_idx);
    }

    /// Narrow the expression passed to `assert()`. Decomposes `and` chains so that
    /// `assert(a and b and c)` narrows all three identifiers.
    fn narrow_assert_expr(&mut self, expr: &Expression, scope_idx: ScopeIndex) {
        match expr {
            Expression::Identifier(ident) => {
                let names = ident.names();
                if names.len() == 1 {
                    if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                        self.narrow_symbol_strip_falsy(sym_idx, scope_idx);
                    }
                } else {
                    self.try_narrow_field(&names, scope_idx);
                }
            }
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                if matches!(op, Operator::And | Operator::None) {
                    for term in &bin.get_terms() {
                        self.narrow_assert_expr(term, scope_idx);
                    }
                }
            }
            Expression::GroupedExpression(group) => {
                if let Some(inner) = group.get_expression() {
                    self.narrow_assert_expr(&inner, scope_idx);
                }
            }
            _ => {}
        }
    }

    /// Narrow multi-return siblings when a symbol from a return-only overload group is narrowed.
    /// Only applies if the called function has return-only overloads.
    fn narrow_siblings(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        let Some(siblings) = self.multi_return_siblings.get(&sym_idx).cloned() else { return };
        // Check that the function has return-only overloads by tracing from any sibling's
        // type_source (a FunctionCall expr) → func expr → symbol → FunctionDef → overloads
        if !self.has_return_only_overloads_from_siblings(&siblings) { return; }
        for &(_, sibling_idx) in &siblings {
            if sibling_idx == sym_idx { continue; }
            self.narrowed_symbols.entry(scope_idx).or_default().insert(sibling_idx);
            self.push_strip_nil_version(sibling_idx, scope_idx);
        }
    }

    /// Check if the function called in a multi-return group has return-only overloads.
    fn has_return_only_overloads_from_siblings(&self, siblings: &[(usize, SymbolIndex)]) -> bool {
        // Get any sibling's type_source to find the FunctionCall expression
        let (_, first_sym) = siblings[0];
        let type_source = self.ir.symbols[first_sym].versions.last()
            .and_then(|v| v.type_source);
        let Some(expr_id) = type_source else { return false };
        let func_expr = match self.ir.expr(expr_id) {
            Expr::FunctionCall { func, .. } => *func,
            _ => return false,
        };
        // Resolve func expr → symbol → FunctionDef → overloads
        let func_idx = match self.ir.expr(func_expr) {
            Expr::SymbolRef(sym_idx, _) => {
                let sym_idx = *sym_idx;
                // Look through the symbol's type_source to find FunctionDef
                self.ir.sym(sym_idx).versions.iter().find_map(|v| {
                    v.type_source.and_then(|ts| match self.ir.expr(ts) {
                        Expr::FunctionDef(idx) => Some(*idx),
                        _ => None,
                    })
                })
            }
            Expr::FieldAccess { .. } => {
                // Method calls — can't easily resolve at build time, skip for now
                None
            }
            _ => None,
        };
        let Some(func_idx) = func_idx else { return false };
        self.ir.func(func_idx).overloads.iter().any(|o| o.is_return_only)
    }

    /// Try to narrow a field access from an identifier with 2+ names (e.g. `self.field`).
    /// Marks the (root_symbol, field_name) pair as narrowed in the given scope.
    fn try_narrow_field(&mut self, names: &[String], scope_idx: ScopeIndex) {
        if names.len() == 2 {
            if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                self.narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, names[1].clone()));
            }
        }
    }

    /// Create a new symbol version with nil stripped (without updating narrowed_symbols).
    /// Used for short-circuit `and` narrowing where the version should be temporary.
    fn push_strip_nil_version(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        if sym_idx < EXT_BASE {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let stripped = self.ir.push_expr(Expr::StripNil(prev_ref));
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(stripped),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
            });
        }
    }

    /// Create a new symbol version with nil and false stripped (truthiness narrowing).
    fn push_strip_falsy_version(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        if sym_idx < EXT_BASE {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let stripped = self.ir.push_expr(Expr::StripFalsy(prev_ref));
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(stripped),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
            });
        }
    }

    /// Create a new symbol version with a specific type stripped from the union.
    /// Used for inverse type() guard narrowing (else-branch of `if type(x) == "t"`).
    fn push_strip_type_version(&mut self, sym_idx: SymbolIndex, strip_type: ValueType, scope_idx: ScopeIndex) {
        if sym_idx < EXT_BASE {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let stripped = self.ir.push_expr(Expr::CastRemove(prev_ref, strip_type));
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(stripped),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
            });
        }
    }

    /// Create a new symbol version narrowed to a specific type.
    /// Used for type() guard narrowing in short-circuit `and` expressions.
    fn push_type_narrowed_version(&mut self, sym_idx: SymbolIndex, narrowed_type: ValueType, scope_idx: ScopeIndex) {
        if sym_idx < EXT_BASE {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: None,
                resolved_type: Some(narrowed_type),
                type_args: Vec::new(),
                created_in_scope: scope_idx,
            });
        }
    }

    /// Push a version that filters the previous type to keep only types matching a
    /// type guard. Unlike `push_type_narrowed_version` (which sets a fixed type),
    /// this preserves specific types like `string[]` when narrowing with `type() == "table"`.
    fn push_type_filter_version(&mut self, sym_idx: SymbolIndex, guard_type: ValueType, scope_idx: ScopeIndex) {
        if sym_idx < EXT_BASE {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let filtered = self.ir.push_expr(Expr::TypeFilter(prev_ref, guard_type));
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(filtered),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
            });
        }
    }

    /// Add a type to strip for a symbol in a scope, combining with any existing strip.
    fn add_type_stripped(&mut self, scope: ScopeIndex, sym_idx: SymbolIndex, vt: ValueType) {
        let map = self.type_stripped_symbols.entry(scope).or_default();
        if let Some(existing) = map.remove(&sym_idx) {
            map.insert(sym_idx, ValueType::union(existing, vt));
        } else {
            map.insert(sym_idx, vt);
        }
    }

    fn is_nil_literal(expr: &Expression) -> bool {
        matches!(expr, Expression::Literal(lit) if lit.is_nil())
    }

    /// Convert a Lua type name string to a ValueType.
    fn type_name_to_value_type(type_name: &str) -> Option<ValueType> {
        match type_name {
            "string" => Some(ValueType::String(None)),
            "number" => Some(ValueType::Number),
            "boolean" => Some(ValueType::Boolean(None)),
            "table" => Some(ValueType::Table(None)),
            "function" => Some(ValueType::Function(None)),
            _ => None,
        }
    }

    /// Extract the type name string literal from an expression pair (either order).
    fn extract_type_name_literal(lhs: &Expression, rhs: &Expression) -> Option<&'static str> {
        let lit_expr = match (lhs, rhs) {
            (_, Expression::Literal(_)) => rhs,
            (Expression::Literal(_), _) => lhs,
            _ => return None,
        };
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "string" => Some("string"),
            "number" => Some("number"),
            "boolean" => Some("boolean"),
            "table" => Some("table"),
            "function" => Some("function"),
            "userdata" => Some("userdata"),
            "thread" => Some("thread"),
            _ => None,
        }
    }

    /// Detect `type(x) == "string"` (or "number", "boolean", "table", "function",
    /// "userdata", "thread") and return the symbol index of `x`.
    fn extract_type_guard_symbol(&self, lhs: &Expression, rhs: &Expression, scope: ScopeIndex) -> Option<SymbolIndex> {
        // Either order: type(x) == "string" or "string" == type(x)
        let (call_expr, lit_expr) = match (lhs, rhs) {
            (Expression::FunctionCall(_), Expression::Literal(_)) => (lhs, rhs),
            (Expression::Literal(_), Expression::FunctionCall(_)) => (rhs, lhs),
            _ => return None,
        };
        // Check that the literal is a non-nil type name string
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "string" | "number" | "boolean" | "table" | "function" | "userdata" | "thread" => {}
            _ => return None,
        }
        // Check that the call is `type(x)` with a single identifier argument
        let call = match call_expr { Expression::FunctionCall(c) => c, _ => unreachable!() };
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() != 1 || names[0] != "type" { return None; }
        let args = call.arguments()?;
        let exprs = args.expressions();
        if exprs.len() != 1 { return None; }
        if let Expression::Identifier(arg_ident) = &exprs[0] {
            let arg_names = arg_ident.names();
            if arg_names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(arg_names[0].clone()), scope);
            }
        }
        None
    }

    /// Extract the target symbol from a `type(x)` call expression.
    /// Returns Some(sym_idx) if the call is `type(single_identifier)`.
    fn extract_type_call_target(&self, call: &FunctionCall, scope: ScopeIndex) -> Option<SymbolIndex> {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() != 1 || names[0] != "type" { return None; }
        let args = call.arguments()?;
        let exprs = args.expressions();
        if exprs.len() != 1 { return None; }
        if let Expression::Identifier(arg_ident) = &exprs[0] {
            let arg_names = arg_ident.names();
            if arg_names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(arg_names[0].clone()), scope);
            }
        }
        None
    }

    /// Try to resolve a FunctionCall's callee to a FunctionIndex by walking
    /// external/local symbol → table → field chains.
    fn try_resolve_call_function(&self, call: &FunctionCall, scope: ScopeIndex) -> Option<FunctionIndex> {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.is_empty() { return None; }

        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        let sym = self.sym(sym_idx);
        let version = sym.versions.last()?;

        if names.len() == 1 {
            // Direct function call: `isType(x)`
            let expr_id = version.type_source?;
            if let Expr::FunctionDef(func_idx) = self.expr(expr_id) {
                return Some(*func_idx);
            }
            return None;
        }

        // Dotted call: `Table.Method(x)` — walk through table fields
        let expr_id = version.type_source?;
        let mut current_table = match self.expr(expr_id) {
            Expr::TableConstructor(ti) => *ti,
            Expr::Literal(ValueType::Table(Some(ti))) => *ti,
            _ => return None,
        };

        for (i, name) in names[1..].iter().enumerate() {
            let field = self.ir.get_field(current_table, name)?;
            let field_expr = self.expr(field.expr);
            if i == names.len() - 2 {
                // Last name — should be a function
                if let Expr::FunctionDef(func_idx) = field_expr {
                    return Some(*func_idx);
                }
                return None;
            } else {
                // Intermediate — should be a table
                match field_expr {
                    Expr::TableConstructor(ti) => current_table = *ti,
                    Expr::Literal(ValueType::Table(Some(ti))) => current_table = *ti,
                    _ => return None,
                }
            }
        }
        None
    }

    /// Extract type guard info from a function call with `@type-narrows`.
    /// Returns `(symbol_to_narrow, class_name)` if the callee is a type guard function.
    fn extract_type_narrows_guard(&self, call: &FunctionCall, scope: ScopeIndex) -> Option<(SymbolIndex, String)> {
        let func_idx = self.try_resolve_call_function(call, scope)?;
        let (target_idx, classname_idx) = self.func(func_idx).type_narrows?;

        let args = call.arguments()?.expressions();
        let ident = call.identifier()?;

        // Extract class name from string literal at classname_idx (1-based)
        if classname_idx == 0 { return None; } // classname can't be self
        let class_lit = args.get(classname_idx - 1)?;
        let class_name = if let Expression::Literal(lit) = class_lit {
            let s = lit.get_string()?;
            s.trim_matches(|c| c == '"' || c == '\'').to_string()
        } else {
            return None;
        };

        // Extract target symbol
        let sym_idx = if target_idx == 0 {
            // Target is the receiver (self) — for colon calls, first name in identifier
            let names = ident.names();
            if names.is_empty() { return None; }
            self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?
        } else {
            // Target is a call-site argument (1-based)
            let target_arg = args.get(target_idx - 1)?;
            if let Expression::Identifier(target_ident) = target_arg {
                let target_names = target_ident.names();
                if target_names.len() == 1 {
                    self.get_symbol(&SymbolIdentifier::Name(target_names[0].clone()), scope)?
                } else {
                    return None;
                }
            } else {
                return None;
            }
        };

        Some((sym_idx, class_name))
    }

    /// Apply type-narrows narrowing: record scope-level narrowing (version is pushed lazily).
    /// Returns true if narrowing was applied.
    fn apply_type_narrows(&mut self, sym_idx: SymbolIndex, class_name: &str, scope: ScopeIndex) -> bool {
        let table_idx = if let Some(&ti) = self.ir.classes.get(class_name) {
            ti
        } else if let Some(&ti) = self.ir.ext.classes.get(class_name) {
            ti
        } else {
            return false;
        };
        let narrowed = ValueType::Table(Some(table_idx));
        // Don't push a version eagerly — due to LIFO block processing, sibling
        // branches can add versions that bury this one.  Instead, the version is
        // pushed lazily when the symbol is actually referenced within the scope
        // (see `get_version_for_name` in the Identifier handler).
        self.type_narrowed_symbols.entry(scope).or_default()
            .insert(sym_idx, narrowed);
        true
    }

    /// Detect `cachedType == "string"` where `cachedType` was assigned from `type(x)`.
    /// Returns the symbol index of `x` (the original target).
    fn extract_cached_type_guard_symbol(&self, lhs: &Expression, rhs: &Expression, scope: ScopeIndex) -> Option<SymbolIndex> {
        let (ident_expr, lit_expr) = match (lhs, rhs) {
            (Expression::Identifier(_), Expression::Literal(_)) => (lhs, rhs),
            (Expression::Literal(_), Expression::Identifier(_)) => (rhs, lhs),
            _ => return None,
        };
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "string" | "number" | "boolean" | "table" | "function" | "userdata" | "thread" => {}
            _ => return None,
        }
        let ident = match ident_expr { Expression::Identifier(i) => i, _ => unreachable!() };
        let names = ident.names();
        if names.len() != 1 { return None; }
        let alias_sym = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        self.type_of_aliases.get(&alias_sym).copied()
    }

    /// Extract the bare-name symbol from an `and` LHS (for ternary idiom suppression).
    /// Given `BinaryExpr(And, [x, ...])`, returns the symbol for `x` if it's a single name.
    fn extract_and_lhs_symbol(expr: &Expression, resolve: impl Fn(String) -> Option<SymbolIndex>) -> Option<SymbolIndex> {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        return resolve(names[0].clone());
                    }
                }
            }
            // Parser flat form: BinaryExpr(None, [x, BinaryExpr(And, ...)])
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [Expression::Identifier(ident), Expression::BinaryExpression(rhs_bin)] = terms.as_slice() {
                    if matches!(rhs_bin.kind(), Operator::And) {
                        let names = ident.names();
                        if names.len() == 1 {
                            return resolve(names[0].clone());
                        }
                    }
                }
            }
        }
        None
    }

    /// Detect field access guards in `and` LHS for 2-name identifiers (e.g. `self.field and ...`
    /// or `self.field ~= nil and ...`). Returns (root_symbol, field_name).
    fn detect_and_lhs_field_guard(&self, lhs: &Expression, scope_idx: ScopeIndex) -> Option<(SymbolIndex, Vec<String>)> {
        // Bare field truthiness: `self.field and ...` or `self._state.x and ...`
        if let Expression::Identifier(ident) = lhs {
            let names = ident.names();
            if names.len() >= 2 {
                let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                return Some((sym_idx, names[1..].to_vec()));
            }
        }
        // Field nil comparison: `self.field ~= nil and ...` or `self._state.x ~= nil and ...`
        if let Expression::BinaryExpression(bin) = lhs {
            if matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() >= 2 {
                            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                            return Some((sym_idx, names[1..].to_vec()));
                        }
                    }
                }
            }
        }
        None
    }

    /// When lowering `a and b` where `a` is a nil/type guard (e.g. `x ~= nil`,
    /// `type(x) == "string"`), detect which symbol should be narrowed.
    /// Returns (symbol_index, guard_narrow_kind) if a guard pattern is found.
    fn detect_and_lhs_guard(&self, lhs: &Expression, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        // Bare name: `x and ...` → truthiness guard (strip nil + false)
        if let Expression::Identifier(ident) = lhs {
            let names = ident.names();
            if names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                    .map(|s| (s, GuardNarrow::StripFalsy));
            }
        }
        if let Expression::BinaryExpression(bin) = lhs {
            // Chained and: `(x and ...) and y` → x must be truthy in y.
            // The parser may produce BinaryExpr(And, [x, ...]) or the flat form
            // BinaryExpr(None, [x, BinaryExpr(And, ...)]).
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let Some(first) = terms.first() {
                    return self.detect_and_lhs_guard(first, scope_idx);
                }
            }
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [first, Expression::BinaryExpression(rhs_bin)] = terms.as_slice() {
                    if matches!(rhs_bin.kind(), Operator::And) {
                        return self.detect_and_lhs_guard(first, scope_idx);
                    }
                }
            }
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    if let Some(sym_idx) = self.extract_type_guard_symbol(l, r, scope_idx)
                        .or_else(|| self.extract_cached_type_guard_symbol(l, r, scope_idx))
                    {
                        let narrowed_type = Self::extract_type_name_literal(l, r)
                            .and_then(Self::type_name_to_value_type);
                        return Some((sym_idx, match narrowed_type {
                            Some(vt) => GuardNarrow::FilterTo(vt),
                            None => GuardNarrow::StripNil,
                        }));
                    }
                }
            }
            if matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
        }
        None
    }

    /// Collect ALL guard symbols from a left-associative `and` chain.
    /// For `And(And(And(a, b), c), rhs)`, given the LHS `And(And(a, b), c)`,
    /// returns guards for `[a, b, c]` — all intermediate operands that must be
    /// truthy for the RHS to execute.
    fn collect_and_chain_guards(&self, lhs: &Expression, scope_idx: ScopeIndex) -> Vec<(SymbolIndex, GuardNarrow)> {
        let mut guards = Vec::new();
        self.collect_and_chain_guards_inner(lhs, scope_idx, &mut guards);
        guards
    }

    fn collect_and_chain_guards_inner(&self, expr: &Expression, scope_idx: ScopeIndex, guards: &mut Vec<(SymbolIndex, GuardNarrow)>) {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Recurse into LHS to collect earlier guards
                    self.collect_and_chain_guards_inner(lhs, scope_idx, guards);
                    // The RHS of this inner `and` is also a guard for the outer RHS
                    if let Some(g) = self.detect_and_lhs_guard_leaf(rhs, scope_idx) {
                        guards.push(g);
                    }
                }
                return;
            }
            // Flat form: BinaryExpr(None, [x, BinaryExpr(And, ...)])
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [lhs, Expression::BinaryExpression(rhs_bin)] = terms.as_slice() {
                    if matches!(rhs_bin.kind(), Operator::And) {
                        self.collect_and_chain_guards_inner(lhs, scope_idx, guards);
                        let rhs_terms = rhs_bin.get_terms();
                        if let [_, rhs_of_and] = rhs_terms.as_slice() {
                            if let Some(g) = self.detect_and_lhs_guard_leaf(rhs_of_and, scope_idx) {
                                guards.push(g);
                            }
                        }
                        return;
                    }
                }
            }
        }
        // Base case: a leaf expression (identifier or comparison)
        if let Some(g) = self.detect_and_lhs_guard_leaf(expr, scope_idx) {
            guards.push(g);
        }
    }

    /// Detect a guard from a single (non-chain) expression — bare name, `x ~= nil`, or type guard.
    fn detect_and_lhs_guard_leaf(&self, expr: &Expression, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        if let Expression::Identifier(ident) = expr {
            let names = ident.names();
            if names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                    .map(|s| (s, GuardNarrow::StripFalsy));
            }
        }
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    if let Some(sym_idx) = self.extract_type_guard_symbol(l, r, scope_idx)
                        .or_else(|| self.extract_cached_type_guard_symbol(l, r, scope_idx))
                    {
                        let narrowed_type = Self::extract_type_name_literal(l, r)
                            .and_then(Self::type_name_to_value_type);
                        return Some((sym_idx, match narrowed_type {
                            Some(vt) => GuardNarrow::FilterTo(vt),
                            None => GuardNarrow::StripNil,
                        }));
                    }
                }
            }
        }
        None
    }

    /// When lowering `a or b` where `a` is an inverse nil guard (e.g. `not x`,
    /// `x == nil`), detect which symbol should be narrowed for the RHS.
    /// In `not x or f(x)`, if `not x` is true (x is nil), the or short-circuits;
    /// so when f(x) executes, x must be non-nil.
    fn detect_or_lhs_guard(&self, lhs: &Expression, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        // `not x or ...` → x is truthy in RHS (strip nil + false)
        if let Expression::UnaryExpression(u) = lhs {
            if matches!(u.kind(), Operator::Not) {
                let terms = u.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                            .map(|s| (s, GuardNarrow::StripFalsy));
                    }
                }
            }
        }
        // `x == nil or ...` → x is non-nil in RHS
        if let Expression::BinaryExpression(bin) = lhs {
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
        }
        None
    }

    /// When lowering `a or b` where `a` is an inverse field nil guard
    /// (e.g. `not self.field`, `self.field == nil`), detect the guarded field.
    fn detect_or_lhs_field_guard(&self, lhs: &Expression, scope_idx: ScopeIndex) -> Option<(SymbolIndex, Vec<String>)> {
        // `not self.field or ...` or `not self._state.x or ...`
        if let Expression::UnaryExpression(u) = lhs {
            if matches!(u.kind(), Operator::Not) {
                let terms = u.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() >= 2 {
                        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                        return Some((sym_idx, names[1..].to_vec()));
                    }
                }
            }
        }
        // `self.field == nil or ...` or `self._state.x == nil or ...`
        if let Expression::BinaryExpression(bin) = lhs {
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() >= 2 {
                            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                            return Some((sym_idx, names[1..].to_vec()));
                        }
                    }
                }
            }
        }
        None
    }

    pub(super) fn lower_function_call(&mut self, call: &FunctionCall, scope_idx: ScopeIndex, ret_index: usize, discarded: bool) -> ExprId {
        let is_method_call = call.identifier().is_some_and(|ident| ident.is_call_to_self());
        let func_id = if let Some(ident) = call.identifier() {
            self.lower_expression(&Expression::Identifier(ident), scope_idx)
        } else if let Some(inner_call) = call.syntax().children().find_map(FunctionCall::cast) {
            // Chained call: f(args1)(args2) — the callee is itself a FunctionCall.
            // Recursively lower it so its arguments are tracked.
            self.lower_function_call(&inner_call, scope_idx, 0, false)
        } else {
            self.ir.push_expr(Expr::Unknown)
        };
        let (args, arg_ranges): (Vec<ExprId>, Vec<(u32, u32)>) = call.arguments()
            .map(|arg_list| arg_list.expressions().iter()
                .map(|expr| {
                    let r = expr.syntax().text_range();
                    (self.lower_expression(expr, scope_idx), (u32::from(r.start()), trimmed_node_end(expr.syntax())))
                })
                .unzip())
            .unwrap_or_default();
        let range = call.syntax().text_range();
        let call_range = (u32::from(range.start()), u32::from(range.end()));
        let expr_id = self.ir.push_expr(Expr::FunctionCall { func: func_id, args, arg_ranges, ret_index, call_range, discarded, is_method_call });
        self.deferred.call_exprs.push(expr_id);
        expr_id
    }

    pub(super) fn insert_function_definition(&mut self, func: &FunctionDefinition, scope_idx: ScopeIndex, inject_self: bool) -> ScopeIndex {
        let node = SyntaxNodePtr::new(func.syntax());
        let params = func
            .params()
            .expect("FunctionDefinition should have params");
        let param_names = params.parameters();
        let is_vararg = params.ellipsis();
        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
        let mut function = Function {
            def_node: node,
            scope: new_scope_idx,
            args: Vec::new(),
            rets: Vec::new(),
            return_annotations: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            generics: Vec::new(),
            generic_constraints_raw: Vec::new(),
            param_annotations: Vec::new(),
            param_descriptions: Vec::new(),
            defclass: None,
            defclass_parent: None,
            is_vararg,
            param_optional: Vec::new(),
            returns_self: false,
            explicit_void_return: false, constructor: false,
            builds_field: None,
            built_name: None,
            built_extends: false,
            returns_built: false,
            returns_built_parent: None,
            dot_defined: !inject_self,
            type_narrows: None,
        };
        if inject_self {
            function.args.push(self.ir.insert_symbol(SymbolIdentifier::Name("self".to_string()), new_scope_idx, node));
        }
        for name in param_names.iter() {
            // Store args as Name so they're findable by normal scope lookup
            function.args.push(self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), new_scope_idx, node));
        }
        self.ir.functions.push(function);
        // Register parameter list range so scope_at_offset finds params
        if let Some(params_node) = func.params() {
            self.ir.block_scopes.push((params_node.syntax().text_range(), new_scope_idx));
        }
        new_scope_idx
    }

    pub(super) fn apply_annotations(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: &SyntaxNode) {
        self.apply_annotations_with_owner(func_idx, _scope_idx, node, None);
    }

    pub(super) fn apply_annotations_with_owner(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: &SyntaxNode, owner_class_name: Option<&str>) {
        let annotations = extract_annotations(node);
        let generics = &annotations.generics;

        // Store resolved generics on the function
        if !generics.is_empty() {
            let resolved_generics: Vec<(String, Option<ValueType>)> = generics.iter().map(|(name, constraint)| {
                let resolved_constraint = constraint.as_ref().and_then(|c| {
                    let base = c.split('<').next().unwrap_or(c);
                    self.resolve_annotation_type(&AnnotationType::Simple(base.to_string()))
                });
                (name.clone(), resolved_constraint)
            }).collect();
            self.ir.functions[func_idx].generics = resolved_generics;
            self.ir.functions[func_idx].generic_constraints_raw = generics.clone();
        }

        // Apply @param annotations to matching function arguments
        // Also store raw annotations on Function for generic inference from structured types
        let func_args = self.ir.functions[func_idx].args.clone();
        let mut param_annotations = vec![AnnotationType::Simple(String::new()); func_args.len()];
        let mut param_descriptions: Vec<Option<String>> = vec![None; func_args.len()];
        for p in annotations.params.iter() {
            let resolved_vt = self.resolve_annotation_type_mut_gen(&p.typ, generics);
            // Always record the raw annotation type (even for `any` which resolves to None)
            for (i, &arg_sym_idx) in func_args.iter().enumerate() {
                if self.ir.symbols[arg_sym_idx].id == SymbolIdentifier::Name(p.name.clone()) {
                    if let Some(vt) = resolved_vt.clone() {
                        let vt = if p.optional {
                            ValueType::union(vt, ValueType::Nil)
                        } else {
                            vt
                        };
                        let expr_id = self.ir.push_expr(Expr::Literal(vt));
                        self.ir.set_type_source(arg_sym_idx, expr_id);
                        // Store resolved type args for parameterized param annotations
                        if let AnnotationType::Parameterized(_, ref type_arg_annotations) = p.typ {
                            let type_args: Vec<ValueType> = type_arg_annotations.iter()
                                .filter_map(|ta| self.resolve_annotation_type_gen(ta, generics))
                                .collect();
                            if !type_args.is_empty() {
                                if let Some(ver) = self.ir.symbols[arg_sym_idx].versions.last_mut() {
                                    ver.type_args = type_args;
                                }
                            }
                        }
                    }
                    param_annotations[i] = p.typ.clone();
                    param_descriptions[i] = p.description.clone();
                    break;
                }
            }
        }
        self.ir.functions[func_idx].param_annotations = param_annotations;
        self.ir.functions[func_idx].param_descriptions = param_descriptions;

        // Collect annotation comment ranges once for param name + type checks
        let comment_ranges = Self::collect_preceding_annotation_ranges(node);
        let func_start = u32::from(node.text_range().start()) as usize;
        let func_end = func_start + "function".len();

        // Check for undefined/duplicate @param names
        if !annotations.params.is_empty() {
            let arg_names: HashSet<String> = func_args.iter()
                .filter_map(|&sym_idx| match &self.ir.symbols[sym_idx].id {
                    SymbolIdentifier::Name(n) => Some(n.clone()),
                    _ => None,
                })
                .collect();
            let mut seen_params: HashSet<String> = HashSet::new();
            for p in annotations.params.iter() {
                let (s, e) = comment_ranges.iter()
                    .find(|(text, _, _)| text.starts_with("---@param") && text.contains(&p.name))
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                if !seen_params.insert(p.name.clone()) {
                    crate::diagnostics::duplicate_doc_param::check(
                        &mut self.diagnostics, &p.name,
                        s, e,
                    );
                } else if !arg_names.contains(&p.name) && p.name != "self" && !(p.name == "..." && self.ir.functions[func_idx].is_vararg) {
                    crate::diagnostics::undefined_doc_param::check(
                        &mut self.diagnostics, &p.name,
                        s, e,
                    );
                }
            }
        }

        // Build param_optional from annotation optional markers
        // Match optional annotations to function args by name
        let mut param_optional = vec![false; func_args.len()];
        for p in annotations.params.iter() {
            if p.optional {
                for (i, &arg_sym_idx) in func_args.iter().enumerate() {
                    if self.ir.symbols[arg_sym_idx].id == SymbolIdentifier::Name(p.name.clone()) {
                        param_optional[i] = true;
                        break;
                    }
                }
            }
        }
        self.ir.functions[func_idx].param_optional = param_optional;

        // Also propagate is_vararg from overloads if any overload has varargs
        if annotations.overloads.iter().any(|s| {
            crate::annotations::parse_overload(s).map_or(false, |sig| sig.is_vararg)
        }) {
            self.ir.functions[func_idx].is_vararg = true;
        }

        // Apply @return annotations
        if !annotations.returns.is_empty() {
            let node_ptr = SyntaxNodePtr::new(node);
            let func_scope = self.ir.functions[func_idx].scope;
            let mut return_vts = Vec::new();
            for (i, ret_annotation) in annotations.returns.iter().enumerate() {
                // @return self — mark the function as returning self
                if matches!(ret_annotation, crate::annotations::AnnotationType::Simple(s) if s == "self") {
                    self.ir.functions[func_idx].returns_self = true;
                    continue;
                }
                // @return built [: Parent] — mark the function as returning the built type
                if let crate::annotations::AnnotationType::Simple(s) = ret_annotation {
                    if s == "built" {
                        self.ir.functions[func_idx].returns_built = true;
                        continue;
                    }
                    if let Some(parent) = s.strip_prefix("built:") {
                        self.ir.functions[func_idx].returns_built = true;
                        self.ir.functions[func_idx].returns_built_parent = Some(parent.to_string());
                        continue;
                    }
                }
                if let Some(vt) = self.resolve_annotation_type_mut_gen(ret_annotation, generics) {
                    let ret_expr = self.ir.push_expr(Expr::Literal(vt.clone()));
                    let ret_sym_idx = self.ir.insert_symbol(
                        SymbolIdentifier::FunctionRet(func_idx, i),
                        func_scope,
                        node_ptr,
                    );
                    self.ir.set_type_source(ret_sym_idx, ret_expr);
                    self.ir.functions[func_idx].rets.push(ret_sym_idx);
                    return_vts.push(vt);
                }
            }
            self.ir.functions[func_idx].return_annotations = return_vts;
        }

        // Apply @builds-field annotation
        if let Some((param_idx, ref field_ann)) = annotations.builds_field {
            if let Some(vt) = self.resolve_annotation_type_gen(field_ann, generics) {
                self.ir.functions[func_idx].builds_field = Some((param_idx, vt));
            }
        }

        // Apply @built-name annotation
        if let Some(param_idx) = annotations.built_name {
            self.ir.functions[func_idx].built_name = Some(param_idx);
        }

        // Apply @built-extends annotation
        if annotations.built_extends {
            self.ir.functions[func_idx].built_extends = true;
        }

        // Apply @type-narrows annotation
        if let Some((target, classname)) = annotations.type_narrows {
            self.ir.functions[func_idx].type_narrows = Some((target, classname));
        }

        // Check for @return ClassName on methods of that class
        if let Some(class_name) = owner_class_name {
            let returns_own_class = annotations.returns.iter().any(|rt| {
                matches!(rt, crate::annotations::AnnotationType::Simple(s) if s == class_name)
            });
            if returns_own_class {
                let r = node.text_range();
                let start = u32::from(r.start()) as usize;
                let end = u32::from(r.end()) as usize;
                if self.ir.functions[func_idx].builds_field.is_some() {
                    crate::diagnostics::builds_field_not_self::check(
                        &mut self.diagnostics, class_name, start, end,
                    );
                } else {
                    crate::diagnostics::return_self_class_name::check(
                        &mut self.diagnostics, class_name, start, end,
                    );
                }
            }
        }

        // Apply @overload annotations
        if !annotations.overloads.is_empty() {
            let overloads: Vec<ResolvedOverload> = annotations.overloads.iter()
                .filter_map(|s| crate::annotations::parse_overload(s))
                .map(|sig| {
                    let params = sig.params.iter().map(|p| {
                        crate::types::ResolvedOverloadParam {
                            name: p.name.clone(),
                            typ: self.resolve_annotation_type_mut_gen(&p.typ, generics),
                            optional: p.optional,
                        }
                    }).collect();
                    let returns = sig.returns.iter()
                        .filter_map(|at| self.resolve_annotation_type_mut_gen(at, generics))
                        .collect();
                    ResolvedOverload { params, returns, is_return_only: sig.is_return_only }
                })
                .collect();
            self.ir.functions[func_idx].overloads = overloads;
        }

        // Validate return-only overloads against @return annotations
        {
            let return_only: Vec<_> = self.ir.functions[func_idx].overloads.iter()
                .filter(|o| o.is_return_only)
                .collect();
            if !return_only.is_empty() {
                let ret_count = self.ir.functions[func_idx].return_annotations.len();
                // @overload return: without any @return annotations
                if ret_count == 0 {
                    crate::diagnostics::malformed_annotation::check(
                        &mut self.diagnostics,
                        "@overload return: requires corresponding @return annotations".to_string(),
                        func_start, func_end,
                    );
                } else {
                    // @overload return: type count doesn't match @return count
                    // (skip nil/empty overloads — they validly represent "no returns")
                    for overload_str in &annotations.overloads {
                        if let Some(sig) = crate::annotations::parse_overload(overload_str) {
                            if sig.is_return_only && !sig.returns.is_empty() {
                                let is_nil_only = sig.returns.len() == 1
                                    && matches!(&sig.returns[0], crate::annotations::AnnotationType::Simple(s) if s == "nil");
                                if !is_nil_only && sig.returns.len() != ret_count {
                                    crate::diagnostics::malformed_annotation::check(
                                        &mut self.diagnostics,
                                        format!(
                                            "@overload return: has {} type(s) but {} @return annotation(s) declared",
                                            sig.returns.len(), ret_count,
                                        ),
                                        func_start, func_end,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check for undefined class references in annotation types
        // Use the actual comment token ranges so diagnostics appear on the annotation, not the function
        {
            let mut diags = Vec::new();
            for p in annotations.params.iter() {
                let (s, e) = comment_ranges.iter()
                    .find(|(text, _, _)| text.starts_with("---@param") && text.contains(&p.name))
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                self.check_annotation_type_names(&p.typ, generics, s, e, &mut diags);
            }
            for (i, ret) in annotations.returns.iter().enumerate() {
                // Find the i-th @return comment
                let (s, e) = comment_ranges.iter()
                    .filter(|(text, _, _)| text.starts_with("---@return"))
                    .nth(i)
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                self.check_annotation_type_names(ret, generics, s, e, &mut diags);
            }
            for (i, overload_str) in annotations.overloads.iter().enumerate() {
                if let Some(sig) = crate::annotations::parse_overload(overload_str) {
                    let (s, e) = comment_ranges.iter()
                        .filter(|(text, _, _)| text.starts_with("---@overload"))
                        .nth(i)
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_end));
                    for p in &sig.params {
                        self.check_annotation_type_names(&p.typ, generics, s, e, &mut diags);
                    }
                    for ret in &sig.returns {
                        self.check_annotation_type_names(ret, generics, s, e, &mut diags);
                    }
                }
            }
            // Note: generic constraint types (e.g. `Class` in `@generic T: Class`)
            // are not checked here — they commonly reference types defined in other
            // project files and would produce false-positive undefined-doc-class warnings.
            self.diagnostics.extend(diags);
        }

        if annotations.doc.is_some() {
            self.ir.functions[func_idx].doc = annotations.doc;
        }
        if annotations.deprecated {
            self.ir.functions[func_idx].deprecated = true;
        }
        if annotations.nodiscard {
            self.ir.functions[func_idx].nodiscard = true;
        }
        if annotations.constructor {
            self.ir.functions[func_idx].constructor = true;
            // @constructor methods must not have return annotations (except @return self)
            if !self.ir.functions[func_idx].return_annotations.is_empty() {
                let r = node.text_range();
                crate::diagnostics::constructor_return::check(
                    &mut self.diagnostics,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
        if annotations.defclass.is_some() {
            self.ir.functions[func_idx].defclass = annotations.defclass;
            self.ir.functions[func_idx].defclass_parent = annotations.defclass_parent;
        }
    }

    /// Collect the text and byte ranges of annotation comment tokens preceding a node.
    /// Returns vec of (comment_text, start, end) in source order.
    fn collect_preceding_annotation_ranges(node: &SyntaxNode) -> Vec<(String, usize, usize)> {
        let Some(first_token) = node.first_token() else { return Vec::new(); };
        let mut results = Vec::new();
        let mut tok = first_token.prev_token();
        while let Some(token) = tok {
            let kind = token.kind();
            if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
                tok = token.prev_token();
                continue;
            }
            if kind == SyntaxKind::Comment {
                let text = token.text().to_string();
                if text.starts_with("---@") || text.starts_with("---|") || text.starts_with("--- @") {
                    let r = token.text_range();
                    results.push((text, u32::from(r.start()) as usize, u32::from(r.end()) as usize));
                    tok = token.prev_token();
                    continue;
                } else if text.starts_with("---") {
                    tok = token.prev_token();
                    continue;
                }
            }
            break;
        }
        results.reverse();
        results
    }

    /// Scan preceding comments for `---@cast` directives and apply type changes.
    /// Walks backward from a statement's first token (same pattern as extract_annotations).
    fn scan_cast_annotations(&mut self, node: &SyntaxNode, scope_idx: ScopeIndex) {
        let Some(first_token) = node.first_token() else { return };
        let mut cast_lines = Vec::new();
        let mut tok = first_token.prev_token();
        while let Some(token) = tok {
            let kind = token.kind();
            if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
                tok = token.prev_token();
                continue;
            }
            if kind == SyntaxKind::Comment {
                // Skip inline trailing comments (on same line as previous code)
                {
                    let mut prev = token.prev_token();
                    let mut is_inline = false;
                    while let Some(ref p) = prev {
                        if p.kind() == SyntaxKind::Whitespace {
                            prev = p.prev_token();
                            continue;
                        }
                        if p.kind() != SyntaxKind::Newline {
                            is_inline = true;
                        }
                        break;
                    }
                    if is_inline { break; }
                }
                let text = token.text();
                if text.starts_with("---@cast") || text.starts_with("--[[@cast") {
                    cast_lines.push(text.to_string());
                    tok = token.prev_token();
                    continue;
                } else if text.starts_with("---@") || text.starts_with("--- @") || text.starts_with("---") || text.starts_with("---|") {
                    // Other annotation or doc comment — keep scanning backward
                    tok = token.prev_token();
                    continue;
                }
            }
            break;
        }
        cast_lines.reverse();
        for line in &cast_lines {
            // Parse both ---@cast and --[[@cast forms
            let content = if let Some(rest) = line.strip_prefix("---@cast") {
                rest.trim()
            } else if let Some(rest) = line.strip_prefix("--[[@cast") {
                rest.trim().trim_end_matches("]]").trim()
            } else {
                continue;
            };
            let Some((var_name, type_str)) = content.split_once(char::is_whitespace) else { continue };
            let type_str = type_str.trim();
            let (mode, type_str) = if let Some(s) = type_str.strip_prefix('+') {
                (CastMode::Add, s.trim())
            } else if let Some(s) = type_str.strip_prefix('-') {
                (CastMode::Remove, s.trim())
            } else {
                (CastMode::Replace, type_str)
            };
            if type_str.is_empty() { continue; }
            let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(var_name.to_string()), scope_idx) else { continue };
            if sym_idx >= EXT_BASE { continue; }
            let ann_type = crate::annotations::parse_type(type_str);
            let Some(cast_vt) = self.resolve_annotation_type_mut_gen(&ann_type, &[]) else { continue };
            match mode {
                CastMode::Replace => {
                    self.push_type_narrowed_version(sym_idx, cast_vt, scope_idx);
                }
                CastMode::Add => {
                    let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
                    let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
                    let cast_expr = self.ir.push_expr(Expr::CastAdd(prev_ref, cast_vt));
                    let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
                    self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                        def_node: node,
                        type_source: Some(cast_expr),
                        resolved_type: None,
                        type_args: Vec::new(),
                        created_in_scope: scope_idx,
                    });
                }
                CastMode::Remove => {
                    let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
                    let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
                    let cast_expr = self.ir.push_expr(Expr::CastRemove(prev_ref, cast_vt));
                    let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
                    self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                        def_node: node,
                        type_source: Some(cast_expr),
                        resolved_type: None,
                        type_args: Vec::new(),
                        created_in_scope: scope_idx,
                    });
                }
            }
        }
    }

    /// Extract an inline `--[[@as Type]]` annotation from tokens following an expression node.
    /// Supports both `--[[@as Type]]` and `--[=[@as Type[]]=]` (equal-sign block comments for array types).
    fn extract_inline_as(expr_node: &SyntaxNode) -> Option<AnnotationType> {
        let last_token = expr_node.last_token()?;
        // First try: scan forward from the last token (comment is outside the node)
        let mut tok = last_token.next_token();
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Whitespace => {
                    tok = t.next_token();
                }
                SyntaxKind::Comment => {
                    return Self::parse_as_comment(t.text());
                }
                _ => break,
            }
        }
        // Second try: scan backward from the last token (comment is inside the node,
        // e.g. when the parser includes trailing trivia in the expression node)
        let mut tok = Some(last_token);
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Whitespace | SyntaxKind::Newline => {
                    tok = t.prev_token();
                }
                SyntaxKind::Comment => {
                    return Self::parse_as_comment(t.text());
                }
                _ => return None,
            }
        }
        None
    }

    /// Parse a comment token as a potential `@as` annotation.
    fn parse_as_comment(text: &str) -> Option<AnnotationType> {
        let inner = if text.starts_with("--[[") && text.ends_with("]]") {
            Some(&text[4..text.len()-2])
        } else if text.starts_with("--[=[") && text.ends_with("]=]") {
            Some(&text[5..text.len()-3])
        } else {
            None
        };
        if let Some(inner) = inner {
            let inner = inner.trim();
            if let Some(rest) = inner.strip_prefix("@as") {
                let rest = rest.trim();
                if !rest.is_empty() {
                    return Some(crate::annotations::parse_type(rest));
                }
            }
        }
        None
    }

    /// Extract an inline `---@type X` annotation from tokens following a Field node.
    /// Looks at sibling tokens after the field ends (past comma/whitespace) on the same line.
    fn extract_inline_type(field_node: &SyntaxNode) -> Option<AnnotationType> {
        let last_token = field_node.last_token()?;
        let mut tok = last_token.next_token();
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Comma | SyntaxKind::Whitespace | SyntaxKind::Semicolon => {
                    tok = t.next_token();
                }
                SyntaxKind::Comment => {
                    let text = t.text();
                    let content = text.trim_start_matches('-').trim();
                    if let Some(rest) = content.strip_prefix("@type") {
                        let rest = rest.trim();
                        if !rest.is_empty() {
                            return Some(crate::annotations::parse_type(rest));
                        }
                    }
                    return None;
                }
                _ => return None,
            }
        }
        None
    }

    /// Extract a `---@type X` annotation from inside a table constructor's opening line.
    /// Matches the pattern `{ ---@type Foo ... }` where the comment follows the `{`.
    fn extract_table_constructor_type(tc_node: &SyntaxNode) -> Option<AnnotationType> {
        let mut found_open_brace = false;
        for item in tc_node.children_with_tokens() {
            match item {
                rowan::NodeOrToken::Token(ref t) => match t.kind() {
                    SyntaxKind::LeftCurlyBracket => { found_open_brace = true; }
                    SyntaxKind::Whitespace if found_open_brace => {}
                    SyntaxKind::Comment if found_open_brace => {
                        let text = t.text();
                        let content = text.trim_start_matches('-').trim();
                        if let Some(rest) = content.strip_prefix("@type") {
                            let rest = rest.trim();
                            if !rest.is_empty() {
                                return Some(crate::annotations::parse_type(rest));
                            }
                        }
                        return None;
                    }
                    _ if found_open_brace => return None,
                    _ => {}
                },
                rowan::NodeOrToken::Node(_) if found_open_brace => return None,
                _ => {}
            }
        }
        None
    }

}
