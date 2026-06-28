use crate::analysis::AnalysisResult;
use crate::types::InjectFieldCheck;
use super::{DiagnosticPass, WowDiagnostic};

pub struct InjectField;

impl DiagnosticPass for InjectField {
    fn run_inject(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, excess_inject: &mut Vec<InjectFieldCheck>, diags: &mut Vec<WowDiagnostic>) {
        for fa in &analysis.ir.field_assignments {
            if fa.is_method_def { continue; }
            if fa.had_annotation_at_build { continue; }
            if fa.in_constructor { continue; }
            if !fa.in_function && fa.table_idx.is_external() { continue; }
            if fa.field_existed_at_build { continue; }
            // @class-annotated variables are class definitions — field assignments
            // on them define new class fields, not inject foreign fields.
            if fa.root_symbol.is_some_and(|s| analysis.ir.class_def_symbols.contains(&s)) { continue; }
            check_inject(analysis, fa.table_idx, &fa.field_name, fa.scope_idx, fa.ident_start, fa.ident_end, diags);
        }
        // Excess structural fields from type-mismatch pipeline: these come from
        // table literals passed as arguments, not from direct field assignments on
        // named variables, so class_def_symbols does not apply.
        for site in excess_inject.iter() {
            if site.field_existed_at_build { continue; }
            check_inject(analysis, site.table_idx, &site.field_name, site.scope_idx, site.start, site.end, diags);
        }
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
    let Some(ref class_name) = table.class_name else { return };
    // Determine whether the class has an author-declared field contract.
    // A class has a field contract when:
    // 1. It has explicit @field annotations in the source file (has_source_fields), or
    // 2. It has constructor methods that define fields (@constructor), or
    // 3. A cross-file class table has fields with explicit type annotations
    //    (annotation_type_raw, not annotation — the latter can be set from function
    //    return inference like CreateFrame which doesn't represent an author contract).
    let has_field_contract = if table_idx.is_external() {
        table.fields.values().any(|f| f.annotation.is_some())
    } else {
        table.has_source_fields
        || !table.constructors.is_empty()
        || analysis.ir.classes.get(class_name.as_str())
            .filter(|&&idx| idx != table_idx)
            .is_some_and(|&idx| analysis.table(idx).fields.values().any(|f| f.annotation_type_raw.is_some()))
    };
    if !has_field_contract { return; }
    let class_name = class_name.clone();
    if let Some(&class_table_idx) = analysis.ir.classes.get(&class_name)
        && analysis.class_has_annotated_field(class_table_idx, field_name) { return; }
    if analysis.suppress_inject_field_on_g(&class_name, field_name, scope_idx) { return; }
    super::INJECT_FIELD.emit(
        diags,
        format!("injecting undefined field '{}' into class '{}'", field_name, class_name),
        start as usize,
        end as usize,
    );
}
