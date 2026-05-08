use std::collections::{HashMap, HashSet};

use crate::analysis::{Analysis, AnalysisResult};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::ValueType;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) fn extract_name(message: &str) -> Option<&str> {
    message.strip_prefix("undefined class '").and_then(|s| s.strip_suffix('\''))
}

pub(crate) struct UndefinedDocClass;

/// Walk @class / @alias declarations from the AST and validate:
/// - undefined-doc-class on parent class names
/// - circle-doc-class on cyclic inheritance
/// - undefined-doc-name on class field types and alias bodies
impl DiagnosticPass for UndefinedDocClass {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        let scan = crate::annotations::scan_all_annotations(root);

        let no_generics: Vec<(String, Option<String>)> = Vec::new();
        let valid_parent_builtins: HashSet<&str> = [
            "table", "userdata", "any", "unknown",
        ].into_iter().collect();
        let primitive_types: HashSet<&str> = [
            "nil", "boolean", "bool", "number", "integer",
            "string", "function", "fun", "true", "false", "thread",
        ].into_iter().collect();

        // ── invalid-class-parent / undefined-doc-class: check parent class names ──
        for class in &scan.classes {
            let prefix = format!("---@class {}", class.name);
            for parent_name in &class.parents {
                if valid_parent_builtins.contains(parent_name.as_str()) { continue; }
                if parent_name.starts_with("table<") { continue; }

                // Primitive type names, string/number literals, unions, and
                // function types cannot be inherited from — classes are tables.
                if primitive_types.contains(parent_name.as_str())
                    || parent_name.starts_with('"') || parent_name.starts_with('\'')
                    || parent_name.contains('|')
                    || parent_name.starts_with("fun(")
                    || parent_name.parse::<f64>().is_ok()
                {
                    if let Some((start, end)) = Analysis::find_nth_annotation_comment_range(root, &prefix, parent_name, 1) {
                        super::INVALID_CLASS_PARENT.emit(
                            diags,
                            format!("cannot inherit from non-class type '{}'", parent_name),
                            start as usize, end as usize,
                        );
                    }
                    continue;
                }

                if analysis.ir.classes.contains_key(parent_name.as_str()) { continue; }

                // Aliases that resolve to non-table types cannot be inherited from.
                let alias_type = analysis.ir.aliases.get(parent_name.as_str())
                    .or_else(|| analysis.ir.ext.aliases.get(parent_name.as_str()));
                if let Some(vt) = alias_type {
                    if !is_inheritable_type(vt)
                        && let Some((start, end)) = Analysis::find_nth_annotation_comment_range(root, &prefix, parent_name, 1)
                    {
                        super::INVALID_CLASS_PARENT.emit(
                            diags,
                            format!("cannot inherit from non-class type '{}' (resolves to `{}`)", parent_name, analysis.format_type(vt)),
                            start as usize, end as usize,
                        );
                    }
                    continue;
                }

                if analysis.ir.parameterized_aliases.contains_key(parent_name.as_str()) { continue; }
                if analysis.ir.ext.parameterized_aliases.contains_key(parent_name.as_str()) { continue; }

                if let Some((start, end)) = Analysis::find_nth_annotation_comment_range(root, &prefix, parent_name, 1) {
                    super::UNDEFINED_DOC_CLASS.emit(
                        diags,
                        format!("undefined class '{}'", parent_name),
                        start as usize, end as usize,
                    );
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
                    super::CIRCLE_DOC_CLASS.emit(
                        diags,
                        format!("circular inheritance: {} -> {}", class.name, cycle_str),
                        start as usize, end as usize,
                    );
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
}

/// Returns true if a ValueType can meaningfully be used as a class parent.
fn is_inheritable_type(vt: &ValueType) -> bool {
    match vt {
        ValueType::Table(_) | ValueType::Any | ValueType::Userdata => true,
        ValueType::Union(members) => members.iter().any(is_inheritable_type),
        ValueType::Intersection(members) => members.iter().any(is_inheritable_type),
        ValueType::OpaqueAlias(_, inner) => is_inheritable_type(inner),
        _ => false,
    }
}
