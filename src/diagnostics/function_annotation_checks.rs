use crate::analysis::{Analysis, AnalysisResult};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::FunctionIndex;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct FunctionAnnotationChecks;

/// Re-derive function-level annotation structure diagnostics from the AST. Covers
/// redundant-class-generic, malformed-annotation (params<F> position, mixed
/// tuple-union @return), and undefined-doc-name on @param/@return/@overload types
/// (the last via Ir::check_annotation_type_names).
impl DiagnosticPass for FunctionAnnotationChecks {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for func_idx in 0..analysis.ir.functions.len() {
            let func = &analysis.ir.functions[func_idx];
            let Some(nid) = func.def_node.node_id else { continue };
            let func_node = SyntaxNode { tree, id: nid };
            let annotations = crate::annotations::extract_annotations(func_node);
            let comment_ranges = Analysis::collect_preceding_annotation_ranges(func_node);
            let generics = &func.generic_constraints_raw;
            let generic_names: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();

            let func_start = u32::from(func_node.text_range().start()) as usize;
            let func_end = u32::from(func_node.text_range().end()) as usize;

            // ── redundant @generic on class methods ──
            if let Some(class_name) = analysis.function_owner_class.get(&FunctionIndex(func_idx)) {
                let class_type_params: Vec<String> = analysis.ir.classes.get(class_name.as_str())
                    .map(|&tidx| analysis.ir.table(tidx).class_type_params.clone())
                    .unwrap_or_default();

                if !class_type_params.is_empty() {
                    for (gname, _) in annotations.generics.iter() {
                        if class_type_params.contains(gname)
                            && let Some((_, s, e)) = comment_ranges.iter().find(|(text, _, _)| {
                                text.starts_with("---@generic") && Analysis::contains_word(text, gname.as_str())
                            })
                        {
                            super::REDUNDANT_CLASS_GENERIC.emit(
                                diags,
                                format!("`@generic {}` is already a type parameter on the class — remove it and use class-level generics", gname),
                                *s, *e,
                            );
                        }
                    }
                    if annotations.params.iter().any(|p| p.name == "self")
                        && let Some((_, s, e)) = comment_ranges.iter().find(|(text, _, _)| {
                            text.starts_with("---@param") && text.contains("self")
                        })
                    {
                        super::REDUNDANT_CLASS_GENERIC.emit(
                            diags,
                            "`@param self` is unnecessary — class-level type parameters are inherited by colon methods automatically".to_string(),
                            *s, *e,
                        );
                    }
                }
            }

