use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, RelatedInfo, WowDiagnostic};

pub(crate) struct FieldTypeMismatch;

/// A scan-fabricated bare `table` placeholder that is never an authoritative
/// field type: either `Table(None)` (a parked bare `table` with no `TableInfo`
/// to flag), or a `Table(Some(idx))` whose table carries [`TableInfo::placeholder`]
/// — the existing "workspace-scan placeholder, parked because a call's return
/// type couldn't be resolved" marker, set at the addon-namespace parking sites
/// and consumed symmetrically by `is_table_subtype_impl`. The self-field/global
/// scan parks one of these on a field whose only writer it couldn't type (a
/// chained/builder call, a `select(...)` with an arg-nested call, etc.), so it
/// carries no shape and no author annotation and must never override an actual
/// write type. A scan-inferred table that *does* carry a shape (an unflagged
/// table with real fields) is not a placeholder and is handled by the
/// structural-table path below.
fn is_bare_scan_placeholder(analysis: &AnalysisResult, vt: &ValueType) -> bool {
    match vt {
        ValueType::Table(None) => true,
        ValueType::Table(Some(idx)) => analysis.table(*idx).placeholder,
        _ => false,
    }
}

/// Build related info pointing to a field declaration if it has a source range
/// and belongs to a local (non-external) table.
fn field_declared_here(table_idx: TableIndex, field_info: &FieldInfo) -> Vec<RelatedInfo> {
    if table_idx.is_external() { return Vec::new(); }
    let Some((start, end)) = field_info.def_range else { return Vec::new(); };
    vec![RelatedInfo {
        file_path: None,
        start: start as usize,
        end: end as usize,
        message: "Field declared here".to_string(),
    }]
}

