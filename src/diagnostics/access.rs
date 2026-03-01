use lsp_types::DiagnosticSeverity;
use crate::annotations::Visibility;
use super::WowDiagnostic;

pub const CODE_PRIVATE: &str = "access-private";
pub const CODE_PROTECTED: &str = "access-protected";

pub fn check(diags: &mut Vec<WowDiagnostic>, visibility: Visibility, same_class: bool, is_subclass: bool, field: &str, start: usize, end: usize) {
    match visibility {
        Visibility::Private if !same_class => {
            diags.push(WowDiagnostic {
                code: CODE_PRIVATE,
                message: format!("'{}' is private and cannot be accessed here", field),
                severity: DiagnosticSeverity::WARNING,
                start,
                end,
            });
        }
        Visibility::Protected if !is_subclass => {
            diags.push(WowDiagnostic {
                code: CODE_PROTECTED,
                message: format!("'{}' is protected and cannot be accessed here", field),
                severity: DiagnosticSeverity::WARNING,
                start,
                end,
            });
        }
        _ => {}
    }
}
