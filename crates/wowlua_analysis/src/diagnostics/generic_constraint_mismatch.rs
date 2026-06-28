use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub struct GenericConstraintMismatch;

impl DiagnosticPass for GenericConstraintMismatch {
    fn run(&self, analysis: &AnalysisResult, tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        // Function call case: walk call_resolutions for calls with generic bindings
        for cr in analysis.ir.call_resolutions.values() {
            if cr.generic_subs.is_empty() { continue; }
            let func = analysis.func(cr.func_idx);
            for (name, bound_type, arg_range) in &cr.generic_subs {
                if matches!(bound_type, ValueType::TypeVariable(_)) { continue; }
                if func.defclass.as_deref() == Some(name.as_str()) { continue; }

                // Check for `keyof` constraints (dynamic — depends on another generic's binding)
                let raw_constraint = func.generic_constraints_raw.iter()
                    .find(|(n, _)| n == name)
                    .and_then(|(_, c)| c.as_ref());
                if let Some(raw_c) = raw_constraint
                    && let Some(ref_name) = crate::annotations::parse_keyof_constraint(raw_c) {
                        if let Some(table_idx) = cr.resolve_keyof_target(ref_name) {
                            let fields = crate::analysis::collect_class_fields_impl(
                                &analysis.ir, &analysis.resolved_expr_cache, table_idx,
                            );
                            let valid = match bound_type {
                                ValueType::String(Some(key)) => fields.iter().any(|(n, _, _)| n == key),
                                _ => true, // Non-literal: can't validate statically
                            };
                            if !valid {
                                let Some(&(start, end)) = arg_range.as_ref() else { continue };
                                let actual_str = analysis.format_value_type_depth(bound_type, 1);
                                super::GENERIC_CONSTRAINT_MISMATCH.emit(diags, format!(
                                    "type `{}` does not satisfy constraint `keyof {}` on generic `{}`",
                                    actual_str, ref_name, name
                                ), start as usize, end as usize);
                            }
                        }
                        continue;
                    }

                let Some(constraint) = func.generics.iter()
                    .find(|(n, _)| n == name)
                    .and_then(|(_, c)| c.as_ref()) else { continue };
                let actual_stripped = bound_type.strip_nil();
                let is_pure_nil = matches!(&actual_stripped, ValueType::Union(t) if t.is_empty());
                if is_pure_nil
                    || (!actual_stripped.is_assignable_to(constraint)
                        && !analysis.is_table_subtype(&actual_stripped, constraint))
                {
                    let Some(&(start, end)) = arg_range.as_ref() else { continue };
                    let constraint_str = analysis.format_value_type_depth(constraint, 1);
                    let actual_str = analysis.format_value_type_depth(bound_type, 1);
                    super::GENERIC_CONSTRAINT_MISMATCH.emit(diags, format!(
                        "type `{}` does not satisfy constraint `{}` on generic `{}`",
                        actual_str, constraint_str, name
                    ), start as usize, end as usize);
                }
            }
        }

        // Class type param case: walk symbols with type_args
        for (_, sym) in analysis.local_symbols() {
            let ver = &sym.versions[0];
            if ver.type_args.is_empty() { continue; }
            let Some(type_source) = ver.type_source else { continue };
            let Expr::Literal(ValueType::Table(Some(class_table_idx))) = analysis.ir.expr(type_source) else { continue };
            let class_table = analysis.table(*class_table_idx);
            if class_table.class_name.is_none() { continue; }
            if class_table.class_type_param_constraints.is_empty() { continue; }
            let def = ver.def_node;
            // `def_node` for a parameter (or `@type`-annotated local) can span the
            // whole enclosing function definition, which would underline the entire
            // function. Narrow to the symbol's name token within that range so the
            // squiggle sits on the offending variable, not the whole body.
            let (emit_start, emit_end) = match &sym.id {
                crate::types::SymbolIdentifier::Name(name) =>
                    name_token_range_in(tree, def.start, def.end, name)
                        .unwrap_or((def.start, def.end)),
                _ => (def.start, def.end),
            };
            for (i, (arg, constraint_raw)) in ver.type_args.iter()
                .zip(class_table.class_type_param_constraints.iter()).enumerate()
            {
                let Some(constraint_str) = constraint_raw else { continue };
                let Some(constraint_type) = analysis.resolve_class_constraint(constraint_str) else { continue };
                let stripped = arg.strip_nil();
                // `is_table_subtype` walks parent classes (the bare `is_assignable_to`
                // can't), so a built/named class whose parent satisfies the constraint
                // (e.g. a builder-built class `Foo : Base` used as `C<Foo>` where
                // `C<T: Base>`) is accepted.
                if !stripped.is_assignable_to(&constraint_type)
                    && !analysis.is_table_subtype(&stripped, &constraint_type) {
                    let param_name = class_table.class_type_params.get(i)
                        .map(|s| s.as_str()).unwrap_or("?");
                    let constraint_display = analysis.format_value_type_depth(&constraint_type, 1);
                    let actual_display = analysis.format_value_type_depth(arg, 1);
                    super::GENERIC_CONSTRAINT_MISMATCH.emit(diags, format!(
                        "type `{}` does not satisfy constraint `{}` on generic `{}`",
                        actual_display, constraint_display, param_name
                    ), emit_start as usize, emit_end as usize);
                }
            }
        }
    }
}

/// Find the first `Name` token whose text matches `name` within the byte range
/// `[start, end)`. Used to narrow a parameter/local diagnostic from its enclosing
/// definition node down to just the variable's name token.
fn name_token_range_in(
    tree: &crate::syntax::tree::SyntaxTree,
    start: u32,
    end: u32,
    name: &str,
) -> Option<(u32, u32)> {
    use crate::syntax::{SyntaxKind, SyntaxNode};
    let root = SyntaxNode::new_root(tree);
    root.descendants_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|tok| {
            // A function parameter is a `Parameter` token; a local name is a `Name`.
            matches!(tok.kind(), SyntaxKind::Name | SyntaxKind::Parameter)
                && tok.text() == name
                && u32::from(tok.text_range().start()) >= start
                && u32::from(tok.text_range().end()) <= end
        })
        .map(|tok| (u32::from(tok.text_range().start()), u32::from(tok.text_range().end())))
}
