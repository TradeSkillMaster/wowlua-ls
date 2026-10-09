use crate::collections::HashSet;
use crate::analysis::{AnalysisResult, NilGuard};
use crate::types::{ExprId, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

fn is_nullable(vt: &ValueType) -> bool {
    match vt {
        ValueType::Union(types) => types.contains(&ValueType::Nil),
        // Bare Nil almost always means unresolved type info (e.g. multi-return
        // position beyond the known return count), not a genuinely nil-typed
        // value. The rare true-nil case is a guaranteed runtime crash anyway.
        ValueType::Nil => false,
        _ => false,
    }
}

/// Check if the key expression is narrowed (nil guard present).
fn key_nil_suppressed(analysis: &AnalysisResult, key_expr: ExprId, start: u32) -> bool {
    let Some(scope_idx) = analysis.scope_at_offset(start) else { return true };
    analysis.nil_guard_at(key_expr, scope_idx, start) != NilGuard::Unguarded
}

pub struct NilIndex;

impl DiagnosticPass for NilIndex {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let mut seen = HashSet::default();
        for &(key_expr, start, end) in &analysis.ir.bracket_index_sites {
            if !seen.insert((start, end)) { continue; }
            let Some(vt) = analysis.resolve_expr_type(key_expr) else { continue };
            if !is_nullable(&vt) { continue; }
            // Skip when the type contains unresolved generic type variables (e.g. K?
            // from `next(bare_table)`). These leak from the query-time resolver's
            // FunctionRet fallback when phase-2 couldn't bind the generics.
            if vt.contains_type_variable() { continue; }
            if key_nil_suppressed(analysis, key_expr, start) { continue; }
            let type_str = analysis.format_value_type_depth(&vt, 0);
            super::NIL_INDEX.emit(
                diags,
                format!("possibly-nil table key of type `{}`", type_str),
                start as usize,
                end as usize,
            );
        }
    }
}
