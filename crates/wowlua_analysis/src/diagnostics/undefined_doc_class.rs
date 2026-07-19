use std::collections::{HashMap, HashSet};

use crate::analysis::{Analysis, AnalysisResult};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::ValueType;
use super::{DiagnosticPass, WowDiagnostic};

pub fn extract_name(message: &str) -> Option<&str> {
    message.strip_prefix("undefined class '").and_then(|s| s.strip_suffix('\''))
}

pub struct UndefinedDocClass;

/// Walk @class / @alias declarations from the AST and validate:
/// - undefined-doc-class on parent class names
/// - circle-doc-class on cyclic inheritance
/// - undefined-doc-name on class field types and alias bodies
impl DiagnosticPass for UndefinedDocClass {
    fn runs_in_meta(&self) -> bool { true }

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
                // Parameterized parents: extract base name and check if it's a known class
                // or builtin. e.g. "Parent<T>" → check "Parent", "table<K,V>" → check "table".
                // Skip if known; fall through if unknown (e.g. "tabel<string, number>" typo).
                if let Some(base) = parent_name.strip_suffix('>').and_then(|s| s.split('<').next())
                    && (valid_parent_builtins.contains(base)
                        || analysis.ir.classes.contains_key(base)
                        || analysis.ir.ext.classes.contains_key(base))
                {
                    continue;
                }

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

                // Known class — local OR cross-file (workspace/stub). The `ext`
                // check is load-bearing for perf: without it, every class whose
                // parent is defined in another file falls through to the
                // `find_nth_annotation_comment_range` full-tree walk below and
                // emits an undefined-doc-class that the post-pass `retain` then
                // discards (because the name IS in `ext.classes`). On large
                // cross-file-inheritance workspaces that wasted walk dominated the
                // whole diagnostic phase. Mirrors the parametrized-parent check above.
                if analysis.ir.classes.contains_key(parent_name.as_str())
                    || analysis.ir.ext.classes.contains_key(parent_name.as_str()) { continue; }

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
            // Parent relationships declared in THIS file. The declared parents win
            // over the resolved IR (a file's own `@class Foo : Bar` takes priority),
            // matching the original precedence.
            let mut scan_parent_map: HashMap<&str, &Vec<String>> = HashMap::new();
            for class in &scan.classes {
                if !class.parents.is_empty() {
                    scan_parent_map.insert(class.name.as_str(), &class.parents);
                }
            }

            // Resolve a class name to its parent class names. Prefer this file's
            // declared parents; otherwise consult the resolved IR class table.
            // Looking this up lazily during the walk — rather than eagerly
            // materializing a map over every class in `ir.classes` — is essential:
            // `ir.classes` holds the entire stub+workspace class universe (tens of
            // thousands of entries), and iterating all of it once per file
            // dominated the whole diagnostics phase on large workspaces. The BFS
            // only ever needs the few ancestors reachable from this file's classes.
            let parents_of = |name: &str| -> Vec<String> {
                if let Some(p) = scan_parent_map.get(name) {
                    return (*p).clone();
                }
                if let Some(&ti) = analysis.ir.classes.get(name) {
                    return analysis.ir.table(ti).parent_classes.iter()
                        .filter_map(|&pi| analysis.ir.table(pi).class_name.clone())
                        .collect();
                }
                Vec::new()
            };

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
                    queue.extend(parents_of(&ancestor));
                    visited.push(ancestor);
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
                // The comment range for each `@field` is captured during the scan
                // above (`class.field_ranges`). Prefer it over a fresh
                // `find_field_comment_range` full-tree walk per field: the latter is
                // O(fields × tree_size) and dominated diagnostic time on large,
                // heavily-annotated workspaces.
                if let Some(&(start, end)) = class.field_ranges.get(field_name) {
                    analysis.ir.check_annotation_type_names(annotation_type, &generics_with_type_params, start as usize, end as usize, diags);
                }
            }
        }

        // ── undefined-doc-name on alias type annotations ──
        for alias in &scan.aliases {
            // `alias.def_range` is the `---@alias` comment range captured during the
            // scan above — reuse it instead of a fresh per-alias full-tree walk.
            if let Some((start, end)) = alias.def_range {
                let generics: Vec<(String, Option<String>)> = alias.type_params.iter()
                    .map(|tp| (tp.clone(), None))
                    .collect();
                let check_generics = if generics.is_empty() { &no_generics } else { &generics };
                analysis.ir.check_annotation_type_names(&alias.typ, check_generics, start as usize, end as usize, diags);
                // Validate the constraint type names themselves, e.g. `@alias W<T: Bogus>`.
                for constraint in alias.type_param_constraints.iter().flatten() {
                    let parsed = crate::annotations::parse_type(constraint);
                    analysis.ir.check_annotation_type_names(&parsed, check_generics, start as usize, end as usize, diags);
                }
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
