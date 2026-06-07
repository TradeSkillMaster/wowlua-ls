use super::*;

pub(super) fn enclose_range(outer: DefNode, inner: DefNode) -> DefNode {
    DefNode {
        start: outer.start.min(inner.start),
        end: outer.end.max(inner.end),
        node_id: outer.node_id,
    }
}

/// Extract the header text for a control flow block (e.g. "if x > 5", "while running").
/// Walks tokens from the start of the node until the stop keyword (ThenKeyword/DoKeyword).
pub(super) fn extract_block_header(node: &SyntaxNode<'_>, stop_kind: SyntaxKind) -> String {
    let mut parts = Vec::new();
    for item in node.children_with_tokens() {
        match item {
            NodeOrToken::Token(tok) => {
                let k = tok.kind();
                if k == stop_kind || k == SyntaxKind::EndKeyword { break; }
                if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline { continue; }
                parts.push(tok.text().to_string());
            }
            NodeOrToken::Node(child) => {
                // Inline the text of child nodes (e.g. Condition, NameList, ExpressionList)
                for tok in child.descendants_with_tokens() {
                    if let NodeOrToken::Token(tok) = tok {
                        let k = tok.kind();
                        if k == SyntaxKind::Whitespace || k == SyntaxKind::Newline { continue; }
                        parts.push(tok.text().to_string());
                    }
                }
            }
        }
    }
    let header = parts.join(" ");
    if header.len() > 80 {
        // Truncate at a char boundary to avoid panicking on multi-byte UTF-8
        let cut = header.floor_char_boundary(77);
        format!("{}...", &header[..cut])
    } else {
        header
    }
}

/// Create a DefNode covering just the first keyword token of a block node.
pub(super) fn keyword_def_node(node: &SyntaxNode<'_>) -> DefNode {
    if let Some(tok) = node.first_token() {
        let r = tok.text_range();
        DefNode { start: u32::from(r.start()), end: u32::from(r.end()), node_id: None }
    } else {
        DefNode::from_node(*node)
    }
}

/// Check if a node spans multiple lines in the source.
pub(super) fn is_multiline(node: &SyntaxNode<'_>, source: &str) -> bool {
    let range = node.text_range();
    let start = u32::from(range.start()) as usize;
    let end = u32::from(range.end()) as usize;
    source[start..end].contains('\n')
}

/// Create a Block document symbol entry from a node with the given name.
/// Finds the Block child and recursively collects nested symbols.
pub(super) fn make_block_entry(
    analysis: &AnalysisResult,
    node: SyntaxNode<'_>,
    name: String,
    tree: &SyntaxTree,
    func_map: &HashMap<u32, FunctionIndex>,
) -> DocumentSymbolEntry {
    let def_node = DefNode::from_node(node);
    let sel = keyword_def_node(&node);
    let children = node.children()
        .find(|c| c.kind() == SyntaxKind::Block)
        .map(|body| analysis.collect_block_symbols(body, tree, func_map))
        .unwrap_or_default();
    DocumentSymbolEntry {
        name,
        detail: None,
        kind: DocumentSymbolKind::Block,
        range: def_node,
        selection_range: sel,
        children,
        deprecated: false,
    }
}

/// Recursively sort document symbol entries by file position.
pub(super) fn sort_entries_recursive(entries: &mut [DocumentSymbolEntry]) {
    entries.sort_by_key(|s| s.range.start);
    for s in entries.iter_mut() {
        sort_entries_recursive(&mut s.children);
    }
}

/// Recursively extend each entry's range to encompass all children's ranges.
/// This is required for VS Code sticky scroll: the parent range must contain
/// children positions so the editor knows the cursor is "inside" the parent.
pub(super) fn extend_ranges_to_children(entries: &mut [DocumentSymbolEntry]) {
    for entry in entries.iter_mut() {
        extend_ranges_to_children(&mut entry.children);
        for child in &entry.children {
            if child.range.end > entry.range.end {
                entry.range.end = child.range.end;
            }
            if child.range.start < entry.range.start {
                entry.range.start = child.range.start;
            }
        }
    }
}

