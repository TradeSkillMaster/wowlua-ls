use super::*;

/// Refactoring action: extract the selected expression into a new local variable.
///
/// Inserts `local newVar = <expr>` on the line before the containing statement
/// and replaces the selection with `newVar`.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_extract_variable_action(
    uri: &lsp_types::Uri,
    text: &str,
    range: lsp_types::Range,
    tree: &SyntaxTree,
) -> Option<CodeAction> {
    let utf8 = use_utf8();
    let start_offset = crate::lsp::lsp_position_to_offset(text, range.start.line, range.start.character, utf8);
    let end_offset = crate::lsp::lsp_position_to_offset(text, range.end.line, range.end.character, utf8);

    if start_offset >= end_offset { return None; }

    let expr_text = text.get(start_offset as usize..end_offset as usize)?;
    let expr_trimmed = expr_text.trim();
    if expr_trimmed.is_empty() { return None; }

    // Don't offer this when the selection is a complete statement (use Extract Function instead)
    let (stmt_start, stmt_end) = find_enclosing_statement_range(tree, start_offset)?;
    if start_offset <= stmt_start && end_offset >= stmt_end { return None; }

    let indent = get_line_indentation(text, stmt_start);
    let numbers = crate::lsp::SafeLinePositions::new(text);

    // Insert `local newVar = <expr>` on the line before the containing statement.
    let insert_line = numbers.lsp_position(stmt_start as usize, utf8).line;
    let insert_pos = Position { line: insert_line, character: 0 };
    let edit_insert = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: format!("{}local {} = {}\n", indent, EXTRACTED_VAR_NAME, expr_trimmed),
    };

    // Replace the selected expression with the variable name.
    let edit_replace = lsp_types::TextEdit {
        range,
        new_text: EXTRACTED_VAR_NAME.to_string(),
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit_insert, edit_replace]);

    Some(CodeAction {
        title: "Extract to local variable".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Refactoring action: extract selected statements into a new local function.
///
/// Analyzes variables used/defined in the selection to determine parameters
/// and return values, then generates a new `local function` definition and
/// replaces the selected code with a call to it.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_extract_function_action(
    uri: &lsp_types::Uri,
    text: &str,
    range: lsp_types::Range,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    let utf8 = use_utf8();
    let sel_start = crate::lsp::lsp_position_to_offset(text, range.start.line, range.start.character, utf8);
    let sel_end = crate::lsp::lsp_position_to_offset(text, range.end.line, range.end.character, utf8);

    if sel_start >= sel_end { return None; }

    // Find the range covered by complete statements inside the selection.
    let (stmts_start, stmts_end) = find_complete_statements_range(tree, sel_start, sel_end)?;

    let body_text = text.get(stmts_start as usize..stmts_end as usize)?;
    if body_text.trim().is_empty() { return None; }

    let numbers = crate::lsp::SafeLinePositions::new(text);

    let indent = get_line_indentation(text, stmts_start);
    let inner_indent = format!("{}    ", indent);

    // Analyze variable flow.
    let params = find_outer_variables_used_in_range(tree, analysis, stmts_start, stmts_end);
    let returns = find_variables_defined_in_range_used_after(
        tree, analysis, stmts_start, stmts_end, text.len() as u32,
    );

    let params_str = params.join(", ");
    let returns_str = returns.join(", ");

    // Decline when the selection contains return statements: extracting them
    // would break control flow (the `return` exits the extracted function, not
    // the original caller).
    if range_contains_return(tree, stmts_start, stmts_end) {
        return None;
    }

    // Build the extracted function text.
    let body_reindented = reindent_block(body_text, &indent, &inner_indent);
    let mut func_text = format!("{}local function {}({})\n", indent, EXTRACTED_FUNC_NAME, params_str);
    func_text.push_str(&body_reindented);
    if !returns.is_empty() {
        func_text.push_str(&format!("{}    return {}\n", indent, returns_str));
    }
    func_text.push_str(&format!("{}end\n\n", indent));

    // Build the replacement call.
    let call_text = if returns.is_empty() {
        format!("{}{}({})\n", indent, EXTRACTED_FUNC_NAME, params_str)
    } else {
        format!("{}local {} = {}({})\n", indent, returns_str, EXTRACTED_FUNC_NAME, params_str)
    };

    // Insertion point: the start of the enclosing function's definition line,
    // or byte 0 (top of file) when at file scope.  Using `stmts_start` as the
    // fallback would produce two edits at the same document position, which has
    // undefined behaviour in the LSP spec.
    let insert_offset = find_enclosing_function_start(analysis, stmts_start)
        .unwrap_or(0);
    let insert_line = numbers.lsp_position(insert_offset as usize, utf8).line;
    let insert_pos = Position { line: insert_line, character: 0 };

    // Edit 1: insert the new function before the enclosing function.
    let edit_insert = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: func_text,
    };

    // Edit 2: replace the selected statements with the call.
    // Align to full lines so indentation is preserved correctly.
    let replace_start_line = numbers.lsp_position(stmts_start as usize, utf8).line;
    let replace_start = Position { line: replace_start_line, character: 0 };
    // Include the trailing newline after the last statement if present.
    let after_end = if stmts_end < text.len() as u32
        && text.as_bytes().get(stmts_end as usize) == Some(&b'\n')
    {
        stmts_end + 1
    } else {
        stmts_end
    };
    let replace_end = numbers.lsp_position(after_end as usize, utf8);

    let edit_replace = lsp_types::TextEdit {
        range: Range { start: replace_start, end: replace_end },
        new_text: call_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit_insert, edit_replace]);

    Some(CodeAction {
        title: "Extract to function".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Returns `true` for syntax node kinds that correspond to statements.
pub(super) fn is_statement_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AssignStatement
            | SyntaxKind::LocalAssignStatement
            | SyntaxKind::FunctionCall
            | SyntaxKind::MethodCall
            | SyntaxKind::DoBlock
            | SyntaxKind::WhileLoop
            | SyntaxKind::RepeatUntilLoop
            | SyntaxKind::IfChain
            | SyntaxKind::ForCountLoop
            | SyntaxKind::ForInLoop
            | SyntaxKind::FunctionDefinition
            | SyntaxKind::ReturnStatement
    )
}