            // ── malformed-annotation: params<F> in non-vararg @param ──
            for p in annotations.params.iter() {
                if p.name == "..." { continue; }
                if let Some(crate::types::ProjectionKind::Params(_)) =
                    crate::annotations::match_projection(&p.typ, &generic_names)
                {
                    let (s, e) = comment_ranges.iter()
                        .find(|(text, _, _)| text.starts_with("---@param") && text.contains(&p.name))
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_start + 1));
                    super::MALFORMED_ANNOTATION.emit(
                        diags,
                        "params<F> projection is only allowed in the vararg slot (`@param ... params<F>`)".to_string(),
                        s, e,
                    );
                }
            }

            // ── malformed-annotation: mixed tuple-union @return ──
            if !annotations.returns.is_empty() {
                let tuple_form_flags: Vec<bool> = annotations.returns.iter()
                    .map(crate::annotations::annotation_is_tuple_form).collect();
                let any_tuple = tuple_form_flags.iter().any(|&b| b);
                let all_tuple = tuple_form_flags.iter().all(|&b| b);
                let is_tuple_form = any_tuple && all_tuple && annotations.returns.len() == 1;
                if any_tuple && !is_tuple_form {
                    let return_ranges: Vec<(usize, usize)> = comment_ranges.iter()
                        .filter(|(text, _, _)| text.starts_with("---@return") || text.starts_with("---|"))
                        .map(|(_, s, e)| (*s, *e))
                        .collect();
                    let (s, e) = if let (Some(first), Some(last)) = (return_ranges.first(), return_ranges.last()) {
                        (first.0, last.1)
                    } else {
                        (func_start, func_start + 1)
                    };
                    super::MALFORMED_ANNOTATION.emit(
                        diags,
                        "cannot mix tuple-union @return with other @return annotations — use a single \
                         tuple-union line with `---|` continuations to list additional cases".to_string(),
                        s, e,
                    );
                }
            }

            // ── malformed-annotation: params<F> in @return ──
            for ret_annotation in annotations.returns.iter() {
                if let Some(crate::types::ProjectionKind::Params(_)) =
                    crate::annotations::match_projection(ret_annotation, &generic_names)
                {
                    let (s, e) = comment_ranges.iter()
                        .find(|(text, _, _)| text.starts_with("---@return"))
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_start + 1));
                    super::MALFORMED_ANNOTATION.emit(
                        diags,
                        "params<F> projection cannot appear in @return (it expands multiple positions, not one)".to_string(),
                        s, e,
                    );
                }
            }

            // ── @requires / @return self<X> must reference the method's class type params ──
            let has_self_reparam = annotations.returns.iter().any(|r| {
                matches!(r, crate::annotations::AnnotationType::Parameterized(n, _) if n == "self")
            });
            if !annotations.requires.is_empty() || has_self_reparam {
                let method_class_type_params: Vec<String> = analysis.function_owner_class
                    .get(&FunctionIndex(func_idx))
                    .and_then(|cn| analysis.ir.classes.get(cn.as_str()))
                    .map(|&tidx| analysis.ir.table(tidx).class_type_params.clone())
                    .unwrap_or_default();

                // @requires T: Constraint — T must be a class type param; the
                // constraint type name must resolve.
                for (pname, constraint) in &annotations.requires {
                    let (s, e) = comment_ranges.iter()
                        .find(|(text, _, _)| text.starts_with("---@requires") && Analysis::contains_word(text, pname))
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_end));
                    if !method_class_type_params.contains(pname) {
                        super::MALFORMED_ANNOTATION.emit(diags, format!(
                            "@requires references `{pname}`, which is not a type parameter of the method's class"
                        ), s, e);
                        continue;
                    }
                    let parsed = crate::annotations::parse_type(constraint);
                    analysis.ir.check_annotation_type_names(&parsed, generics, s, e, diags);
                }

                // @return self<X> — the number of type args must match the class's
                // type-param arity (a non-generic class declares none).
                for ret in &annotations.returns {
                    if let crate::annotations::AnnotationType::Parameterized(name, args) = ret
                        && name == "self"
                        && method_class_type_params.len() != args.len()
                    {
                        let (s, e) = comment_ranges.iter()
                            .find(|(text, _, _)| text.starts_with("---@return") && text.contains("self"))
                            .map(|(_, s, e)| (*s, *e))
                            .unwrap_or((func_start, func_end));
                        let msg = if method_class_type_params.is_empty() {
                            "@return self<...> requires the method's class to declare type parameters".to_string()
                        } else {
                            format!(
                                "@return self<...> expects {} type argument(s) to match the class, found {}",
                                method_class_type_params.len(), args.len()
                            )
                        };
                        super::MALFORMED_ANNOTATION.emit(diags, msg, s, e);
                    }
                }
            }

            // ── undefined-doc-name on @param, @return, @overload types ──
            for p in &annotations.params {
                let (s, e) = comment_ranges.iter()
                    .find(|(text, _, _)| text.starts_with("---@param") && Analysis::contains_word(text, &p.name))
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                analysis.ir.check_annotation_type_names(&p.typ, generics, s, e, diags);
            }
            for ret in &annotations.returns {
                let (s, e) = comment_ranges.iter()
                    .find(|(text, _, _)| text.starts_with("---@return"))
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                analysis.ir.check_annotation_type_names(ret, generics, s, e, diags);
            }
            for (i, overload_str) in annotations.overloads.iter().enumerate() {
                if let Some(sig) = crate::annotations::parse_overload(overload_str) {
                    let (s, e) = comment_ranges.iter()
                        .filter(|(text, _, _)| text.starts_with("---@overload"))
                        .nth(i)
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_end));
                    for p in &sig.params {
                        analysis.ir.check_annotation_type_names(&p.typ, generics, s, e, diags);
                    }
                    for ret in &sig.returns {
                        analysis.ir.check_annotation_type_names(ret, generics, s, e, diags);
                    }
                }
            }
        }
    }
}