impl AnalysisResult {
    pub fn document_symbols(&self, tree: &SyntaxTree) -> Vec<DocumentSymbolEntry> {
        let mut class_children: HashMap<String, Vec<DocumentSymbolEntry>> = HashMap::new();
        let mut top_level: Vec<DocumentSymbolEntry> = Vec::new();

        // Build func start offset → FunctionIndex lookup for nested symbol enrichment
        let func_map: HashMap<u32, FunctionIndex> = self.ir.functions.iter().enumerate()
            .filter(|(_, f)| f.def_node != DefNode::DUMMY)
            .map(|(i, f)| (f.def_node.start, FunctionIndex::from(i)))
            .collect();

        // Collect scope-0 symbols (file-level definitions)
        for (id, &sym_idx) in &self.ir.scopes[0].symbols {
            let SymbolIdentifier::Name(name) = id else { continue };
            if sym_idx.is_external() { continue; }
            let sym = self.sym(sym_idx);
            let ver = match sym.versions.first() {
                Some(v) => v,
                None => continue,
            };
            let def = ver.def_node;
            if def == DefNode::DUMMY { continue; }

            match &ver.resolved_type {
                Some(ValueType::Function(Some(func_idx))) => {
                    let func = self.func(*func_idx);
                    let func_def = func.def_node;
                    let base_range = if func_def != DefNode::DUMMY { func_def } else { def };
                    let range = enclose_range(base_range, def);
                    let detail = self.document_symbol_func_detail(*func_idx, name);
                    top_level.push(DocumentSymbolEntry {
                        name: name.clone(),
                        detail: Some(detail),
                        kind: DocumentSymbolKind::Function,
                        range,
                        selection_range: def,
                        children: Vec::new(),
                        deprecated: func.deprecated,
                    });
                }
                Some(ValueType::Table(Some(table_idx))) => {
                    let table = self.table(*table_idx);
                    if let Some(cn) = &table.class_name {
                        // Local @class tables are handled below via ir.classes.
                        // But if the class is external (e.g. Frame), collect methods here.
                        if !self.ir.classes.contains_key(cn) {
                            let children = self.collect_table_func_children(*table_idx);
                            if !children.is_empty() {
                                top_level.push(DocumentSymbolEntry {
                                    name: name.clone(),
                                    detail: None,
                                    kind: DocumentSymbolKind::Variable,
                                    range: def,
                                    selection_range: def,
                                    children,
                                    deprecated: false,
                                });
                            }
                        }
                        continue;
                    }
                    // Non-class table: collect function-typed fields as children
                    let children = self.collect_table_func_children(*table_idx);
                    top_level.push(DocumentSymbolEntry {
                        name: name.clone(),
                        detail: None,
                        kind: DocumentSymbolKind::Variable,
                        range: def,
                        selection_range: def,
                        children,
                        deprecated: false,
                    });
                }
                _ => {
                    let detail = ver.resolved_type.as_ref().map(|vt| self.format_type_depth(vt, 0));
                    top_level.push(DocumentSymbolEntry {
                        name: name.clone(),
                        detail,
                        kind: DocumentSymbolKind::Variable,
                        range: def,
                        selection_range: def,
                        children: Vec::new(),
                        deprecated: false,
                    });
                }
            }
        }

        // Collect class methods from table fields
        for (class_name, &table_idx) in &self.ir.classes {
            if table_idx.is_external() { continue; }
            let children = self.collect_table_func_children(table_idx);
            class_children.entry(class_name.clone()).or_default().extend(children);
        }

        // Emit @class declarations as Class symbols with methods as children
        for (class_name, &table_idx) in &self.ir.classes {
            if table_idx.is_external() { continue; }
            let (range_start, range_end) = if let Some(&(s, e)) = self.ir.class_def_ranges.get(class_name) {
                (s, e)
            } else {
                continue;
            };
            let range = DefNode { start: range_start, end: range_end, node_id: None };
            let children = class_children.remove(class_name).unwrap_or_default();
            top_level.push(DocumentSymbolEntry {
                name: class_name.clone(),
                detail: None,
                kind: DocumentSymbolKind::Class,
                range,
                selection_range: range,
                children,
                deprecated: false,
            });
        }

        // Any methods whose class wasn't found as a local @class go top-level
        for (_class, methods) in class_children {
            top_level.extend(methods);
        }

        // Enrich function/method entries with nested block children
        self.enrich_with_nested_symbols(&mut top_level, tree, &func_map);

        // Extend parent ranges to encompass all children (required for sticky scroll)
        extend_ranges_to_children(&mut top_level);

        // Sort by position in file (recursively)
        sort_entries_recursive(&mut top_level);

        top_level
    }

