use super::*;

pub fn compute_code_actions(
    uri: &lsp_types::Uri,
    text: &str,
    range: lsp_types::Range,
    context_diagnostics: &[lsp_types::Diagnostic],
    tree_and_analysis: Option<(&SyntaxTree, &AnalysisResult)>,
) -> Vec<CodeActionOrCommand> {
    let mut actions: Vec<CodeActionOrCommand> = Vec::new();

    // Collect the *first* quickfix edit per diagnostic occurrence, grouped by
    // diagnostic code.  Using only the first action avoids inflating the count
    // or producing conflicting edits when a single diagnostic yields multiple
    // alternative fixes.  BTreeMap gives stable, alphabetical emit order.
    let mut fix_groups: BTreeMap<String, Vec<Vec<lsp_types::TextEdit>>> = BTreeMap::new();

    for diag in context_diagnostics {
        let code_str = match &diag.code {
            Some(NumberOrString::String(s)) => s.as_str(),
            _ => continue,
        };
        if diag.source.as_deref() != Some("wowlua_ls") {
            continue;
        }

        // Quick fixes (shown before suppression actions)
        let quick_fixes = compute_quick_fixes(uri, text, diag, tree_and_analysis);

        // Record the edits from the *first* fix action that targets this file.
        // Iterating further would count alternative fixes as extra occurrences.
        for action in &quick_fixes {
            if let CodeActionOrCommand::CodeAction(ca) = action
                && let Some(edit) = &ca.edit
                && let Some(changes) = &edit.changes
                && let Some(file_edits) = changes.get(uri)
            {
                fix_groups.entry(code_str.to_string())
                    .or_default()
                    .push(file_edits.clone());
                break; // one entry per diagnostic occurrence
            }
        }

        actions.extend(quick_fixes);

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_line_action(uri, text, diag, code_str),
        ));

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_next_line_action(uri, text, diag, code_str),
        ));

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_file_action(uri, text, diag, code_str),
        ));
    }

    // Emit "Fix all 'code' in this file (N occurrences)" for codes with 2+
    // fixable instances.  BTreeMap iteration is sorted, so the bulk actions
    // appear in a stable, alphabetical order regardless of diagnostic ordering.
    for (code_str, edit_groups) in &fix_groups {
        if edit_groups.len() < 2 {
            continue;
        }
        let n = edit_groups.len();
        let all_edits: Vec<lsp_types::TextEdit> =
            edit_groups.iter().flatten().cloned().collect();
        let Some(merged) = merge_edits_for_fix_all(all_edits) else { continue };
        // `lsp_types::Uri` contains an `Arc` for reference counting only; it is
        // never mutated through hash/eq, so using it as a HashMap key is safe.
        #[allow(clippy::mutable_key_type)]
        let mut changes = HashMap::new();
        changes.insert(uri.clone(), merged);
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Fix all '{}' in this file ({} occurrences)", code_str, n),
            kind: Some(CodeActionKind::QUICKFIX),
            is_preferred: Some(false),
            edit: Some(lsp_types::WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }));
    }

    // Source action: offer annotation stubs for the function at cursor position.
    let cursor_offset = crate::lsp::lsp_position_to_offset(
        text, range.start.line, range.start.character, use_utf8(),
    );
    if let Some(action) = make_generate_annotation_stubs_source_action(uri, text, cursor_offset, tree_and_analysis) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // Refactor: combine multiple `---@return` lines into a single-line tuple return.
    if let Some(action) = make_combine_returns_action(uri, text, cursor_offset, tree_and_analysis) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // Refactoring actions (only when there's a real selection)
    if range.start != range.end
        && let Some((tree, analysis)) = tree_and_analysis
    {
        if let Some(action) = make_extract_variable_action(uri, text, range, tree) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
        if let Some(action) = make_extract_function_action(uri, text, range, tree, analysis) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
    }

    actions
}

/// Merge TextEdits for a "fix all" batch action.
///
/// - Pure-insertion edits (`range.start == range.end`) at the same position are
///   concatenated so that multiple fields injected into the same class land
///   adjacent to each other.
/// - All edits are sorted descending by start position (bottom-to-top) so that
///   applying them does not shift the byte positions of earlier edits in the file.
/// - Returns `None` if any two replacement edits have overlapping ranges, which
///   would corrupt the document; the caller skips the bulk action in that case.
pub(super) fn merge_edits_for_fix_all(edits: Vec<lsp_types::TextEdit>) -> Option<Vec<lsp_types::TextEdit>> {
    let (mut insertions, mut replacements): (Vec<_>, Vec<_>) = edits
        .into_iter()
        .partition(|e| e.range.start == e.range.end);

    // Sort replacements by start position so we can check for overlaps in one pass.
    replacements.sort_by_key(|e| (e.range.start.line, e.range.start.character));
    for pair in replacements.windows(2) {
        // Two replacements overlap when the earlier one's end is after the later
        // one's start (comparing line/character lexicographically).
        let end = pair[0].range.end;
        let next_start = pair[1].range.start;
        if (end.line, end.character) > (next_start.line, next_start.character) {
            return None;
        }
    }

    // Sort ascending so same-position insertions are adjacent.
    insertions.sort_by_key(|e| (e.range.start.line, e.range.start.character));

    // Merge consecutive insertions at the same position.
    let mut merged: Vec<lsp_types::TextEdit> = Vec::new();
    for ins in insertions {
        if let Some(last) = merged.last_mut()
            && last.range.start == ins.range.start
        {
            last.new_text.push_str(&ins.new_text);
            continue;
        }
        merged.push(ins);
    }

    merged.extend(replacements);

    // Sort bottom-to-top so applying them does not shift preceding edit positions.
    merged.sort_by(|a, b| {
        b.range.start.line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });

    Some(merged)
}

