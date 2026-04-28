use crate::analysis::AnalysisResult;
use crate::types::*;

pub(crate) const CODE: &str = "generic-constraint-mismatch";

pub(crate) fn run(analysis: &AnalysisResult, diags: &mut Vec<super::WowDiagnostic>) {
    // Function call case: walk call_resolutions for calls with generic bindings
    for cr in analysis.ir.call_resolutions.values() {
        if cr.generic_subs.is_empty() { continue; }
        let func = analysis.func(cr.func_idx);
        for (name, bound_type, arg_range) in &cr.generic_subs {
            let Some(constraint) = func.generics.iter()
                .find(|(n, _)| n == name)
                .and_then(|(_, c)| c.as_ref()) else { continue };
            if matches!(bound_type, ValueType::TypeVariable(_)) { continue; }
            if func.defclass.as_deref() == Some(name.as_str()) { continue; }
            let actual_stripped = bound_type.strip_nil();
            let is_pure_nil = matches!(&actual_stripped, ValueType::Union(t) if t.is_empty());
            if is_pure_nil
                || (!actual_stripped.is_assignable_to(constraint)
                    && !analysis.is_table_subtype(&actual_stripped, constraint))
            {
                let Some(&(start, end)) = arg_range.as_ref() else { continue };
                let constraint_str = analysis.format_value_type_depth(constraint, 1);
                let actual_str = analysis.format_value_type_depth(bound_type, 1);
                diags.push(super::WowDiagnostic {
                    code: CODE,
                    message: format!(
                        "type `{}` does not satisfy constraint `{}` on generic `{}`",
                        actual_str, constraint_str, name
                    ),
                    severity: lsp_types::DiagnosticSeverity::WARNING,
                    start: start as usize,
                    end: end as usize,
                });
            }
        }
    }

    // Class type param case: walk symbols with type_args
    for sym in &analysis.ir.symbols {
        let ver = &sym.versions[0];
        if ver.type_args.is_empty() { continue; }
        let Some(type_source) = ver.type_source else { continue };
        let Expr::Literal(ValueType::Table(Some(class_table_idx))) = analysis.ir.expr(type_source) else { continue };
        let class_table = analysis.table(*class_table_idx);
        if class_table.class_name.is_none() { continue; }
        if class_table.class_type_param_constraints.is_empty() { continue; }
        let def = ver.def_node;
        for (i, (arg, constraint_raw)) in ver.type_args.iter()
            .zip(class_table.class_type_param_constraints.iter()).enumerate()
        {
            let Some(constraint_str) = constraint_raw else { continue };
            let Some(constraint_type) = analysis.resolve_class_constraint(constraint_str) else { continue };
            let stripped = arg.strip_nil();
            if !stripped.is_assignable_to(&constraint_type) {
                let param_name = class_table.class_type_params.get(i)
                    .map(|s| s.as_str()).unwrap_or("?");
                let constraint_display = analysis.format_value_type_depth(&constraint_type, 1);
                let actual_display = analysis.format_value_type_depth(arg, 1);
                diags.push(super::WowDiagnostic {
                    code: CODE,
                    message: format!(
                        "type `{}` does not satisfy constraint `{}` on generic `{}`",
                        actual_display, constraint_display, param_name
                    ),
                    severity: lsp_types::DiagnosticSeverity::WARNING,
                    start: def.start as usize,
                    end: def.end as usize,
                });
            }
        }
    }
}