    /// Recursively walk function/method entries and add nested blocks as children.
    pub(super) fn enrich_with_nested_symbols(
        &self,
        entries: &mut [DocumentSymbolEntry],
        tree: &SyntaxTree,
        func_map: &HashMap<u32, FunctionIndex>,
    ) {
        for entry in entries.iter_mut() {
            if matches!(entry.kind, DocumentSymbolKind::Function | DocumentSymbolKind::Method)
                && let Some(node_id) = entry.range.node_id
            {
                // node_id points to the FunctionDefinition AST node; find its Block child
                let func_node = SyntaxNode { tree, id: node_id };
                if let Some(block) = func_node.children().find(|c| c.kind() == SyntaxKind::Block) {
                    let nested = self.collect_block_symbols(block, tree, func_map);
                    entry.children.extend(nested);
                }
            }
            // Recurse into existing children (e.g. class methods, table methods)
            self.enrich_with_nested_symbols(&mut entry.children, tree, func_map);
        }
    }

    /// Walk a Block AST node and collect nested document symbol entries for
    /// functions and control flow blocks.
    pub(super) fn collect_block_symbols(
        &self,
        block: SyntaxNode<'_>,
        tree: &SyntaxTree,
        func_map: &HashMap<u32, FunctionIndex>,
    ) -> Vec<DocumentSymbolEntry> {
        let mut entries = Vec::new();
        let source = tree.source();

        for child in block.children() {
            let kind = child.kind();
            match kind {
                SyntaxKind::FunctionDefinition => {
                    if !is_multiline(&child, source) { continue; }
                    let start = u32::from(child.text_range().start());

                    let Some(func_def) = FunctionDefinition::cast(child) else { continue };
                    let name = func_def.name().unwrap_or_else(|| "function".to_string());
                    let detail = func_map.get(&start)
                        .map(|&idx| self.document_symbol_func_detail(idx, &name));
                    let is_method = func_map.get(&start).is_some_and(|&idx| {
                        let f = self.func(idx);
                        f.args.first().is_some_and(|&sym_idx| {
                            matches!(&self.sym(sym_idx).id, SymbolIdentifier::Name(n) if n == "self")
                        })
                    });
                    let deprecated = func_map.get(&start)
                        .is_some_and(|&idx| self.func(idx).deprecated);
                    let def_node = DefNode::from_node(child);

                    let mut entry = DocumentSymbolEntry {
                        name,
                        detail,
                        kind: if is_method { DocumentSymbolKind::Method } else { DocumentSymbolKind::Function },
                        range: def_node,
                        selection_range: def_node,
                        children: Vec::new(),
                        deprecated,
                    };

                    // Recurse into function body
                    if let Some(body) = func_def.block() {
                        entry.children = self.collect_block_symbols(body.syntax(), tree, func_map);
                    }
                    entries.push(entry);
                }
                SyntaxKind::IfChain => {
                    let Some(if_chain) = crate::ast::IfChain::cast(child) else { continue };
                    for branch in if_chain.if_branches() {
                        let br = branch.syntax();
                        if !is_multiline(&br, source) { continue; }
                        let name = extract_block_header(&br, SyntaxKind::ThenKeyword);
                        entries.push(make_block_entry(self, br, name, tree, func_map));
                    }
                    if let Some(else_branch) = if_chain.else_branch() {
                        let eb = else_branch.syntax();
                        if !is_multiline(&eb, source) { continue; }
                        entries.push(make_block_entry(self, eb, "else".to_string(), tree, func_map));
                    }
                }
                SyntaxKind::WhileLoop | SyntaxKind::ForCountLoop | SyntaxKind::ForInLoop => {
                    if !is_multiline(&child, source) { continue; }
                    let name = extract_block_header(&child, SyntaxKind::DoKeyword);
                    entries.push(make_block_entry(self, child, name, tree, func_map));
                }
                SyntaxKind::DoBlock => {
                    if !is_multiline(&child, source) { continue; }
                    entries.push(make_block_entry(self, child, "do".to_string(), tree, func_map));
                }
                SyntaxKind::RepeatUntilLoop => {
                    if !is_multiline(&child, source) { continue; }
                    entries.push(make_block_entry(self, child, "repeat".to_string(), tree, func_map));
                }
                _ => {}
            }
        }
        entries
    }

