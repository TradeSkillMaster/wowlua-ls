//! Go-to-definition and hover on a `keyof X` string argument: the literal names a
//! field/method of `X`, so it navigates to (and hovers) that member. The target
//! table for each keyof argument is recorded on the `CallResolution` during call
//! resolution (`resolve_call::record_call_resolution`). Completion and the existence
//! check reuse the flattened key union — see `resolve_call` and
//! `diagnostics/generic_constraint_mismatch`.

use super::*;

impl AnalysisResult {
    /// If the String `token` is an argument whose parameter type is (or contains)
    /// `keyof X`, return `X`'s table and the referenced key name.
    fn keyof_string_target(&self, token: &SyntaxToken) -> Option<(TableIndex, String)> {
        let (arg_index, _param_index, call_res) = self.call_resolution_for_arg(token)?;
        let &target = call_res.keyof_arg_targets.get(&arg_index)?;
        let name = keyof_string_value(token.text())?;
        if name.is_empty() {
            return None;
        }
        Some((target, name))
    }

    /// Go-to-definition on a `keyof X` string: jump to the named member of `X`.
    pub(super) fn keyof_string_definition_at(&self, tree: &SyntaxTree, offset: u32) -> Option<DefinitionResult> {
        let token = SyntaxNode::new_root(tree)
            .token_at_offset(TextSize::from(offset))
            .left_biased()?;
        if token.kind() != SyntaxKind::String {
            return None;
        }
        let (target, name) = self.keyof_string_target(&token)?;
        // A local definition (with a source range) takes priority over an external stub.
        if let Some(fi) = self.get_field(target, &name)
            && let Some((start, end)) = fi.def_range
        {
            return Some(DefinitionResult::Local(TextRange::new(
                TextSize::from(start),
                TextSize::from(end),
            )));
        }
        let loc = self.find_external_field_location(target, &name)?;
        Some(DefinitionResult::External(loc.clone()))
    }

    /// Hover on a `keyof X` string: show the named member's type.
    pub(super) fn keyof_string_hover_at(&self, tree: &SyntaxTree, offset: u32) -> Option<HoverResult> {
        let token = SyntaxNode::new_root(tree)
            .token_at_offset(TextSize::from(offset))
            .left_biased()?;
        if token.kind() != SyntaxKind::String {
            return None;
        }
        let (target, name) = self.keyof_string_target(&token)?;
        let fi = self.get_field(target, &name)?;
        // A function member — the common case, an event/handler method — hovers like
        // the main field-access hover path: a function-aware kind label with a real
        // signature, rather than `(field) name: fun(self: X, ...)`.
        if let Some(ValueType::Function(Some(func_idx))) = self.resolve_expr_type(fi.expr) {
            let has_self = self.func(func_idx).args.first().is_some_and(|&s| {
                matches!(&self.sym(s).id, SymbolIdentifier::Name(n) if n == "self")
            });
            let (kind_label, sep) = if has_self { ("method", ":") } else { ("function", ".") };
            let qualified_name = match self.table(target).class_name.as_ref() {
                Some(cn) => format!("{cn}{sep}{name}"),
                None => name.clone(),
            };
            let type_str = format!("({kind_label}) {}",
                self.format_function_decl(func_idx, &qualified_name, has_self, None));
            let doc = self.format_function_doc(func_idx);
            return Some(HoverResult { type_str, doc });
        }
        let type_str = format!("(field) {}: {}", name, self.format_field_type(fi, 0));
        Some(HoverResult { type_str, doc: None })
    }
}

/// Value of a simple quoted string literal, stripping the quotes. Returns `None`
/// for long-bracket strings (`[[...]]`), whose content isn't a member name.
fn keyof_string_value(raw: &str) -> Option<String> {
    let first = raw.as_bytes().first().copied()?;
    if first != b'"' && first != b'\'' {
        return None;
    }
    Some(raw.trim_matches(|c| c == '"' || c == '\'').to_string())
}
