use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub struct MissingFields;

/// Whether a class field must be present in a constructor (non-nullable, non-function).
fn is_required_field(fi: &FieldInfo) -> bool {
    if fi.lateinit { return false; }
    let Some(ann) = &fi.annotation else { return false };
    let is_nullable = match ann {
        ValueType::Nil => true,
        ValueType::Union(types) => types.contains(&ValueType::Nil),
        _ => false,
    };
    if is_nullable { return false; }
    if matches!(ann, ValueType::Function(_)) { return false; }
    true
}

/// Required-field test that also honors built-in-stub-shadow scoping. When a
/// workspace `@class` reuses a built-in stub class name, its OWN `@field`
/// declarations form the construction contract — the stub's fields (additively
/// merged onto the same table) are not part of it, so requiring them would
/// false-positive on the workspace's own constructors. For such a class we
/// require only the workspace-declared fields; for a plain stub class with no
/// workspace `@field` contract, or any non-stub class, behavior is unchanged.
fn is_required_contract_field(
    analysis: &AnalysisResult,
    class_name: &str,
    field_name: &str,
    fi: &FieldInfo,
) -> bool {
    if !is_required_field(fi) { return false; }
    if analysis.ir.ext.stub_class_names.contains(class_name)
        && let Some(declared) = analysis.ir.ext.declared_class_fields.get(class_name)
    {
        return declared.contains(field_name);
    }
    true
}

fn check_missing_fields(
    analysis: &AnalysisResult,
    ctor_idx: TableIndex,
    class_idx: TableIndex,
    diags: &mut Vec<WowDiagnostic>,
) {
    let rhs_table = analysis.ir.table(ctor_idx);

    let class_table = analysis.table(class_idx);
    let Some(class_name) = &class_table.class_name else { return };

    let Some(&(start, end)) = analysis.ir.table_ranges.iter()
        .find(|(_, idx)| **idx == ctor_idx)
        .map(|(range, _)| range) else { return };

    // Argument context (strict): a plain table literal passed where a class with
    // methods is expected cannot be a real instance — it lacks the methods that
    // would be called on it. Require the class's (possibly inherited) methods.
    // The lenient `@type` / assignment / bracket contexts are not in
    // `tc_arg_constructors`, so explicit casts and the construct-then-Mixin idiom
    // still type-check. A function-typed param accepts the *data* type instead.
    if analysis.ir.tc_arg_constructors.contains(&ctor_idx) {
        let mut missing_methods: Vec<String> = analysis.collect_class_fields(class_idx)
            .into_iter()
            .filter(|(name, ty, lateinit)| {
                !*lateinit
                    && matches!(ty, ValueType::Function(_) | ValueType::FunctionSig(_))
                    && !rhs_table.fields.contains_key(name.as_str())
            })
            .map(|(name, _, _)| name)
            .collect();
        if !missing_methods.is_empty() {
            missing_methods.sort();
            super::MISSING_FIELDS.emit(
                diags,
                format!(
                    "table literal cannot satisfy class '{class_name}': it requires methods \
                     (e.g. '{}') that only an instance provides",
                    missing_methods[0]
                ),
                start as usize,
                end as usize,
            );
            return;
        }
    }

    if rhs_table.fields.is_empty() { return; }

    let mut missing: Vec<&str> = Vec::new();
    for (field_name, fi) in &class_table.fields {
        if !is_required_contract_field(analysis, class_name, field_name, fi) { continue; }
        if !rhs_table.fields.contains_key(field_name.as_str()) {
            missing.push(field_name);
        }
    }
    if !missing.is_empty() {
        missing.sort_unstable();
        let fields_str = missing.join("', '");
        let message = if missing.len() == 1 {
            format!("missing required field '{}' in class '{}'", fields_str, class_name)
        } else {
            format!("missing required fields '{}' in class '{}'", fields_str, class_name)
        };
        super::MISSING_FIELDS.emit(diags, message, start as usize, end as usize);
    }
}

/// Whether a *collected* (possibly inherited) class field — `(name, resolved_type,
/// lateinit)` from [`AnalysisResult::collect_class_fields`] — is required in a
/// constructor: non-lateinit, non-nullable, and not a method (methods come from the
/// mixin, never from a data literal). Honors the same built-in-stub-shadow scoping
/// as [`is_required_contract_field`]; the inherited counterpart of `is_required_field`.
fn is_required_collected_field(
    analysis: &AnalysisResult,
    class_name: &str,
    field_name: &str,
    ty: &ValueType,
    lateinit: bool,
) -> bool {
    if lateinit { return false; }
    let nullable = matches!(ty, ValueType::Nil)
        || matches!(ty, ValueType::Union(types) if types.contains(&ValueType::Nil));
    if nullable { return false; }
    if matches!(ty, ValueType::Function(_) | ValueType::FunctionSig(_)) { return false; }
    if analysis.ir.ext.stub_class_names.contains(class_name)
        && let Some(declared) = analysis.ir.ext.declared_class_fields.get(class_name)
    {
        return declared.contains(field_name);
    }
    true
}

