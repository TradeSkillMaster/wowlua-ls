use std::collections::HashMap;
use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::types::*;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "duplicate-set-field";

pub(crate) fn run(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
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
                diags.push(WowDiagnostic {
                    code: CODE,
                    message: format!("field '{}' already set on '{}'", site.field_name, class_name),
                    severity: DiagnosticSeverity::WARNING,
                    start: site.ident_start as usize,
                    end: site.ident_end as usize,
                });
            }
            seen.insert(key, i);
        } else {
            seen.insert(key, i);
        }
    }
}
