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

    /// Untyped-receiver fallback for the `RECV:RegisterEvent("EVENT")` /
    /// `function RECV:EVENT(...)` handler-registration pattern. When `RECV` has no
    /// resolved type (e.g. `local E = unpack(ns)` yields `any`), the typed
    /// `keyof self` path in [`Self::keyof_string_definition_at`] can't fire, so the
    /// event-name string resolves to nothing and the editor is left with a useless
    /// fallback jump. Match a sibling method `function RECV:<value>()` defined on
    /// the *same* receiver symbol and offer it. Purely navigational — no type or
    /// diagnostic effect — and only consulted when the typed string paths (keyof /
    /// event) find nothing, so a typed receiver keeps its authoritative result.
    pub(super) fn sibling_handler_string_definitions_at(&self, tree: &SyntaxTree, offset: u32) -> Vec<DefinitionResult> {
        let root = SyntaxNode::new_root(tree);
        let Some(token) = root.token_at_offset(TextSize::from(offset)).left_biased() else {
            return Vec::new();
        };
        if token.kind() != SyntaxKind::String {
            return Vec::new();
        }
        let Some(value) = keyof_string_value(token.text()).filter(|v| !v.is_empty()) else {
            return Vec::new();
        };

        // The string must be an argument of a colon method call `RECV:METHOD(...)`
        // with a simple single-name receiver (the common addon-object pattern).
        let Some(call_node) = token
            .ancestors()
            .find(|n| matches!(n.kind(), SyntaxKind::FunctionCall | SyntaxKind::MethodCall))
        else {
            return Vec::new();
        };
        let in_args = call_node
            .children()
            .find(|n| n.kind() == SyntaxKind::ArgumentList)
            .is_some_and(|al| {
                al.text_range().start() <= token.text_range().start()
                    && token.text_range().end() <= al.text_range().end()
            });
        if !in_args {
            return Vec::new();
        }
        let Some(ident) = FunctionCall::cast(call_node).and_then(|c| c.identifier()) else {
            return Vec::new();
        };
        if !ident.is_call_to_self() {
            return Vec::new();
        }
        let names = ident.names();
        if names.len() != 2 {
            return Vec::new();
        }
        let receiver = names[0].clone();

        // Resolve the receiver to a symbol at the call site so a same-named local in
        // an unrelated scope can't produce a bogus match.
        let recv_sym = self
            .scope_at_offset(token.text_range().start())
            .and_then(|s| self.get_symbol(&SymbolIdentifier::Name(receiver.clone()), s));

        let mut out: Vec<DefinitionResult> = Vec::new();
        let mut seen: Vec<TextRange> = Vec::new();
        for fd in root.descendants().filter_map(FunctionDefinition::cast) {
            let Some(fi) = fd.identifier() else { continue };
            if !fi.is_call_to_self() {
                continue;
            }
            let fnames = fi.names();
            if fnames.len() != 2 || fnames[0] != receiver || fnames[1] != value {
                continue;
            }
            // Scope-correct: the definition's receiver must resolve to the same
            // symbol as the call's receiver (when the receiver resolves at all).
            if recv_sym.is_some() {
                let def_sym = self
                    .scope_at_offset(fi.syntax().text_range().start())
                    .and_then(|s| self.get_symbol(&SymbolIdentifier::Name(receiver.clone()), s));
                if def_sym != recv_sym {
                    continue;
                }
            }
            let r = fi.syntax().text_range();
            if !seen.contains(&r) {
                seen.push(r);
                out.push(DefinitionResult::Local(r));
            }
        }
        out
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
