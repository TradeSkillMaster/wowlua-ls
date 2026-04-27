use std::collections::{HashMap, HashSet};

use lsp_types::DiagnosticSeverity;
use crate::analysis::{Analysis, AnalysisResult};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "undefined-doc-class";

pub(crate) fn extract_name(message: &str) -> Option<&str> {
    message.strip_prefix("undefined class '").and_then(|s| s.strip_suffix('\''))
}

/// Walk @class / @alias declarations from the AST and validate:
/// - undefined-doc-class on parent class names
/// - circle-doc-class on cyclic inheritance
/// - undefined-doc-name on class field types and alias bodies
pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    let root = SyntaxNode::new_root(tree);
    let scan = crate::annotations::scan_all_annotations(root);

    let no_generics: Vec<(String, Option<String>)> = Vec::new();
    let builtin_types: HashSet<&str> = [
        "nil", "boolean", "bool", "number", "integer",
        "string", "table", "function", "fun", "any",
        "unknown", "userdata", "thread",
    ].into_iter().collect();

    // ── undefined-doc-class: check parent class names ──
    for class in &scan.classes {
        for parent_name in &class.parents {
            if builtin_types.contains(parent_name.as_str()) { continue; }
            if analysis.ir.classes.contains_key(parent_name.as_str()) { continue; }
            if analysis.ir.aliases.contains_key(parent_name.as_str()) { continue; }
            if analysis.ir.parameterized_aliases.contains_key(parent_name.as_str()) { continue; }
            if analysis.ir.ext.parameterized_aliases.contains_key(parent_name.as_str()) { continue; }
            let prefix = format!("---@class {}", class.name);
            if let Some((start, end)) = Analysis::find_nth_annotation_comment_range(root, &prefix, parent_name, 1) {
                diags.push(WowDiagnostic {
                    code: CODE,
                    message: format!("undefined class '{}'", parent_name),
                    severity: DiagnosticSeverity::WARNING,
                    start: start as usize,
                    end: end as usize,
                });
            }
        }
    }

    // ── circle-doc-class: detect circular inheritance chains ──
    {
        let mut parent_map: HashMap<String, Vec<String>> = HashMap::new();
        for class in &scan.classes {
            if !class.parents.is_empty() {
                parent_map.insert(class.name.clone(), class.parents.clone());
            }
        }
        for (class_name, table_idx) in &analysis.ir.classes {
            let t = analysis.ir.table(*table_idx);
            if !t.parent_classes.is_empty() && !parent_map.contains_key(class_name.as_str()) {
                let parents: Vec<String> = t.parent_classes.iter()
                    .filter_map(|&pi| analysis.ir.table(pi).class_name.clone())
                    .collect();
                if !parents.is_empty() {
                    parent_map.insert(class_name.clone(), parents);
                }
            }
        }

        let mut reported: HashSet<String> = HashSet::new();
        for class in &scan.classes {
            let mut visited = vec![class.name.clone()];
            let mut queue = class.parents.clone();
            let mut found_cycle = false;
            while let Some(ancestor) = queue.pop() {
                if ancestor == class.name {
                    found_cycle = true;
                    break;
                }
                if visited.contains(&ancestor) { continue; }
                visited.push(ancestor.clone());
                if let Some(parents) = parent_map.get(&ancestor) {
                    queue.extend(parents.iter().cloned());
                }
            }
            if found_cycle && reported.insert(class.name.clone())
                && let Some((start, end)) = class.def_range
            {
                let cycle_str = visited[1..].join(" -> ");
                diags.push(WowDiagnostic {
                    code: super::circle_doc_class::CODE,
                    message: format!("circular inheritance: {} -> {}", class.name, cycle_str),
                    severity: DiagnosticSeverity::WARNING,
                    start: start as usize,
                    end: end as usize,
                });
            }
        }
    }

    // ── undefined-doc-name on class field type annotations ──
    for class in &scan.classes {
        let mut generics_with_type_params: Vec<(String, Option<String>)> = class.generics.clone();
        for tp in &class.type_params {
            generics_with_type_params.push((tp.clone(), None));
        }
        for (field_name, annotation_type, _) in &class.fields {
            if let Some((start, end)) = Analysis::find_field_comment_range(root, &class.name, field_name, false) {
                analysis.ir.check_annotation_type_names(annotation_type, &generics_with_type_params, start as usize, end as usize, diags);
            }
        }
    }

    // ── undefined-doc-name on alias type annotations ──
    for alias in &scan.aliases {
        if let Some((start, end)) = Analysis::find_nth_annotation_comment_range(root, "---@alias", &alias.name, 1) {
            let generics: Vec<(String, Option<String>)> = alias.type_params.iter()
                .map(|tp| (tp.clone(), None))
                .collect();
            let check_generics = if generics.is_empty() { &no_generics } else { &generics };
            analysis.ir.check_annotation_type_names(&alias.typ, check_generics, start as usize, end as usize, diags);
        }
    }
}