/// Walk up the tree from `offset` to find the innermost enclosing statement.
/// Returns its `(start, end)` byte range.
pub(super) fn find_enclosing_statement_range(tree: &SyntaxTree, offset: u32) -> Option<(u32, u32)> {
    let token_id = tree.token_at_offset(offset).right_biased()?;
    let mut node_id = tree.token_parent(token_id);
    loop {
        let node = tree.node(node_id);
        if is_statement_kind(node.kind) && node.start != u32::MAX {
            return Some((node.start, node.end));
        }
        node_id = tree.node_parent(node_id)?;
    }
}

/// Find the innermost `Block` node whose range fully contains `[start, end]`.
pub(super) fn find_innermost_block_containing(tree: &SyntaxTree, start: u32, end: u32) -> Option<NodeId> {
    let mut best: Option<(u32, NodeId)> = None;
    for (i, node) in tree.nodes.iter().enumerate() {
        if node.kind != SyntaxKind::Block { continue; }
        if node.start == u32::MAX { continue; }
        if node.start <= start && node.end >= end {
            let len = node.end - node.start;
            match best {
                None => best = Some((len, NodeId(i as u32))),
                Some((best_len, _)) if len < best_len => best = Some((len, NodeId(i as u32))),
                _ => {}
            }
        }
    }
    best.map(|(_, id)| id)
}

/// Find the byte range `(first_stmt_start, last_stmt_end)` for the complete
/// statements that are direct children of the innermost block fully within
/// `[sel_start, sel_end]`.
pub(super) fn find_complete_statements_range(tree: &SyntaxTree, sel_start: u32, sel_end: u32) -> Option<(u32, u32)> {
    let block_id = find_innermost_block_containing(tree, sel_start, sel_end)?;

    let mut first_start: Option<u32> = None;
    let mut last_end: u32 = 0;

    for child_id in tree.child_nodes(block_id) {
        let node = tree.node(child_id);
        if !is_statement_kind(node.kind) { continue; }
        if node.start == u32::MAX { continue; }
        if node.start >= sel_start && node.end <= sel_end {
            if first_start.is_none_or(|s| node.start < s) {
                first_start = Some(node.start);
            }
            if node.end > last_end {
                last_end = node.end;
            }
        }
    }

    let first_start = first_start?;
    if last_end == 0 { return None; }
    Some((first_start, last_end))
}

/// Find the byte offset where the innermost enclosing function definition begins,
/// for use as the insertion point when placing the extracted function.
pub(super) fn find_enclosing_function_start(analysis: &AnalysisResult, offset: u32) -> Option<u32> {
    analysis.ir.local_functions().map(|(_, f)| f)
        .filter(|f| {
            f.def_node.start < offset
                && f.def_node.end > offset
                && f.def_node.start != f.def_node.end
        })
        .min_by_key(|f| f.def_node.end - f.def_node.start)
        .map(|f| f.def_node.start)
}

/// Returns `true` if any `ReturnStatement` node falls entirely within `[start, end]`.
pub(super) fn range_contains_return(tree: &SyntaxTree, start: u32, end: u32) -> bool {
    tree.nodes.iter().any(|node| {
        node.kind == SyntaxKind::ReturnStatement
            && node.start != u32::MAX
            && node.start >= start
            && node.end <= end
    })
}

