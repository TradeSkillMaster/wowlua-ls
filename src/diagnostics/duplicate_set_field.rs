use std::collections::HashMap;
use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, RelatedInfo, WowDiagnostic};

pub(crate) struct DuplicateSetField;

fn rhs_reads_field(analysis: &AnalysisResult, expr_id: ExprId, root_name: &str, field_name: &str, depth: u8) -> bool {
    if depth > 20 { return false; }
    match analysis.expr(expr_id) {
        Expr::FieldAccess { table, field, .. } => {
            if field == field_name
                && let Expr::SymbolRef(sym_idx, _) = analysis.expr(*table)
                && matches!(&analysis.sym(*sym_idx).id, SymbolIdentifier::Name(n) if n == root_name)
            {
                return true;
            }
            rhs_reads_field(analysis, *table, root_name, field_name, depth + 1)
        }
        Expr::FunctionCall { func, args, .. } => {
            rhs_reads_field(analysis, *func, root_name, field_name, depth + 1)
                || args.iter().any(|a| rhs_reads_field(analysis, *a, root_name, field_name, depth + 1))
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            rhs_reads_field(analysis, *lhs, root_name, field_name, depth + 1)
                || rhs_reads_field(analysis, *rhs, root_name, field_name, depth + 1)
        }
        Expr::UnaryOp { operand, .. } => rhs_reads_field(analysis, *operand, root_name, field_name, depth + 1),
        Expr::BracketIndex { table, key, .. } => {
            rhs_reads_field(analysis, *table, root_name, field_name, depth + 1)
                || rhs_reads_field(analysis, *key, root_name, field_name, depth + 1)
        }
        Expr::Grouped(inner) | Expr::StripNil(inner) | Expr::StripFalsy(inner)
        | Expr::CastAdd(inner, _) | Expr::CastRemove(inner, _) | Expr::TypeFilter(inner, _) => {
            rhs_reads_field(analysis, *inner, root_name, field_name, depth + 1)
        }
        Expr::OverloadNarrow { inner, .. } => rhs_reads_field(analysis, *inner, root_name, field_name, depth + 1),
        Expr::BranchMerge(exprs) => exprs.iter().any(|e| rhs_reads_field(analysis, *e, root_name, field_name, depth + 1)),
        _ => false,
    }
}

impl DiagnosticPass for DuplicateSetField {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let sites = &analysis.ir.field_assignments;
        let mut seen: HashMap<(TableIndex, &str, &str, ScopeIndex), usize> = HashMap::new();
        for (i, site) in sites.iter().enumerate() {
            let Some(class_name) = &analysis.table(site.table_idx).class_name else { continue };
            let key = (site.table_idx, site.root_name.as_str(), site.field_name.as_str(), site.scope_idx);
            if let Some(&first_idx) = seen.get(&key) {
                let has_intervening = sites[first_idx + 1..i].iter().any(|s| {
                    s.table_idx == site.table_idx && s.scope_idx == site.scope_idx && s.field_name != site.field_name
                });
                let Some(stmt_gap) = (site.block_stmt_index as usize).checked_sub(sites[first_idx].block_stmt_index as usize) else {
                    seen.insert(key, i);
                    continue;
                };
                let intervening_in_scope = sites[first_idx + 1..i].iter()
                    .filter(|s| s.scope_idx == site.scope_idx)
                    .count();
                let all_intervening_are_field_assigns = stmt_gap == intervening_in_scope + 1;
                if !has_intervening && all_intervening_are_field_assigns {
                    if rhs_reads_field(analysis, site.actual_expr, &site.root_name, &site.field_name, 0) {
                        seen.insert(key, i);
                        continue;
                    }
                    let first = &sites[first_idx];
                    let related = vec![RelatedInfo {
                        file_path: None,
                        start: first.ident_start as usize,
                        end: first.ident_end as usize,
                        message: "First occurrence here".to_string(),
                    }];
                    super::DUPLICATE_SET_FIELD.emit_with_related(diags, format!("field '{}' already set on '{}'", site.field_name, class_name), site.ident_start as usize, site.ident_end as usize, related);
                }
                seen.insert(key, i);
            } else {
                seen.insert(key, i);
            }
        }
    }
}