impl DiagnosticPass for FieldTypeMismatch {
    fn run_inject(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, excess_inject: &mut Vec<InjectFieldCheck>, diags: &mut Vec<WowDiagnostic>) {
        for fa in &analysis.ir.field_assignments {
            if !fa.had_annotation_at_build { continue; }
            let Some(field_info) = analysis.get_field(fa.table_idx, &fa.field_name) else { continue };
            let Some(ref expected) = field_info.annotation else { continue };
            let Some(actual) = analysis.resolve_expr_type(fa.actual_expr) else { continue };
            // A `from_scan` field carries an inferred type from the workspace
            // self-field/global scan, not an author annotation — so a `table` it
            // fabricated is never authoritative for an actual write type.
            if field_info.from_scan {
                // Bare `table` placeholder (see `is_bare_scan_placeholder`). The
                // scan couldn't type the field's only writer — a chained/builder
                // call, or `self.x = select(3, UnitClass("player"))` whose
                // arg-nested call makes the scan treat the assignment as
                // unresolvable — and parked a bare `table`. It is a "type unknown"
                // marker with no shape, so it must never override the actual write
                // type: skip for ANY actual, including the very scalar/nil write
                // that produced the placeholder.
                if is_bare_scan_placeholder(analysis, expected) {
                    continue;
                }
                // Scan-inferred *structural* table (a constructor shape captured
                // from the field's initial assignment). Later field additions can
                // grow the underlying table, making the constructor appear to have
                // fewer fields than expected — a false positive. Skip only when the
                // actual is also a table (the incremental-build scenario);
                // genuinely wrong types (e.g. a string where a table shape was
                // expected) still fire the diagnostic.
                if matches!(expected, ValueType::Table(Some(idx)) if analysis.table(*idx).class_name.is_none())
                    && matches!(actual, ValueType::Table(_))
                {
                    continue;
                }
            }
            // A scanned self-field whose type was augmented by a `@narrows-arg`
            // mixin (`self.X = CreateFrame(...); Mixin(self.X, M)` → field type
            // `Frame & M`) is intentionally assigned only its base value at the
            // assignment site — the following `Mixin` call completes the type.
            // Don't flag that base assignment against the fuller intersection
            // when the assigned value matches one of the intersection members.
            // (Gated on `from_scan` so an explicit `@field T & U` stays strict.)
            if field_info.from_scan
                && let ValueType::Intersection(members) = expected
                && members.iter().any(|m| actual.is_assignable_to(m) || analysis.is_table_subtype(&actual, m))
            {
                continue;
            }
            if fa.lateinit {
                if matches!(actual, ValueType::Nil) { continue; }
                let stripped = actual.strip_nil();
                if stripped.is_assignable_to(expected) { continue; }
                if analysis.is_table_subtype(&stripped, expected) { continue; }
            }
            if actual.is_assignable_to(expected) {
                continue;
            }
            if analysis.is_table_subtype(&actual, expected) {
                analysis.check_excess_structural_fields(excess_inject, &actual, expected, fa.expr_start as usize, fa.expr_end as usize);
                continue;
            }
            let expected_str = analysis.format_value_type_depth(expected, 1);
            let actual_str = analysis.format_value_type_depth(&actual, 1);
            let mut message = format!("expected `{}` for field '{}', got `{}`", expected_str, fa.field_name, actual_str);
            super::append_structural_mismatch_suffix(&mut message, analysis, &actual, expected);
            let related = field_declared_here(fa.table_idx, field_info);
            super::FIELD_TYPE_MISMATCH.emit_with_related(
                diags,
                message,
                fa.expr_start as usize,
                fa.expr_end as usize,
                related,
            );
        }

        // Check constructor fields against @class @field annotations.
        // Table constructor fields don't create FieldAssignment records, so the
        // loop above misses them. Walk symbols to find class-typed variables with
        // a constructor RHS and compare each constructor field's type against the
        // class's @field annotation.
        // Only checks version 0 (initial assignment). Reassignments like
        // `obj = { x = "wrong" }` on a class-typed variable are not caught.
        for (_, sym) in analysis.local_symbols() {
            let ver = &sym.versions[0];
            let Some(original_expr) = ver.original_type_source else { continue };
            let Some(type_source) = ver.type_source else { continue };

            let Expr::Literal(ValueType::Table(Some(class_table_idx))) = analysis.ir.expr(type_source) else { continue };
            let class_table = analysis.table(*class_table_idx);
            if class_table.class_name.is_none() { continue; }

            let Some(rhs_table_idx) = analysis.ir.find_table_index(original_expr) else { continue };
            let rhs_table = analysis.ir.table(rhs_table_idx);
            if rhs_table.fields.is_empty() { continue; }

            for (field_name, rhs_field) in &rhs_table.fields {
                if rhs_field.annotation.is_some() { continue; }
                let Some(class_field) = class_table.fields.get(field_name) else { continue };
                let Some(ref expected) = class_field.annotation else { continue };
                check_constructor_field(
                    analysis,
                    ConstructorField {
                        field_name,
                        field_expr: rhs_field.expr,
                        def_range: rhs_field.def_range,
                        fallback_table_idx: rhs_table_idx,
                    },
                    ExpectedField {
                        expected,
                        lateinit: class_field.lateinit,
                        class_table_idx: *class_table_idx,
                        class_field_def_range: class_field.def_range,
                    },
                    excess_inject, diags,
                );
            }
        }

        // Phase 3: Check array element constructor fields against @type T[]
        // element annotations. When a variable has @type Shape[] (or
        // @type ClassName[]), each positional element in the table constructor
        // is checked field-by-field against the element type's fields.
        // Only checks version 0 (initial assignment), like Phase 2 above.
        for (&sym_idx, annotation_type) in &analysis.ir.symbol_type_annotations {
            let Some(elem_table_idx) = extract_array_element_table(analysis, annotation_type) else { continue };
            if analysis.table(elem_table_idx).fields.is_empty() { continue; }

            let sym = analysis.sym(sym_idx);
            let ver = &sym.versions[0];
            let Some(original_expr) = ver.original_type_source else { continue };
            let Some(rhs_table_idx) = analysis.ir.find_table_index(original_expr) else { continue };
            let rhs_array_fields = &analysis.ir.table(rhs_table_idx).array_fields;
            if rhs_array_fields.is_empty() { continue; }

            // Clone to release the borrow on analysis.ir before the inner loop
            // calls find_table_index/table/get_field/resolve_expr_type.
            let rhs_array_fields = rhs_array_fields.clone();

            for elem_expr_id in &rhs_array_fields {
                let Some(inner_table_idx) = analysis.ir.find_table_index(*elem_expr_id) else { continue };
                // Collect field data upfront to release the borrow on ir.tables
                // before calling analysis methods in check_constructor_field.
                let inner_fields: Vec<_> = analysis.ir.table(inner_table_idx).fields.iter()
                    .filter(|(_, f)| f.annotation.is_none())
                    .map(|(name, f)| (name.clone(), f.expr, f.def_range))
                    .collect();

                for (field_name, field_expr, def_range) in &inner_fields {
                    let Some(expected_field) = analysis.get_field(elem_table_idx, field_name) else { continue };
                    let Some(ref expected) = expected_field.annotation else { continue };
                    check_constructor_field(
                        analysis,
                        ConstructorField {
                            field_name,
                            field_expr: *field_expr,
                            def_range: *def_range,
                            fallback_table_idx: inner_table_idx,
                        },
                        ExpectedField {
                            expected,
                            lateinit: expected_field.lateinit,
                            class_table_idx: elem_table_idx,
                            class_field_def_range: expected_field.def_range,
                        },
                        excess_inject, diags,
                    );
                }
            }
        }
    }
}

