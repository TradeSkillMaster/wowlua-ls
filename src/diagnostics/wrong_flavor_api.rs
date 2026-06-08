use crate::analysis::AnalysisResult;
use crate::types::{Expr, ScopeIndex, SymbolIdentifier, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct WrongFlavorApi;

impl DiagnosticPass for WrongFlavorApi {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.project_flavors == 0 { return; }
        for expr in analysis.ir.exprs.iter() {
            let Expr::FunctionCall { func: callee, ret_index, call_range, .. } = expr else { continue };
            if *ret_index != 0 { continue; }
            let Some(ValueType::Function(Some(func_idx))) = analysis.resolve_expr_type(*callee) else { continue };
            let call_mask = analysis.func(func_idx).flavors;
            if call_mask == 0 { continue; }
            let scope_idx = analysis.ir.scope_at_offset(call_range.0).unwrap_or(ScopeIndex(0));
            if analysis.ir.and_guarded_call_exprs.contains(callee) {
                continue;
            }
            let mut callee_inner = *callee;
            loop {
                match analysis.ir.expr(callee_inner) {
                    Expr::StripNil(inner) | Expr::StripFalsy(inner) => callee_inner = *inner,
                    Expr::AssignNarrow { inner, .. } => callee_inner = *inner,
                    _ => break,
                }
            }
            if let Expr::SymbolRef(sym_idx, _) = analysis.ir.expr(callee_inner) {
                if !sym_idx.is_external() {
                    continue;
                }
                if analysis.is_symbol_narrowed(*sym_idx, scope_idx)
                    || analysis.is_symbol_falsy_narrowed(*sym_idx, scope_idx) {
                    continue;
                }
            } else if let Some((root_sym, chain)) = analysis.ir.extract_field_chain(callee_inner)
                && root_sym.is_external()
                && (analysis.is_symbol_narrowed(root_sym, scope_idx)
                    || analysis.is_symbol_falsy_narrowed(root_sym, scope_idx)
                    || analysis.is_field_chain_narrowed(root_sym, &chain, scope_idx))
            {
                continue;
            }
            let active = if let Some(&flavor_mask) = analysis.ir.and_guarded_flavor_exprs.get(callee) {
                flavor_mask
            } else {
                analysis.active_flavors_at(scope_idx)
            };
            let missing_mask = crate::flavor::unsupported_flavors(active, call_mask);
            if missing_mask == 0 { continue; }
            let name = analysis.function_name(func_idx).unwrap_or_else(|| {
                if let Some((sym_idx, chain)) = analysis.ir.extract_field_chain(callee_inner) {
                    let sym = analysis.sym(sym_idx);
                    if let SymbolIdentifier::Name(base) = &sym.id {
                        return format!("{}.{}", base, chain.join("."));
                    }
                }
                "?".to_string()
            });
            let missing = crate::flavor::format_flavor_list(missing_mask);
            let available = crate::flavor::format_flavor_list(crate::flavor::effective_mask(call_mask));
            super::WRONG_FLAVOR_API.emit(diags, format!(
                "API '{}' not available in flavor '{}' (available in: {})",
                name, missing, available,
            ), call_range.0 as usize, call_range.1 as usize);
        }
    }
}