    pub(super) fn field_func_idx(&self, field: &FieldInfo) -> Option<FunctionIndex> {
        if let Some(Some(ValueType::Function(Some(idx)))) = self.resolved_expr_cache.get(field.expr.val()) {
            return Some(*idx);
        }
        if let Some(ValueType::Function(Some(idx))) = &field.annotation {
            return Some(*idx);
        }
        if let Expr::FunctionDef(idx) = self.expr(field.expr) {
            return Some(*idx);
        }
        None
    }

    pub(super) fn collect_table_func_children(&self, table_idx: TableIndex) -> Vec<DocumentSymbolEntry> {
        let table = self.table(table_idx);
        let mut children = Vec::new();
        for (field_name, field) in &table.fields {
            let func_idx = self.field_func_idx(field);
            let Some(func_idx) = func_idx else { continue };
            let func = self.func(func_idx);
            let func_def = func.def_node;
            if func_def == DefNode::DUMMY { continue; }
            let has_self_param = func.args.first()
                .is_some_and(|&sym_idx| matches!(&self.sym(sym_idx).id, SymbolIdentifier::Name(n) if n == "self"));
            let kind = if has_self_param { DocumentSymbolKind::Method } else { DocumentSymbolKind::Function };
            let detail = self.document_symbol_func_detail(func_idx, field_name);
            let sel_range = match field.def_range {
                Some((s, e)) => DefNode { start: s, end: e, node_id: None },
                None => func_def,
            };
            children.push(DocumentSymbolEntry {
                name: field_name.clone(),
                detail: Some(detail),
                kind,
                range: enclose_range(func_def, sel_range),
                selection_range: sel_range,
                children: Vec::new(),
                deprecated: func.deprecated,
            });
        }
        children
    }

    pub(super) fn document_symbol_func_detail(&self, func_idx: FunctionIndex, display_name: &str) -> String {
        let func = self.func(func_idx);
        let args: Vec<String> = func.args.iter().enumerate()
            .filter(|&(_, &sym_idx)| {
                if let SymbolIdentifier::Name(ref n) = self.sym(sym_idx).id {
                    return n != "self";
                }
                true
            })
            .map(|(i, &sym_idx)| {
                let param_name = match &self.sym(sym_idx).id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "?".to_string(),
                };
                let optional = func.param_optional.get(i).copied().unwrap_or(false);
                let ann_has_nil = func.param_annotations.get(i)
                    .is_some_and(crate::annotations::annotation_type_is_nullable);
                let suffix = if optional && !ann_has_nil { "?" } else { "" };
                let type_str = self.param_annotation_text(func, i)
                    .or_else(|| {
                        self.sym(sym_idx).versions.first()
                            .and_then(|v| v.resolved_type.as_ref())
                            .map(|rt| {
                                let display_type = if optional && !ann_has_nil { rt.strip_nil() } else { rt.clone() };
                                self.format_type_depth(&display_type, 1)
                            })
                    });
                match type_str {
                    Some(t) => format!("{}{}: {}", param_name, suffix, t),
                    None => format!("{}{}", param_name, suffix),
                }
            }).collect();
        let mut all_args = args;
        if func.is_vararg {
            let vararg_str = match &func.vararg_annotation {
                Some(ann) => format_vararg_param(ann),
                None => "...".to_string(),
            };
            all_args.push(vararg_str);
        }
        let rets: Vec<String> = func.return_annotations.iter()
            .map(|vt| self.format_type_depth(vt, 1))
            .collect();
        if rets.is_empty() {
            format!("function {}({})", display_name, all_args.join(", "))
        } else {
            format!("function {}({}): {}", display_name, all_args.join(", "), join_returns(&rets))
        }
    }
}