/// Compute targeted quick fix actions for a single diagnostic.
/// Exported for integration testing.
pub fn compute_quick_fixes(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    tree_and_analysis: Option<(&SyntaxTree, &AnalysisResult)>,
) -> Vec<CodeActionOrCommand> {
    let code_str = match &diag.code {
        Some(NumberOrString::String(s)) => s.as_str(),
        _ => return vec![],
    };

    match code_str {
        "unused-local" => {
            vec![CodeActionOrCommand::CodeAction(make_prefix_underscore_action(uri, diag))]
        }
        "inject-field" => {
            let Some((_, analysis)) = tree_and_analysis else { return vec![] };
            make_add_field_action(uri, text, diag, analysis)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "incomplete-signature-doc" => {
            let Some((tree, analysis)) = tree_and_analysis else { return vec![] };
            make_generate_annotations_action(uri, text, diag, tree, analysis)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "undefined-global" => {
            make_add_local_declaration_action(uri, text, diag)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "type-mismatch" | "return-mismatch" | "field-type-mismatch" | "assign-type-mismatch" => {
            make_as_cast_action(uri, diag)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "missing-fields" => {
            let Some((_, analysis)) = tree_and_analysis else { return vec![] };
            make_fill_missing_fields_action(uri, text, diag, analysis)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "invalid-op" => {
            let Some((tree, analysis)) = tree_and_analysis else { return vec![] };
            make_nil_coalesce_action(uri, text, diag, tree, analysis)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        _ => vec![],
    }
}

/// Quick fix for `unused-local`: prefix the variable name with `_`.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_prefix_underscore_action(
    uri: &lsp_types::Uri,
    diag: &lsp_types::Diagnostic,
) -> CodeAction {
    let insert_pos = diag.range.start;
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: "_".to_string(),
    };
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    CodeAction {
        title: "Prefix with `_`".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Quick fix for `inject-field`: insert a `---@field name type` annotation above the `@class`.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_add_field_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    // Parse field name and class name from message:
    // "injecting undefined field 'NAME' into class 'CLASS'"
    let msg = diag.message.as_str();
    let after = msg.strip_prefix("injecting undefined field '")?;
    let (field_name, rest) = after.split_once("' into class '")?;
    let class_name = rest.strip_suffix('\'')?;

    // Only offer the fix when the class is defined in this file.
    let &(class_start, _) = analysis.ir.class_def_ranges.get(class_name)?;

    // Convert class annotation start to line number.
    let numbers = crate::lsp::SafeLinePositions::new(text);
    let (class_line, _) = numbers.line_col(class_start as usize);

    // Try to infer the field type from the matching FieldAssignment.
    let byte_offset = crate::lsp::lsp_position_to_offset(text, diag.range.start.line, diag.range.start.character, use_utf8());
    let field_type_str = analysis.ir.field_assignments.iter()
        .find(|fa| fa.ident_start == byte_offset)
        .and_then(|fa| analysis.resolve_expr_type(fa.actual_expr))
        .filter(|vt| !matches!(vt, ValueType::Any))
        .map(|vt| analysis.format_type_depth(&vt, 1))
        .unwrap_or_else(|| "any".to_string());

    // Insert `---@field name type` on the line immediately after the `---@class` annotation.
    let insert_pos = Position { line: class_line.0 + 1, character: 0 };
    let new_text = format!("---@field {} {}\n", field_name, field_type_str);
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: format!("Add `@field {}` to `{}`", field_name, class_name),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Quick fix for `incomplete-signature-doc`: generate missing `@param`/`@return` annotations.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_generate_annotations_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    _tree: &SyntaxTree,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    let byte_offset = crate::lsp::lsp_position_to_offset(text, diag.range.start.line, diag.range.start.character, use_utf8());

    // Find the enclosing function by byte range.
    let func = analysis.ir.local_functions().map(|(_, f)| f).find(|f| {
        f.def_node.start <= byte_offset && byte_offset <= f.def_node.end
    })?;

    let sentinel = AnnotationType::Simple(String::new());

    // Collect @param lines for unannotated parameters (skip self).
    let mut annotation_lines: Vec<String> = Vec::new();
    for (arg_idx, &sym_idx) in func.args.iter().enumerate() {
        let name = match &analysis.sym(sym_idx).id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => continue,
        };
        if name == "self" { continue; }
        let has_annotation = func.param_annotations.get(arg_idx)
            .is_some_and(|a| a != &sentinel);
        if has_annotation { continue; }
        // Try to get the inferred type; fall back to "any".
        let type_str = analysis.sym(sym_idx).versions.last()
            .and_then(|v| v.resolved_type.as_ref())
            .filter(|vt| !matches!(vt, ValueType::Any | ValueType::Nil))
            .map(|vt| analysis.format_type_depth(vt, 1))
            .unwrap_or_else(|| "any".to_string());
        annotation_lines.push(format!("---@param {} {}", name, type_str));
    }

    // Add @param for varargs if unannotated.
    if func.is_vararg && func.vararg_annotation.is_none() {
        annotation_lines.push("---@param ... any".to_string());
    }

    // Add @return if missing.
    let needs_return = func.return_annotations.is_empty()
        && !func.returns_self
        && !func.returns_built;
    if needs_return {
        annotation_lines.push("---@return any".to_string());
    }

    if annotation_lines.is_empty() { return None; }

    // Get the indentation of the function definition line.
    let numbers = crate::lsp::SafeLinePositions::new(text);
    let (func_start_line, _) = numbers.line_col(func.def_node.start as usize);
    let indent = text.split('\n')
        .nth(func_start_line.0 as usize)
        .map(|l| {
            let trimmed = l.trim_start();
            &l[..l.len() - trimmed.len()]
        })
        .unwrap_or("");

    let new_text: String = annotation_lines.iter()
        .map(|l| format!("{}{}\n", indent, l))
        .collect();

    let insert_pos = Position { line: func_start_line.0, character: 0 };
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text,
    };

    let title = if annotation_lines.len() == 1 {
        format!("Add `{}`", annotation_lines[0])
    } else {
        "Generate missing annotations".to_string()
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Source action: generate all missing `---@param` / `---@return` annotation stubs for the
/// function enclosing the cursor. Fires regardless of whether any diagnostic is active —
/// it only requires at least one annotation to be missing.
#[allow(clippy::mutable_key_type)]
pub fn make_generate_annotation_stubs_source_action(
    uri: &lsp_types::Uri,
    text: &str,
    cursor_offset: u32,
    tree_and_analysis: Option<(&SyntaxTree, &AnalysisResult)>,
) -> Option<CodeAction> {
    let (_, analysis) = tree_and_analysis?;

    // Find the innermost function whose def_node span contains the cursor.
    // We search by def_node (start..end) rather than enclosing_function_at() because
    // enclosing_function_at() is scope-based: scopes start inside the body, so a
    // cursor on the `function` keyword line (which is the most natural place for
    // this action) would not be covered. def_node.end is an exclusive bound
    // (TextRange convention), so the comparison is `start <= cursor < end`.
    let func = analysis.ir.local_functions().map(|(_, f)| f)
        .filter(|f| f.def_node.start <= cursor_offset && cursor_offset < f.def_node.end)
        .min_by_key(|f| f.def_node.end - f.def_node.start)?;

    // Collect @param lines for unannotated parameters (skip self).
    // Use the same sentinel-detection pattern as build_ir.rs: an unannotated
    // parameter slot holds `AnnotationType::Simple("")`.
    let mut annotation_lines: Vec<String> = Vec::new();
    for (arg_idx, &sym_idx) in func.args.iter().enumerate() {
        let name = match &analysis.sym(sym_idx).id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => continue,
        };
        if name == "self" { continue; }
        let is_annotated = func.param_annotations.get(arg_idx)
            .is_some_and(|a| !matches!(a, AnnotationType::Simple(s) if s.is_empty()));
        if is_annotated { continue; }
        let type_str = analysis.sym(sym_idx).versions.last()
            .and_then(|v| v.resolved_type.as_ref())
            .filter(|vt| !matches!(vt, ValueType::Any | ValueType::Nil))
            .map(|vt| analysis.format_type_depth(vt, 1))
            .unwrap_or_else(|| "any".to_string());
        annotation_lines.push(format!("---@param {} {}", name, type_str));
    }

    // Add @param for varargs if unannotated.
    if func.is_vararg && func.vararg_annotation.is_none() {
        annotation_lines.push("---@param ... any".to_string());
    }

    // Add @return stubs when the function has no return annotations and the body
    // actually returns a value (format_inferred_returns returns empty for void functions).
    // Use inferred types when available; fall back to "any" for unknown positions.
    if func.return_annotations.is_empty() && !func.returns_self && !func.returns_built {
        let inferred = analysis.format_inferred_returns(func, 1);
        for type_str in &inferred {
            let display = if type_str == "?" { "any".to_string() } else { type_str.clone() };
            annotation_lines.push(format!("---@return {}", display));
        }
    }

    if annotation_lines.is_empty() { return None; }

    // Get the indentation of the function definition line.
    let numbers = crate::lsp::SafeLinePositions::new(text);
    let (func_start_line, _) = numbers.line_col(func.def_node.start as usize);
    let indent = text.split('\n')
        .nth(func_start_line.0 as usize)
        .map(|l| {
            let trimmed = l.trim_start();
            &l[..l.len() - trimmed.len()]
        })
        .unwrap_or("");

    let new_text: String = annotation_lines.iter()
        .map(|l| format!("{}{}\n", indent, l))
        .collect();

    let insert_pos = Position { line: func_start_line.0, character: 0 };
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: "Generate annotation stubs".to_string(),
        kind: Some(CodeActionKind::SOURCE),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// If `line` is a `---@return` doc-comment, return the text following `@return`
/// (trimmed). Accepts extra leading dashes (`----`) and an optional space before
/// the tag (`--- @return`). Returns `None` for any other line.
pub(super) fn return_annotation_body(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let t = t.strip_prefix("---")?;
    let t = t.trim_start_matches('-').trim_start();
    let after = t.strip_prefix("@return")?;
    // Guard against `@returns` and friends: the tag must be followed by
    // whitespace or end-of-line.
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }
    Some(after.trim())
}

/// Refactor: combine a contiguous run of two or more `---@return` lines into a
/// single-line tuple return, e.g.
///
/// ```text
/// ---@return boolean success
/// ---@return number? numInvalidItems
/// ---@return number? numChangedOperations
/// ```
///
/// becomes
///
/// ```text
/// ---@return (boolean success, number? numInvalidItems, number? numChangedOperations)
/// ```
///
/// Fires when the cursor sits on one of the `@return` comment lines, or inside a
/// function whose annotation block ends with such a run. Per-position trailing
/// prose descriptions are dropped (the tuple shorthand has no slot for them).
#[allow(clippy::mutable_key_type)]
pub(super) fn make_combine_returns_action(
    uri: &lsp_types::Uri,
    text: &str,
    cursor_offset: u32,
    tree_and_analysis: Option<(&SyntaxTree, &AnalysisResult)>,
) -> Option<CodeAction> {
    let utf8 = use_utf8();
    let numbers = crate::lsp::SafeLinePositions::new(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let is_return_line = |i: usize| lines.get(i).is_some_and(|l| return_annotation_body(l).is_some());

    let (cursor_line, _) = numbers.line_col(cursor_offset as usize);
    let cursor_line = cursor_line.0 as usize;

    // Determine a line that belongs to the `@return` run.
    let anchor = if is_return_line(cursor_line) {
        // Cursor is directly on a `@return` comment line.
        cursor_line
    } else {
        // Cursor is inside a function: use the line immediately above its
        // definition, which must be the last line of the `@return` run.
        let (_, analysis) = tree_and_analysis?;
        let func = analysis.ir.local_functions().map(|(_, f)| f)
            .filter(|f| f.def_node.start <= cursor_offset && cursor_offset < f.def_node.end)
            .min_by_key(|f| f.def_node.end - f.def_node.start)?;
        let (func_line, _) = numbers.line_col(func.def_node.start as usize);
        let above = (func_line.0 as usize).checked_sub(1)?;
        if !is_return_line(above) { return None; }
        above
    };

    // Expand to the full contiguous run of `@return` lines around the anchor.
    // The `is_return_line` predicate naturally stops at non-`@return` lines
    // (blank lines, code, `@param`, etc.), so orphaned annotation blocks above
    // will not be swept in.
    let mut first = anchor;
    while first > 0 && is_return_line(first - 1) { first -= 1; }
    let mut last = anchor;
    while is_return_line(last + 1) { last += 1; }

    // Need at least two lines to combine.
    if last == first { return None; }

    // Parse each line into a `type [name]` tuple position.
    let mut positions: Vec<String> = Vec::new();
    for line in &lines[first..=last] {
        let body = return_annotation_body(line)?;
        if body.is_empty() { return None; }
        // Don't flatten forms that aren't simple `type name`: an existing tuple
        // `(...)`, a `@return built` builder return, or a variadic `...T` return
        // (which has special fill-remaining-slots semantics incompatible with
        // tuple shorthand).
        if body.starts_with('(') { return None; }
        let stripped = crate::annotations::strip_return_description(body);
        if stripped.starts_with("...") { return None; }
        if stripped == "built"
            || stripped.starts_with("built ")
            || stripped.starts_with("built:")
        {
            return None;
        }
        let typ = crate::annotations::extract_type_prefix(stripped);
        if typ.is_empty() { return None; }
        let name = stripped[typ.len()..].split_whitespace().next().unwrap_or("");
        if name.is_empty() {
            positions.push(typ.to_string());
        } else {
            positions.push(format!("{} {}", typ, name));
        }
    }

    // Preserve the indentation of the first `@return` line.
    let indent = {
        let l = lines[first];
        &l[..l.len() - l.trim_start().len()]
    };
    let combined = format!("{}---@return ({})", indent, positions.join(", "));

    // Precompute byte offsets of line starts (O(n) once, then O(1) per lookup).
    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(lines.iter().map(|l| l.len() + 1))
        .scan(0usize, |acc, x| { *acc += x; Some(*acc) })
        .collect();
    let start_off = line_offsets[first];
    let end_off = line_offsets[last + 1];

    let edit = lsp_types::TextEdit {
        range: Range {
            start: numbers.lsp_position(start_off, utf8),
            end: numbers.lsp_position(end_off, utf8),
        },
        new_text: format!("{}\n", combined),
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: "Combine into single-line tuple return".to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Quick fix for `undefined-global`: insert `local` before the first assignment to the name.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_add_local_declaration_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
) -> Option<CodeAction> {
    // Parse global name from message: "undefined global 'NAME'"
    let name = diag.message
        .strip_prefix("undefined global '")?
        .strip_suffix('\'')?;

    // Find the first assignment `NAME = ` in the file.
    let (assign_line, assign_col) = find_first_assignment_line(text, name)?;

    let insert_pos = Position { line: assign_line, character: assign_col };
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: "local ".to_string(),
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: format!("Add `local` declaration for `{}`", name),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Quick fix for type-mismatch family: insert `--[[@as TYPE]]` after the expression.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_as_cast_action(
    uri: &lsp_types::Uri,
    diag: &lsp_types::Diagnostic,
) -> Option<CodeAction> {
    let expected_type = extract_expected_type(&diag.message)?;

    // Use long-bracket form if the type contains `]` (e.g. `string[]`).
    let new_text = if expected_type.contains(']') {
        format!(" --[=[@as {}]=]", expected_type)
    } else {
        format!(" --[[@as {}]]", expected_type)
    };

    let insert_pos = diag.range.end;
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text,
    };
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: format!("Cast to `{}`", expected_type),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Quick fix for `missing-fields`: insert all missing required fields with placeholder values.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_fill_missing_fields_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    let msg = diag.message.as_str();

    // Parse field names and class name from the diagnostic message:
    // "missing required field 'NAME' in class 'CLASS'"
    // "missing required fields 'A', 'B' in class 'CLASS'"
    let (fields_raw, class_name) = if let Some(after) = msg.strip_prefix("missing required field '") {
        let (f, r) = after.split_once("' in class '")?;
        (f, r.strip_suffix('\'')?)
    } else if let Some(after) = msg.strip_prefix("missing required fields '") {
        let (f, r) = after.split_once("' in class '")?;
        (f, r.strip_suffix('\'')?)
    } else {
        return None;
    };

    // Field names are joined as "a', 'b', 'c" in the message.
    let field_names: Vec<&str> = fields_raw.split("', '").collect();
    if field_names.is_empty() { return None; }

    // Look up the class table to get field type info for placeholders.
    let class_table_idx = analysis.ir.classes.get(class_name)
        .or_else(|| analysis.ir.ext.classes.get(class_name))?;
    let class_table = analysis.table(*class_table_idx);

    // Convert the diagnostic range to byte offsets.
    // The diagnostic range spans the entire table constructor `{...}`.
    // The range end is exclusive, so the `}` is at end_byte - 1.
    let open_byte = crate::lsp::lsp_position_to_offset(
        text, diag.range.start.line, diag.range.start.character, use_utf8(),
    ) as usize;
    let end_byte = crate::lsp::lsp_position_to_offset(
        text, diag.range.end.line, diag.range.end.character, use_utf8(),
    ) as usize;
    if end_byte == 0 || end_byte > text.len() { return None; }
    let close_byte = end_byte - 1;

    // Determine base indentation from the line that contains the opening `{`.
    let line_start = text[..open_byte].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_prefix = &text[line_start..open_byte];
    let base_indent_len = line_prefix.len() - line_prefix.trim_start().len();
    let base_indent = &text[line_start..line_start + base_indent_len];
    let field_indent = format!("{}    ", base_indent);

    // Check whether the `}` is already on its own line (multiline table).
    // Also capture the position of the newline that precedes the `}` line so we
    // can insert new fields before that newline when brace_on_own_line is true.
    let brace_nl = text[..close_byte].rfind('\n');
    let brace_on_own_line = brace_nl.is_some_and(|nl| {
        text[nl + 1..close_byte].trim().is_empty()
    });

    // Check whether we need a comma after the last existing field.
    let content_before_close = text[open_byte + 1..close_byte].trim_end();
    let needs_leading_comma = !content_before_close.is_empty()
        && !content_before_close.ends_with(',')
        && !content_before_close.ends_with(';');

    // Build the field lines shared by both branches.
    let mut field_lines = String::new();
    if needs_leading_comma { field_lines.push(','); }
    for name in &field_names {
        let placeholder = class_table.fields.get(*name)
            .and_then(|fi| fi.annotation.as_ref())
            .map(placeholder_for_type)
            .unwrap_or("nil");
        field_lines.push_str(&format!("\n{}{} = {},", field_indent, name, placeholder));
    }

    // Choose the insertion byte offset and finalize the text.
    let (insert, insert_byte) = if brace_on_own_line {
        // The `}` is already on its own line.  Insert new fields before the `\n`
        // that starts the `}` line so the `}` stays on its own line.
        let nl = brace_nl.unwrap(); // safe: brace_on_own_line implies brace_nl is Some
        (field_lines, nl)
    } else {
        // Single-line table or `}` on the same line as last field.
        // Insert new fields followed by a newline and the base indent to move `}` down.
        field_lines.push('\n');
        field_lines.push_str(base_indent);
        (field_lines, close_byte)
    };

    let numbers = crate::lsp::SafeLinePositions::new(text);
    let insert_pos = numbers.lsp_position(insert_byte, use_utf8());
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: insert,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);

    let title = if field_names.len() == 1 {
        format!("Fill missing field `{}`", field_names[0])
    } else {
        "Fill all missing fields".to_string()
    };

    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// If `ty` is a nilable `number?` or `string?` (a union of `nil` and exactly
/// one of `Number` or `String`), return the Lua literal to coalesce nil to:
/// `"0"` for numbers, `"\"\""` for strings. Returns `None` for any other shape
/// (e.g. multi-member unions like `string|number|nil`, bare non-union types, or
/// unions whose non-nil member is a table/function/boolean).
pub(super) fn nil_coalesce_default(ty: &ValueType) -> Option<&'static str> {
    let ValueType::Union(members) = ty.strip_opaque() else { return None };
    let mut non_nil = None;
    for m in members {
        if matches!(m, ValueType::Nil) { continue; }
        if non_nil.is_some() { return None; } // more than one non-nil member
        non_nil = Some(m);
    }
    match non_nil? {
        ValueType::Number => Some("0"),
        ValueType::String(_) => Some("\"\""),
        _ => None,
    }
}

/// Quick fix for `invalid-op` on a binary operation with a possibly-nil
/// `number?`/`string?` operand: wrap the nilable operand(s) in `(expr or 0)`
/// (numbers) or `(expr or "")` (strings) so the operation becomes well-typed.
#[allow(clippy::mutable_key_type)]
pub(super) fn make_nil_coalesce_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    let utf8 = use_utf8();
    let diag_start = crate::lsp::lsp_position_to_offset(text, diag.range.start.line, diag.range.start.character, utf8);
    let diag_end = crate::lsp::lsp_position_to_offset(text, diag.range.end.line, diag.range.end.character, utf8);

    // Locate the IR binary-op site whose range matches the diagnostic.
    let site = analysis.ir.binary_op_sites.iter()
        .find(|s| s.expr_start == diag_start && s.expr_end == diag_end)?;
    let crate::types::Expr::BinaryOp { lhs, rhs, .. } = *analysis.ir.expr(site.expr_id) else { return None };

    // Determine the coalesce default for each operand (None if not nilable num/str).
    let lhs_default = analysis.resolve_expr_type(lhs).as_ref().and_then(nil_coalesce_default);
    let rhs_default = analysis.resolve_expr_type(rhs).as_ref().and_then(nil_coalesce_default);
    if lhs_default.is_none() && rhs_default.is_none() { return None; }

    // Find the matching BinaryExpression syntax node to get operand text ranges.
    // The IR lowers operands left-to-right, so term[0] is `lhs`, term[1] is `rhs`.
    let root = crate::syntax::tree::SyntaxNode::new_root(tree);
    let bin_node = root.descendants().find(|n| {
        n.kind() == SyntaxKind::BinaryExpression
            && n.text_range().start().0 == diag_start
            && n.text_range().end().0 == diag_end
    })?;
    let terms = BinaryExpression::cast(bin_node)?.get_terms();
    if terms.len() != 2 { return None; }

    let numbers = crate::lsp::SafeLinePositions::new(text);
    let mut edits = Vec::new();
    for (operand, default) in [(&terms[0], lhs_default), (&terms[1], rhs_default)] {
        let Some(default) = default else { continue };
        let range = operand.syntax().text_range();
        let (op_start, op_end) = (range.start().0, range.end().0);
        let operand_text = text.get(op_start as usize..op_end as usize)?;
        edits.push(lsp_types::TextEdit {
            range: Range {
                start: numbers.lsp_position(op_start as usize, utf8),
                end: numbers.lsp_position(op_end as usize, utf8),
            },
            new_text: format!("({} or {})", operand_text, default),
        });
    }
    if edits.is_empty() { return None; }

    // Sort edits in reverse document order so that applying them sequentially
    // does not shift the byte positions of earlier edits.
    edits.sort_by(|a, b| {
        b.range.start.line.cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });

    let title = if edits.len() == 1 {
        let default = lhs_default.or(rhs_default).unwrap_or("?");
        format!("Provide fallback `or {}` for possibly-nil value", default)
    } else {
        "Provide fallbacks for possibly-nil values".to_string()
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Return a Lua literal placeholder value for the given type.
pub(super) fn placeholder_for_type(vt: &ValueType) -> &'static str {
    match vt {
        ValueType::String(_) => "\"\"",
        ValueType::Number => "0",
        ValueType::Boolean(_) => "false",
        ValueType::Table(_) => "{}",
        // missing-fields skips Function-typed fields, but handle it for completeness.
        ValueType::Function(_) => "function() end",
        ValueType::Union(types) => {
            // Pick the placeholder for the first non-nil member.
            for t in types {
                if !matches!(t, ValueType::Nil) {
                    return placeholder_for_type(t);
                }
            }
            "nil"
        }
        ValueType::OpaqueAlias(_, inner) => placeholder_for_type(inner),
        _ => "nil",
    }
}

/// Extract the expected type from a type-mismatch family diagnostic message.
/// Handles:
///   "expected `TYPE` for parameter 'NAME', got `TYPE`"  (type-mismatch)
///   "expected return type `TYPE`, got `TYPE`"            (return-mismatch)
///   "expected `TYPE` for field 'NAME', got `TYPE`"      (field-type-mismatch)
///   "cannot assign 'TYPE' to 'NAME' (expected 'TYPE')"  (assign-type-mismatch)
pub(super) fn extract_expected_type(msg: &str) -> Option<&str> {
    // assign-type-mismatch: "cannot assign 'X' to 'Y' (expected 'TYPE')"
    if let Some(rest) = msg.strip_prefix("cannot assign ") {
        let expected = rest.rsplit("(expected '").next()?;
        return expected.strip_suffix("')");
    }
    // return-mismatch: "expected return type `TYPE`, got ..."
    if let Some(rest) = msg.strip_prefix("expected return type `") {
        return rest.split('`').next().filter(|s| !s.is_empty());
    }
    // type-mismatch / field-type-mismatch: "expected `TYPE` for ..."
    if let Some(rest) = msg.strip_prefix("expected `") {
        return rest.split('`').next().filter(|s| !s.is_empty());
    }
    None
}

/// Search `text` for the first line where `name` appears as an assignment LHS (`name = `).
/// Skips comment lines and avoids matching inside longer identifiers.
/// Returns `(line_index, column_of_name)` (both 0-based), or `None` if not found.
pub(super) fn find_first_assignment_line(text: &str, name: &str) -> Option<(u32, u32)> {
    for (line_idx, line) in text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") { continue; }
        if let Some(col) = find_assignment_in_line(line, name) {
            return Some((line_idx as u32, col as u32));
        }
    }
    None
}

/// Returns the byte column of `name` on `line` if `name` appears as an assignment LHS.
/// Checks that `name` is not part of a longer identifier and is followed by `=` (not `==`).
pub(super) fn find_assignment_in_line(line: &str, name: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut idx = 0;
    while idx + name.len() <= line.len() {
        // `idx` advances byte-by-byte, so it can land inside a multibyte UTF-8
        // character (e.g. `©`). Slicing there would panic; skip non-boundaries.
        if !line.is_char_boundary(idx) {
            idx += 1;
            continue;
        }
        if line[idx..].starts_with(name) {
            let before_ok = idx == 0 || {
                let b = bytes[idx - 1];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            let after_idx = idx + name.len();
            let after_char_ok = after_idx >= line.len() || {
                let b = bytes[after_idx];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            if before_ok && after_char_ok {
                let after_trimmed = line[after_idx..].trim_start();
                if after_trimmed.starts_with('=') && !after_trimmed.starts_with("==") {
                    return Some(idx);
                }
            }
        }
        idx += 1;
    }
    None
}

#[allow(clippy::mutable_key_type)]
/// If `codes_text` (the part after a `---@diagnostic disable*:` marker) already
/// contains `code`, return a no-op edit; otherwise return an edit that appends
/// `, code` at column `trimmed_len` on `line`.
pub(super) fn merge_diagnostic_codes_edit(
    line: u32,
    trimmed_len: u32,
    codes_text: &str,
    code: &str,
) -> lsp_types::TextEdit {
    let existing: Vec<&str> = codes_text.split(',').map(|s| s.trim()).collect();
    let pos = Position { line, character: trimmed_len };
    if existing.contains(&code) {
        lsp_types::TextEdit {
            range: Range { start: pos, end: pos },
            new_text: String::new(),
        }
    } else {
        lsp_types::TextEdit {
            range: Range { start: pos, end: pos },
            new_text: format!(", {}", code),
        }
    }
}

#[allow(clippy::mutable_key_type)]
pub(super) fn make_disable_line_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let target_line = diag.range.start.line;
    let line_text = text.split('\n')
        .nth(target_line as usize)
        .unwrap_or("");
    let line_trimmed = line_text.trim_end();

    let marker = "---@diagnostic disable-line:";
    let edit = if let Some(pos) = line_trimmed.find(marker) {
        let codes_text = &line_trimmed[pos + marker.len()..];
        merge_diagnostic_codes_edit(target_line, line_trimmed.len() as u32, codes_text, code)
    } else {
        let insert_pos = Position { line: target_line, character: line_text.len() as u32 };
        lsp_types::TextEdit {
            range: Range { start: insert_pos, end: insert_pos },
            new_text: format!(" ---@diagnostic disable-line: {}", code),
        }
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);

    CodeAction {
        title: format!("Disable `{}` on this line", code),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[allow(clippy::mutable_key_type)]
pub(super) fn make_disable_next_line_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let target_line = diag.range.start.line;

    let marker = "---@diagnostic disable-next-line:";

    // Check if the previous line already has a disable-next-line directive
    let edit = if target_line > 0 {
        let prev_line = text.split('\n').nth((target_line - 1) as usize).unwrap_or("");
        let prev_trimmed = prev_line.trim_end();
        let prev_content = prev_trimmed.trim_start();
        if let Some(codes_text) = prev_content.strip_prefix(marker) {
            merge_diagnostic_codes_edit(target_line - 1, prev_trimmed.len() as u32, codes_text, code)
        } else {
            make_new_disable_next_line_edit(text, target_line, code)
        }
    } else {
        make_new_disable_next_line_edit(text, target_line, code)
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);

    CodeAction {
        title: format!("Disable `{}` for this line (above)", code),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(super) fn make_new_disable_next_line_edit(text: &str, target_line: u32, code: &str) -> lsp_types::TextEdit {
    let indent = text.split('\n')
        .nth(target_line as usize)
        .map(|line| {
            let trimmed = line.trim_start();
            &line[..line.len() - trimmed.len()]
        })
        .unwrap_or("");
    let insert_text = format!("{}---@diagnostic disable-next-line: {}\n", indent, code);
    let insert_pos = Position { line: target_line, character: 0 };
    lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: insert_text,
    }
}

#[allow(clippy::mutable_key_type)]
pub(super) fn make_disable_file_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let marker = "---@diagnostic disable:";

    // Search the comment-only prefix of the file for an existing file-level
    // disable directive. Stop at the first line that is neither blank nor a
    // `---` comment so we don't merge into a scoped directive buried inside a
    // function body.
    let mut found: Option<(u32, &str)> = None;
    for (line_idx, line) in text.split('\n').enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(marker) {
            found = Some((line_idx as u32, line));
            break;
        }
        if !trimmed.is_empty() && !trimmed.starts_with("---") && !trimmed.starts_with("#!") {
            break; // first non-comment code line — stop searching
        }
    }

    let edit = if let Some((line_idx, line_text)) = found {
        let line_trimmed = line_text.trim_end();
        let content = line_trimmed.trim_start();
        let codes_text = content.strip_prefix(marker).unwrap_or("");
        merge_diagnostic_codes_edit(line_idx, line_trimmed.len() as u32, codes_text, code)
    } else {
        let insert_pos = Position { line: 0, character: 0 };
        lsp_types::TextEdit {
            range: Range { start: insert_pos, end: insert_pos },
            new_text: format!("---@diagnostic disable: {}\n", code),
        }
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);

    CodeAction {
        title: format!("Disable `{}` for this file", code),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_assignment_in_line_handles_multibyte_chars() {
        // Regression: `idx` advances byte-by-byte and could slice inside a
        // multibyte UTF-8 char (e.g. `©`), panicking on a non-char-boundary.
        let line = "-- Copyright © 2024";
        assert_eq!(find_assignment_in_line(line, "foo"), None);

        // Assignment LHS after a multibyte char on the same line resolves to
        // the correct byte column without panicking.
        let line = "x = \"© sym\"";
        assert_eq!(find_assignment_in_line(line, "x"), Some(0));
    }

    #[test]
    fn find_first_assignment_line_handles_multibyte_chars() {
        let text = "-- Copyright © 2024 Acme\nlocal y\nmyVar = 5\n";
        assert_eq!(find_first_assignment_line(text, "myVar"), Some((2, 0)));

        // A multibyte char earlier on the same line before the assignment match.
        let text = "tbl.field = \"© sym\"\nmyVar = 5";
        assert_eq!(find_first_assignment_line(text, "myVar"), Some((1, 0)));
    }
}
