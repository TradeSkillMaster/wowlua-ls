use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use crate::types::SymbolIdentifier;
use super::{DiagnosticPass, WowDiagnostic};

pub struct UnknownCastVariable;

/// Flags a `@cast` whose target names no variable in scope (usually a typo), so
/// the cast silently does nothing. The targets are recorded where build_ir
/// applies casts (`Ir::unknown_cast_vars`); one `undefined-global` would accept
/// (a global the project declares, or one the file assigns later) is not undefined.
impl DiagnosticPass for UnknownCastVariable {
    fn run(&self, analysis: &AnalysisResult, _tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (name, start, end, scope_idx) in &analysis.ir.unknown_cast_vars {
            if analysis.is_allowed_global(name)
                || analysis.get_symbol_at(&SymbolIdentifier::Name(name.clone()), *scope_idx, *start).is_some()
            {
                continue;
            }
            super::UNKNOWN_CAST_VARIABLE.emit(
                diags,
                format!("undefined variable '{name}' in @cast"),
                *start as usize, *end as usize,
            );
        }
    }
}