/// Return the leading whitespace of the line that contains `offset`.
pub(super) fn get_line_indentation(text: &str, offset: u32) -> String {
    let offset = (offset as usize).min(text.len());
    let line_start = text[..offset].rfind('\n').map_or(0, |p| p + 1);
    let line = &text[line_start..];
    let trimmed_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    line[..trimmed_len].to_string()
}

/// Collect the names of local (non-external) variables from an outer scope that
/// are used within the byte range `[start, end)`.  These become parameters of
/// the extracted function.
pub(super) fn find_outer_variables_used_in_range(
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    start: u32,
    end: u32,
) -> Vec<String> {
    let mut seen_syms: HashSet<SymbolIndex> = HashSet::new();
    let mut result = Vec::new();

    for token in tree.all_tokens() {
        if token.kind != SyntaxKind::Name { continue; }
        if token.start < start || token.start >= end { continue; }

        let Some((sym_idx, name, _)) = analysis.find_symbol_at(tree, token.start) else { continue };

        // Skip WoW API globals and already-seen symbols.
        if sym_idx.is_external() { continue; }
        if name == "self" { continue; }
        if !seen_syms.insert(sym_idx) { continue; }

        let sym = analysis.sym(sym_idx);
        let Some(first_version) = sym.versions.first() else { continue };

        // If the symbol's first definition is before this selection it comes
        // from the enclosing scope → treat as a parameter.
        if first_version.def_node.start < start {
            result.push(name);
        }
    }

    result
}

/// Find the names of local variables that are **defined or reassigned** within
/// `[start, end)` and also **used** after `end`.  These become the return
/// values of the extracted function.
///
/// This includes:
/// - Variables introduced (first defined) inside the range.
/// - Outer-scope variables that are *reassigned* inside the range (their
///   modified value must be returned so the caller sees the update).
pub(super) fn find_variables_defined_in_range_used_after(
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    start: u32,
    end: u32,
    file_end: u32,
) -> Vec<String> {
    // Pass 1 – collect symbols that have any version defined inside [start, end),
    // preserving first-encounter order so the returned list is deterministic.
    let mut defined_in_range_ordered: Vec<(SymbolIndex, String)> = Vec::new();
    let mut defined_in_range_set: HashSet<SymbolIndex> = HashSet::new();
    for token in tree.all_tokens() {
        if token.kind != SyntaxKind::Name { continue; }
        if token.start < start || token.start >= end { continue; }

        let Some((sym_idx, name, _)) = analysis.find_symbol_at(tree, token.start) else { continue };
        if sym_idx.is_external() { continue; }
        if defined_in_range_set.contains(&sym_idx) { continue; }

        let sym = analysis.sym(sym_idx);
        // Accept the symbol if *any* version (including reassignments of outer
        // variables) has its definition node inside the selection range.
        let any_version_in_range = sym.versions.iter().any(|v| {
            v.def_node.start >= start && v.def_node.start < end
        });
        if any_version_in_range {
            defined_in_range_set.insert(sym_idx);
            defined_in_range_ordered.push((sym_idx, name));
        }
    }

    if defined_in_range_ordered.is_empty() { return Vec::new(); }

    // Pass 2 – find which of those symbols are referenced after `end`.
    let mut used_after: HashSet<SymbolIndex> = HashSet::new();
    for token in tree.all_tokens() {
        if token.kind != SyntaxKind::Name { continue; }
        if token.start < end || token.start >= file_end { continue; }

        let Some((sym_idx, _, _)) = analysis.find_symbol_at(tree, token.start) else { continue };
        if defined_in_range_set.contains(&sym_idx) {
            used_after.insert(sym_idx);
        }
    }

    // Filter the definition-ordered list to only those used after the range.
    defined_in_range_ordered
        .into_iter()
        .filter(|(idx, _)| used_after.contains(idx))
        .map(|(_, name)| name)
        .collect()
}

/// Re-indent a block of text: strip `old_indent` from the start of each line
/// and prepend `new_indent`.
pub(super) fn reindent_block(text: &str, old_indent: &str, new_indent: &str) -> String {
    let mut result = String::new();
    for line in text.split('\n') {
        if line.trim().is_empty() {
            // Preserve blank lines without adding spurious whitespace.
            result.push('\n');
        } else if let Some(stripped) = line.strip_prefix(old_indent) {
            result.push_str(new_indent);
            result.push_str(stripped);
            result.push('\n');
        } else {
            // Line has less indentation than expected — keep as-is relative to new_indent.
            result.push_str(new_indent);
            result.push_str(line.trim_start());
            result.push('\n');
        }
    }
    // Drop a trailing blank line that would be added by the final split.
    if result.ends_with("\n\n") {
        result.pop();
    }
    result
}