/// A constructor field whose actual value type is being checked.
/// `fallback_table_idx` supplies a source range when the field has no `def_range`.
struct ConstructorField<'a> {
    field_name: &'a str,
    field_expr: ExprId,
    def_range: Option<(u32, u32)>,
    fallback_table_idx: TableIndex,
}

/// The declared `@field` annotation a constructor field is checked against.
/// `class_table_idx`/`class_field_def_range` locate the annotation for related info.
struct ExpectedField<'a> {
    expected: &'a ValueType,
    lateinit: bool,
    class_table_idx: TableIndex,
    class_field_def_range: Option<(u32, u32)>,
}

/// Check a single constructor field's actual type against an expected annotation type.
/// Shared by Phase 2 (class constructor fields) and Phase 3 (array element fields).
fn check_constructor_field(
    analysis: &AnalysisResult,
    field: ConstructorField,
    expected: ExpectedField,
    excess_inject: &mut Vec<InjectFieldCheck>,
    diags: &mut Vec<WowDiagnostic>,
) {
    let ConstructorField { field_name, field_expr, def_range, fallback_table_idx } = field;
    let ExpectedField { expected, lateinit, class_table_idx, class_field_def_range } = expected;
    let Some(actual) = analysis.resolve_expr_type(field_expr) else { return };
    if matches!(actual, ValueType::Nil) { return; }

    if lateinit {
        let stripped = actual.strip_nil();
        if stripped.is_assignable_to(expected) { return; }
        if analysis.is_table_subtype(&stripped, expected) { return; }
    }

    if actual.is_assignable_to(expected) { return; }

    let Some((start, end)) = def_range.or_else(|| {
        analysis.ir.table_ranges.iter()
            .find(|(_, idx)| **idx == fallback_table_idx)
            .map(|(&(s, e), _)| (s, e))
    }) else { return };

    if analysis.is_table_subtype(&actual, expected) {
        analysis.check_excess_structural_fields(
            excess_inject, &actual, expected, start as usize, end as usize,
        );
        return;
    }

    let expected_str = analysis.format_value_type_depth(expected, 1);
    let actual_str = analysis.format_value_type_depth(&actual, 1);
    let mut message = format!("expected `{}` for field '{}', got `{}`", expected_str, field_name, actual_str);
    super::append_structural_mismatch_suffix(&mut message, analysis, &actual, expected);
    let related = if !class_table_idx.is_external()
        && let Some((rs, re)) = class_field_def_range
    {
        vec![RelatedInfo {
            file_path: None,
            start: rs as usize,
            end: re as usize,
            message: "Field declared here".to_string(),
        }]
    } else {
        Vec::new()
    };
    super::FIELD_TYPE_MISMATCH.emit_with_related(
        diags,
        message,
        start as usize,
        end as usize,
        related,
    );
}

/// Extract the element table index from an array annotation type.
/// For `@type T[]`, returns the TableIndex of T.
/// For unions like `@type T[] | nil`, finds the first array member.
fn extract_array_element_table(analysis: &AnalysisResult, vt: &ValueType) -> Option<TableIndex> {
    match vt {
        ValueType::Table(Some(idx)) => {
            let t = analysis.table(*idx);
            if t.value_type_annotated
                && let Some(ValueType::Table(Some(elem_idx))) = &t.value_type {
                    return Some(*elem_idx);
                }
            None
        }
        ValueType::Union(parts) => parts.iter().find_map(|p| extract_array_element_table(analysis, p)),
        _ => None,
    }
}
