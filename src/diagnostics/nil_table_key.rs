use crate::analysis::{Analysis, AnalysisResult};
use crate::annotations::AnnotationType;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

/// Returns true if the annotation type is or contains nil (recursively through unions).
fn annotation_type_contains_nil(at: &AnnotationType) -> bool {
    match at {
        AnnotationType::Simple(name) => name == "nil",
        AnnotationType::Union(members) => members.iter().any(annotation_type_contains_nil),
        _ => false,
    }
}

/// Recursively collect formatted key type strings for every `table<K, V>` where K contains nil.
fn collect_nil_table_keys(at: &AnnotationType) -> Vec<String> {
    let mut violations = Vec::new();
    collect_nil_table_keys_inner(at, &mut violations);
    violations
}

fn collect_nil_table_keys_inner(at: &AnnotationType, out: &mut Vec<String>) {
    match at {
        AnnotationType::Parameterized(base, args) if base == "table" && args.len() >= 2 => {
            if annotation_type_contains_nil(&args[0]) {
                out.push(crate::annotations::format_annotation_type(&args[0]));
            }
            for arg in args {
                collect_nil_table_keys_inner(arg, out);
            }
        }
        AnnotationType::Parameterized(_, args) => {
            for arg in args {
                collect_nil_table_keys_inner(arg, out);
            }
        }
        AnnotationType::Union(members) => {
            for m in members {
                collect_nil_table_keys_inner(m, out);
            }
        }
        AnnotationType::Array(inner) => collect_nil_table_keys_inner(inner, out),
        AnnotationType::Fun(params, returns, _) => {
            for p in params {
                collect_nil_table_keys_inner(&p.typ, out);
            }
            for r in returns {
                collect_nil_table_keys_inner(r, out);
            }
        }
        AnnotationType::Intersection(members) => {
            for m in members {
                collect_nil_table_keys_inner(m, out);
            }
        }
        AnnotationType::NonNil(inner) => collect_nil_table_keys_inner(inner, out),
        AnnotationType::TableLiteral(fields) => {
            for (_, ft) in fields {
                collect_nil_table_keys_inner(ft, out);
            }
        }
        AnnotationType::VarArgs(inner) => collect_nil_table_keys_inner(inner, out),
        AnnotationType::Tuple(positions, _) => {
            for pos in positions {
                collect_nil_table_keys_inner(&pos.typ, out);
            }
        }
        AnnotationType::IndexedAccess(_, key) => collect_nil_table_keys_inner(key, out),
        AnnotationType::Simple(_) | AnnotationType::Backtick(_) => {}
    }
}

/// Emit nil-table-key diagnostics for pre-collected violations.
fn emit_violations(violations: &[String], start: usize, end: usize, diags: &mut Vec<WowDiagnostic>) {
    for key_type_str in violations {
        super::NIL_TABLE_KEY.emit(
            diags,
            format!("table key type `{}` includes nil — Lua table keys cannot be nil", key_type_str),
            start, end,
        );
    }
}

pub(crate) struct NilTableKey;

impl DiagnosticPass for NilTableKey {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        let scan = crate::annotations::scan_all_annotations(root);

        // ── @class Foo : table<K, V> parents ──
        for class in &scan.classes {
            let prefix = format!("---@class {}", class.name);
            for parent_name in &class.parents {
                if !parent_name.contains('<') { continue; }
                let at = crate::annotations::parse_type(parent_name);
                let violations = collect_nil_table_keys(&at);
                if !violations.is_empty()
                    && let Some((start, end)) = Analysis::find_nth_annotation_comment_range(root, &prefix, parent_name, 1)
                {
                    emit_violations(&violations, start as usize, end as usize, diags);
                }
            }

            // ── @field types ──
            for (field_name, annotation_type, _) in &class.fields {
                let violations = collect_nil_table_keys(annotation_type);
                if !violations.is_empty()
                    && let Some((start, end)) = Analysis::find_field_comment_range(root, &class.name, field_name, false)
                {
                    emit_violations(&violations, start as usize, end as usize, diags);
                }
            }
        }

        // ── @alias types ──
        for alias in &scan.aliases {
            let violations = collect_nil_table_keys(&alias.typ);
            if !violations.is_empty()
                && let Some((start, end)) = Analysis::find_nth_annotation_comment_range(root, "---@alias", &alias.name, 1)
            {
                emit_violations(&violations, start as usize, end as usize, diags);
            }
        }

        // ── Function annotations (@param, @return, @overload) ──
        for func in &analysis.ir.functions {
            let Some(nid) = func.def_node.node_id else { continue };
            let func_node = SyntaxNode { tree, id: nid };
            let annotations = crate::annotations::extract_annotations(func_node);
            let comment_ranges = Analysis::collect_preceding_annotation_ranges(func_node);
            let func_start = u32::from(func_node.text_range().start()) as usize;
            let func_end = u32::from(func_node.text_range().end()) as usize;

            for p in &annotations.params {
                let violations = collect_nil_table_keys(&p.typ);
                if !violations.is_empty() {
                    let (s, e) = comment_ranges.iter()
                        .find(|(text, _, _)| Analysis::comment_is_tag(text, "---@param") && Analysis::contains_word(text, &p.name))
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_end));
                    emit_violations(&violations, s, e, diags);
                }
            }

            for (i, ret) in annotations.returns.iter().enumerate() {
                let violations = collect_nil_table_keys(ret);
                if !violations.is_empty() {
                    let (s, e) = comment_ranges.iter()
                        .filter(|(text, _, _)| Analysis::comment_is_tag(text, "---@return"))
                        .nth(i)
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_end));
                    emit_violations(&violations, s, e, diags);
                }
            }

            for (i, overload_str) in annotations.overloads.iter().enumerate() {
                if let Some(sig) = crate::annotations::parse_overload(overload_str) {
                    let (s, e) = comment_ranges.iter()
                        .filter(|(text, _, _)| Analysis::comment_is_tag(text, "---@overload"))
                        .nth(i)
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_end));
                    for p in &sig.params {
                        let violations = collect_nil_table_keys(&p.typ);
                        emit_violations(&violations, s, e, diags);
                    }
                    for r in &sig.returns {
                        let violations = collect_nil_table_keys(r);
                        emit_violations(&violations, s, e, diags);
                    }
                }
            }
        }

        // ── @type on variables ──
        for node in root.descendants() {
            match node.kind() {
                crate::syntax::SyntaxKind::LocalAssignStatement | crate::syntax::SyntaxKind::AssignStatement => {
                    let annotations = crate::annotations::extract_annotations(node);
                    if let Some(ref at) = annotations.var_type {
                        let violations = collect_nil_table_keys(at);
                        if !violations.is_empty() {
                            let comment_ranges = Analysis::collect_preceding_annotation_ranges(node);
                            let (s, e) = comment_ranges.iter()
                                .find(|(text, _, _)| Analysis::comment_is_tag(text, "---@type"))
                                .map(|(_, s, e)| (*s, *e))
                                .unwrap_or_else(|| {
                                    let r = node.text_range();
                                    (u32::from(r.start()) as usize, u32::from(r.end()) as usize)
                                });
                            emit_violations(&violations, s, e, diags);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