/// Emits a diagnostic only when no union member is satisfied by the constructor.
/// For single-class expectations, delegates to `check_missing_fields`.
///
/// A member is satisfied only when the literal provides all of the member's required
/// fields **and** shares at least one of its (inherited) field names. The share guard
/// is load-bearing for the `Data | Object` mixin-parameter unions the stub-gen remap
/// produces: the object member (`colorRGB : ColorRGBData, ColorMixin`) declares no
/// fields of its own and the data member can be all-optional (`ItemLocationData`), so
/// without it a typo'd / foreign literal (`{red,green,blue}`, `{foo}`) would vacuously
/// satisfy a member and slip through. Fields are collected with inheritance
/// (`collect_class_fields`), matching the single-class and `@type` (`check_fields_impl`)
/// paths.
fn check_missing_fields_union(
    analysis: &AnalysisResult,
    ctor_idx: TableIndex,
    class_indices: &[TableIndex],
    diags: &mut Vec<WowDiagnostic>,
) {
    if class_indices.len() <= 1 {
        // Single class — use the original path
        if let Some(&class_idx) = class_indices.first() {
            check_missing_fields(analysis, ctor_idx, class_idx, diags);
        }
        return;
    }

    let rhs_table = analysis.ir.table(ctor_idx);
    if rhs_table.fields.is_empty() { return; }

    let mut any_shared = false;
    let mut best: Option<(usize, TableIndex)> = None; // (missing_required_count, class_idx)
    for &class_idx in class_indices {
        let Some(class_name) = analysis.table(class_idx).class_name.clone() else { continue };
        let fields = analysis.collect_class_fields(class_idx);
        let shares = fields.iter().any(|(n, _, _)| rhs_table.fields.contains_key(n.as_str()));
        any_shared |= shares;
        let missing_required = fields.iter().filter(|(n, ty, lateinit)| {
            is_required_collected_field(analysis, &class_name, n, ty, *lateinit)
                && !rhs_table.fields.contains_key(n.as_str())
        }).count();
        if shares && missing_required == 0 {
            // The literal overlaps this member and supplies all its required fields.
            return;
        }
        if best.is_none_or(|(count, _)| missing_required < count) {
            best = Some((missing_required, class_idx));
        }
    }

    let Some(&(start, end)) = analysis.ir.table_ranges.iter()
        .find(|(_, idx)| **idx == ctor_idx)
        .map(|(range, _)| range) else { return };
    let Some((_, best_idx)) = best else { return };

    if !any_shared {
        // The literal shares no field with any member — a typo'd or foreign table.
        // The best member may have no *required* fields to report (all-optional
        // data), so emit a dedicated message naming a stray key rather than relying
        // on the missing-required path.
        let class_name = analysis.table(best_idx).class_name.clone().unwrap_or_default();
        let mut keys: Vec<&str> = rhs_table.fields.keys().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        let stray = keys.first().copied().unwrap_or_default();
        super::MISSING_FIELDS.emit(
            diags,
            format!("table literal cannot satisfy class '{class_name}': '{stray}' is not a field of it"),
            start as usize,
            end as usize,
        );
        return;
    }

    // Shares a member but is missing required fields — report against the best member.
    check_missing_fields(analysis, ctor_idx, best_idx, diags);
}

impl DiagnosticPass for MissingFields {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        // Pass 1: Symbols with @class type annotation assigned a table constructor
        for (_, sym) in analysis.local_symbols() {
            let ver = &sym.versions[0];
            let Some(original_expr) = ver.original_type_source else { continue };
            let Some(type_source) = ver.type_source else { continue };

            let Expr::Literal(ValueType::Table(Some(class_table_idx))) = analysis.ir.expr(type_source) else { continue };

            let Some(rhs_table_idx) = analysis.ir.find_table_index(original_expr) else { continue };
            // Skip constructors already covered by tc_expected_class (Pass 2)
            if analysis.ir.tc_expected_class.contains_key(&rhs_table_idx) { continue; }

            check_missing_fields(analysis, rhs_table_idx, *class_table_idx, diags);
        }

        // Pass 2: Table constructors with expected class from tc_expected_class
        // (covers nested constructors in table<K,V>, function args, bracket assignments)
        for (&ctor_idx, class_indices) in &analysis.ir.tc_expected_class {
            if ctor_idx.is_external() { continue; }
            check_missing_fields_union(analysis, ctor_idx, class_indices, diags);
        }
    }
}
