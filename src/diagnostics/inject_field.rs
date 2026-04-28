use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::types::InjectFieldCheck;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "inject-field";

pub(crate) fn run(analysis: &AnalysisResult, excess: &[InjectFieldCheck], diags: &mut Vec<WowDiagnostic>) {
    for fa in &analysis.ir.field_assignments {
        if fa.is_method_def { continue; }
        if fa.had_annotation_at_build { continue; }
        if fa.in_constructor { continue; }
        if !fa.in_function && fa.table_idx.is_external() { continue; }
        if fa.field_existed_at_build { continue; }
        check_inject(analysis, fa.table_idx, &fa.field_name, fa.scope_idx, fa.ident_start, fa.ident_end, diags);
    }
    for site in excess {
        if site.field_existed_at_build { continue; }
        check_inject(analysis, site.table_idx, &site.field_name, site.scope_idx, site.start, site.end, diags);
    }
}

fn check_inject(
    analysis: &AnalysisResult,
    table_idx: crate::types::TableIndex,
    field_name: &str,
    scope_idx: crate::types::ScopeIndex,
    start: u32,
    end: u32,
    diags: &mut Vec<WowDiagnostic>,
) {
    if analysis.class_has_annotated_field(table_idx, field_name) { return; }
    let table = analysis.table(table_idx);
    let has_annotations = table.fields.values().any(|f| f.annotation.is_some());
    let Some(ref class_name) = table.class_name else { return };
    if !has_annotations { return; }
    let class_name = class_name.clone();
    if let Some(&class_table_idx) = analysis.ir.classes.get(&class_name)
        && analysis.class_has_annotated_field(class_table_idx, field_name) { return; }
    if analysis.suppress_inject_field_on_g(&class_name, field_name, scope_idx) { return; }
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("injecting undefined field '{}' into class '{}'", field_name, class_name),
        severity: DiagnosticSeverity::HINT,
        start: start as usize,
        end: end as usize,
    });
}
