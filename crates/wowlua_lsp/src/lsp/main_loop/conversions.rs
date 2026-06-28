use super::*;

pub(super) fn uri_to_path(uri: &lsp_types::Uri, workspace_root: &Option<PathBuf>) -> Option<PathBuf> {
    let path = uri_to_abs_path(uri)?;
    let root = workspace_root.as_ref()?;
    if path.starts_with(root) { Some(path) } else { None }
}

pub(super) fn defnode_to_range(def: crate::types::DefNode, numbers: &crate::lsp::SafeLinePositions) -> Range {
    numbers.lsp_range(def.start as usize, def.end as usize, use_utf8())
}

pub(super) fn entry_to_document_symbol(
    entry: crate::types::DocumentSymbolEntry,
    numbers: &crate::lsp::SafeLinePositions,
) -> DocumentSymbol {
    let kind = match entry.kind {
        DocumentSymbolKind::Function => SymbolKind::FUNCTION,
        DocumentSymbolKind::Method => SymbolKind::METHOD,
        DocumentSymbolKind::Class => SymbolKind::CLASS,
        DocumentSymbolKind::Variable => SymbolKind::VARIABLE,
        DocumentSymbolKind::Block => SymbolKind::STRUCT,
    };
    let range = defnode_to_range(entry.range, numbers);
    let selection_range = defnode_to_range(entry.selection_range, numbers);
    let children = if entry.children.is_empty() {
        None
    } else {
        Some(entry.children.into_iter()
            .map(|c| entry_to_document_symbol(c, numbers))
            .collect())
    };
    let tags = if entry.deprecated {
        Some(vec![SymbolTag::DEPRECATED])
    } else {
        None
    };
    #[allow(deprecated)]
    DocumentSymbol {
        name: entry.name,
        detail: entry.detail,
        kind,
        tags,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

/// Permissive URI → path conversion (unlike `uri_to_path`, doesn't require the path
/// to be inside the workspace root). Used for dedupe only.
pub(super) fn uri_to_path_lax(uri: &lsp_types::Uri) -> Option<PathBuf> {
    uri_to_abs_path(uri)
}

pub(super) fn pos_from_numbers(numbers: &crate::lsp::SafeLinePositions, offset: u32) -> Position {
    numbers.lsp_position(offset as usize, use_utf8())
}

/// Check if a URI points to a file that should be ignored by project config.
pub(super) fn is_ignored_uri(uri: &lsp_types::Uri, configs: &crate::config::ProjectConfigs) -> bool {
    uri_to_abs_path(uri).is_some_and(|p| configs.is_ignored(&p))
}

/// Handle a `textDocument/diagnostic` pull request (LSP 3.17).
/// Returns diagnostics for one document, using cached analysis when available.
pub(super) fn is_toc_extension(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("toc"))
}

pub(super) fn convert_toc_diagnostics(
    toc_diags: Vec<crate::toc::diagnostics::TocDiagnostic>,
    text: &str,
) -> Vec<lsp_types::Diagnostic> {
    let numbers = crate::lsp::SafeLinePositions::new(text);
    toc_diags.into_iter().map(|d| {
        let severity = match d.severity {
            crate::toc::diagnostics::TocSeverity::Error => lsp_types::DiagnosticSeverity::ERROR,
            crate::toc::diagnostics::TocSeverity::Warning => lsp_types::DiagnosticSeverity::WARNING,
            crate::toc::diagnostics::TocSeverity::Hint => lsp_types::DiagnosticSeverity::HINT,
        };
        lsp_types::Diagnostic {
            range: numbers.lsp_range(d.start as usize, d.end as usize, use_utf8()),
            severity: Some(severity),
            code: Some(lsp_types::NumberOrString::String(d.code.to_string())),
            source: Some("wowlua_ls".to_string()),
            message: d.message,
            ..Default::default()
        }
    }).collect()
}
