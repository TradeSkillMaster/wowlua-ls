use crate::analysis::AnalysisResult;
use crate::types::{InjectFieldCheck, SymbolIndex, ValueType};
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
            // A field write on a multi-type union receiver (`---@type A|B|C` /
            // `---@param x A|B`) carries only a single `table_idx` — the *first*
            // union member (the deferred resolver picks the first `Table` in the
            // union; see `resolve_deferred_field_assignments`). A field declared on
            // a *later* member would otherwise be reported as injected into the
            // first. Mirror `call_arity`'s `receiver_is_multi_type_union` leniency:
            // a union-typed receiver has no statically known runtime member, so a
            // write of a field declared on any member is legitimate.
            if union_receiver_declares_field(analysis, fa.root_symbol, fa.ident_start, &fa.field_name) { continue; }
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

/// True when the field-write receiver resolves to a genuine multi-type union
/// (2+ table-bearing members) and `field_name` is declared (own or inherited) on
/// ANY of those members. Every `field_assignments` entry's receiver is its root
/// symbol (deep `a.b.c = …` chains are resolved separately and never reach this
/// list), so resolving the root symbol's type at the write site recovers the full
/// union the single recorded `table_idx` collapsed away.
///
/// Suppression uses `class_has_annotated_field` (author `@field` only), not bare
/// field existence, deliberately: the deferred resolver registers the injected
/// field onto the first union member as an *un*annotated field, so an existence
/// check would also see a genuinely-injected control field there and wrongly
/// suppress it. A `@field`-declared member is the honest "this is not an
/// injection" signal — matching the single-member suppression in `check_inject`.
fn union_receiver_declares_field(
    analysis: &AnalysisResult,
    root_symbol: Option<SymbolIndex>,
    ident_start: u32,
    field_name: &str,
) -> bool {
    let Some(sym) = root_symbol else { return false };
    let Some(rt) = analysis.symbol_resolved_type_at(sym, ident_start) else { return false };
    if !matches!(rt, ValueType::Union(_)) { return false; }
    let mut indices = Vec::new();
    super::collect_class_indices(rt, &mut indices);
    // A single-table union (`Foo | string`, `T | nil`) already resolves to that
    // one member and is checked normally; only a real multi-table union is lenient.
    if indices.len() < 2 { return false; }
    indices.iter().any(|&idx| analysis.class_has_annotated_field(idx, field_name))
}
