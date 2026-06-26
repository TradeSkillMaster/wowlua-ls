use crate::analysis::AnalysisResult;
use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::SyntaxNode;
use super::{DiagnosticPass, WowDiagnostic, UNKNOWN_CALLBACK_EVENT};

/// `unknown-callback-event` (default-off): flag a string-literal event name passed to
/// a callback-registry consumer method (`:RegisterCallback("…")`, `:TriggerEvent("…")`,
/// …) that the receiving registry never declared via `GenerateCallbackEvents`. Only
/// fires when the registry's event set is *complete* — an unresolved/dynamic event
/// list leaves the set incomplete and suppresses the check, so this never
/// false-positives on a registry whose events couldn't be fully determined.
pub(crate) struct CallbackEvents;

impl DiagnosticPass for CallbackEvents {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if !analysis.has_callback_registries() {
            return;
        }
        for element in SyntaxNode::new_root(tree).descendants_with_tokens() {
            let Some(token) = element.as_token() else { continue };
            if token.kind() != SyntaxKind::String {
                continue;
            }
            let Some(value) = simple_string_value(token.text()) else { continue };
            if value.is_empty() {
                continue;
            }
            let Some(set) = analysis.callback_event_set_for_string(token, tree) else { continue };
            // Suppress when the registry's events couldn't be fully resolved.
            if !set.complete || set.events.contains(&value) {
                continue;
            }
            let range = token.text_range();
            UNKNOWN_CALLBACK_EVENT.emit(
                diags,
                format!("unknown callback event '{value}' — not registered on this callback registry"),
                u32::from(range.start()) as usize,
                u32::from(range.end()) as usize,
            );
        }
    }
}

/// Extract the value of a simple quoted string literal, stripping the quotes. Returns
/// `None` for long-bracket strings (`[[...]]`), whose content shouldn't be treated as
/// a single event name.
fn simple_string_value(raw: &str) -> Option<String> {
    let first = raw.as_bytes().first().copied()?;
    if first != b'"' && first != b'\'' {
        return None;
    }
    Some(raw.trim_matches(|c| c == '"' || c == '\'').to_string())
}
