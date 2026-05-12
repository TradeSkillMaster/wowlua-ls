//! Markdown documentation generation from `DocNamespace` data.
//!
//! Produces VitePress-compatible `.md` snippet files: one per class plus an `index.md`.
//! Per-class files are designed for inclusion via `<!--@include: ./api/ClassName.md-->`
//! so users can write custom prose above the generated API reference.

use std::collections::HashSet;
use std::fmt::Write as FmtWrite;
use std::path::Path;

use crate::doc_gen::{DocField, DocFieldKind, DocNamespace, DocParam};

/// Generate markdown documentation files from doc namespaces.
///
/// Creates `{out_dir}/index.md` and `{out_dir}/{ClassName}.md` for each namespace.
pub(crate) fn generate_markdown_docs(namespaces: &[DocNamespace], out_dir: &Path) -> std::io::Result<()> {
    let output_classes: HashSet<&str> = namespaces.iter()
        .map(|ns| ns.name.as_str())
        .collect();

    // Write per-class pages
    for ns in namespaces {
        let page = render_class_page(ns, &output_classes);
        let filename = format!("{}.md", ns.name);
        std::fs::write(out_dir.join(&filename), page)?;
    }

    // Write index page
    let index = render_index(namespaces);
    std::fs::write(out_dir.join("index.md"), index)?;

    Ok(())
}

/// Render the index page listing all classes.
fn render_index(namespaces: &[DocNamespace]) -> String {
    let mut out = String::new();
    out.push_str("# API Reference\n\n");

    if namespaces.is_empty() {
        out.push_str("No classes found.\n");
        return out;
    }

    out.push_str("| Class | Description |\n");
    out.push_str("|-------|-------------|\n");
    for ns in namespaces {
        let desc = ns.defines.first()
            .and_then(|d| d.desc.as_deref())
            .map(first_line)
            .unwrap_or("");
        let _ = writeln!(out, "| [{}]({}.md) | {} |", ns.name, ns.name, escape_pipes(desc));
    }
    out
}

/// Render a single class snippet (no `# Title` — meant for `<!--@include:-->` embedding).
fn render_class_page(ns: &DocNamespace, output_classes: &HashSet<&str>) -> String {
    let mut out = String::new();

    // Inheritance — only linkify parents that have a page in the output set
    if let Some(define) = ns.defines.first()
        && !define.extends.is_empty()
    {
        let parents: Vec<String> = define.extends.iter()
            .map(|p| {
                if output_classes.contains(p.view.as_str()) {
                    format!("[{}]({}.md)", p.view, p.view)
                } else {
                    p.view.clone()
                }
            })
            .collect();
        let _ = writeln!(out, "Inherits: {}\n", parents.join(", "));
    }

    // Partition fields into data fields, methods, and functions
    let mut data_fields: Vec<&DocField> = Vec::new();
    let mut methods: Vec<&DocField> = Vec::new();
    let mut functions: Vec<&DocField> = Vec::new();

    for field in &ns.fields {
        // Skip private fields
        if field.visible.as_deref() == Some("private") {
            continue;
        }
        match field.kind {
            DocFieldKind::DataField => data_fields.push(field),
            DocFieldKind::Method => methods.push(field),
            DocFieldKind::Function => functions.push(field),
        }
    }

    // Data fields table
    if !data_fields.is_empty() {
        out.push_str("## Fields\n\n");
        out.push_str("| Name | Type | Description |\n");
        out.push_str("|------|------|-------------|\n");
        for field in &data_fields {
            let name = format_field_name(&field.name, field.deprecated);
            let type_str = field.view.as_deref().unwrap_or("any");
            let desc = field.desc.as_deref().unwrap_or("");
            let _ = writeln!(out, "| {} | `{}` | {} |",
                name, escape_table_code(type_str), escape_pipes(desc));
        }
        out.push('\n');
    }

    // Methods
    if !methods.is_empty() {
        out.push_str("## Methods\n\n");
        for method in &methods {
            render_callable(&mut out, &ns.name, method, ":");
        }
    }

    // Static functions
    if !functions.is_empty() {
        out.push_str("## Functions\n\n");
        for func in &functions {
            render_callable(&mut out, &ns.name, func, ".");
        }
    }

    out
}

/// Render a method or function section.
fn render_callable(out: &mut String, class_name: &str, field: &DocField, separator: &str) {
    // Section header
    let name = format_field_name(&field.name, field.deprecated);
    let _ = writeln!(out, "### {}\n", name);

    if field.deprecated {
        out.push_str("::: warning Deprecated\n:::\n\n");
    }

    // Signature code block
    if let Some(ref extends) = field.extends {
        let params = format_param_list(&extends.args);
        let returns = format_return_list(&extends.returns);
        let _ = write!(out, "```lua\nfunction {}{}{}({})", class_name, separator, field.name, params);
        if !returns.is_empty() {
            let _ = write!(out, "\n  -> {}", returns);
        }
        out.push_str("\n```\n\n");
    }

    // Description
    if let Some(ref desc) = field.desc
        && !desc.is_empty()
    {
        let _ = writeln!(out, "{}\n", desc);
    }

    // Parameters table
    if let Some(ref extends) = field.extends {
        let visible_params: Vec<&DocParam> = extends.args.iter()
            .filter(|p| p.name.as_deref() != Some("self"))
            .collect();
        if !visible_params.is_empty() {
            out.push_str("**Parameters:**\n\n");
            out.push_str("| Name | Type | Description |\n");
            out.push_str("|------|------|-------------|\n");
            for param in &visible_params {
                let pname = param.name.as_deref().unwrap_or("...");
                let ptype = &param.view;
                let pdesc = param.desc.as_deref().unwrap_or("");
                let _ = writeln!(out, "| `{}` | `{}` | {} |",
                    pname, escape_table_code(ptype), escape_pipes(pdesc));
            }
            out.push('\n');
        }

        // Returns table
        if !extends.returns.is_empty() {
            out.push_str("**Returns:**\n\n");
            let has_names = extends.returns.iter().any(|r| r.name.is_some());
            if has_names {
                out.push_str("| Name | Type | Description |\n");
                out.push_str("|------|------|-------------|\n");
                for ret in &extends.returns {
                    let rname = ret.name.as_deref().unwrap_or("");
                    let rtype = &ret.view;
                    let rdesc = ret.desc.as_deref().unwrap_or("");
                    let _ = writeln!(out, "| `{}` | `{}` | {} |",
                        rname, escape_table_code(rtype), escape_pipes(rdesc));
                }
            } else {
                out.push_str("| Type | Description |\n");
                out.push_str("|------|-------------|\n");
                for ret in &extends.returns {
                    let rtype = &ret.view;
                    let rdesc = ret.desc.as_deref().unwrap_or("");
                    let _ = writeln!(out, "| `{}` | {} |",
                        escape_table_code(rtype), escape_pipes(rdesc));
                }
            }
            out.push('\n');
        }
    }
}

/// Format a parameter list for the signature code block.
fn format_param_list(params: &[DocParam]) -> String {
    params.iter()
        .filter(|p| p.name.as_deref() != Some("self"))
        .map(|p| {
            let name = p.name.as_deref().unwrap_or("...");
            name.to_string()
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format a return type list for the signature code block.
fn format_return_list(returns: &[DocParam]) -> String {
    returns.iter()
        .map(|r| r.view.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format a field/method name, applying strikethrough if deprecated.
fn format_field_name(name: &str, deprecated: bool) -> String {
    if deprecated {
        format!("~~{}~~", name)
    } else {
        name.to_string()
    }
}

/// Get the first line of a string.
fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// Escape pipe characters for markdown table cells.
fn escape_pipes(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Escape content for use inside backtick-fenced inline code in a table cell.
/// Backticks are replaced with single-quotes since nested backtick fencing
/// in markdown tables is unreliable across renderers.
fn escape_table_code(s: &str) -> String {
    s.replace('|', "\\|").replace('`', "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc_gen::*;

    fn make_namespace() -> DocNamespace {
        DocNamespace {
            name: "MyClass".to_string(),
            defines: vec![DocDefine {
                extends: vec![DocBaseClass { view: "ParentClass".to_string() }],
                desc: Some("A test class".to_string()),
            }],
            fields: vec![
                DocField {
                    name: "count".to_string(),
                    kind: DocFieldKind::DataField,
                    extends: None,
                    view: Some("number".to_string()),
                    deprecated: false,
                    visible: Some("public".to_string()),
                    desc: None,
                },
                DocField {
                    name: "GetCount".to_string(),
                    kind: DocFieldKind::Method,
                    extends: Some(DocFieldExtends {
                        args: vec![DocParam {
                            name: Some("self".to_string()),
                            view: "MyClass".to_string(),
                            desc: None,
                        }],
                        returns: vec![DocParam {
                            name: None,
                            view: "number".to_string(),
                            desc: None,
                        }],
                    }),
                    view: None,
                    deprecated: false,
                    visible: Some("public".to_string()),
                    desc: Some("Gets the count.".to_string()),
                },
                DocField {
                    name: "Create".to_string(),
                    kind: DocFieldKind::Function,
                    extends: Some(DocFieldExtends {
                        args: vec![DocParam {
                            name: Some("name".to_string()),
                            view: "string".to_string(),
                            desc: Some("The name".to_string()),
                        }],
                        returns: vec![DocParam {
                            name: None,
                            view: "MyClass".to_string(),
                            desc: None,
                        }],
                    }),
                    view: None,
                    deprecated: false,
                    visible: Some("public".to_string()),
                    desc: Some("Creates a new instance.".to_string()),
                },
                DocField {
                    name: "_internal".to_string(),
                    kind: DocFieldKind::DataField,
                    extends: None,
                    view: Some("any".to_string()),
                    deprecated: false,
                    visible: Some("private".to_string()),
                    desc: None,
                },
            ],
        }
    }

    fn all_classes(ns: &DocNamespace) -> HashSet<&str> {
        let mut set: HashSet<&str> = std::collections::HashSet::new();
        set.insert(ns.name.as_str());
        for d in &ns.defines {
            for p in &d.extends {
                set.insert(p.view.as_str());
            }
        }
        set
    }

    #[test]
    fn class_snippet_has_no_title() {
        let ns = make_namespace();
        let classes = all_classes(&ns);
        let md = render_class_page(&ns, &classes);
        assert!(!md.starts_with("# "), "snippet should not start with a title header");
    }

    #[test]
    fn class_snippet_shows_inheritance() {
        let ns = make_namespace();
        let classes = all_classes(&ns);
        let md = render_class_page(&ns, &classes);
        assert!(md.contains("Inherits: [ParentClass](ParentClass.md)"));
    }

    #[test]
    fn inheritance_link_omitted_when_parent_not_in_output() {
        let ns = make_namespace();
        // Only include MyClass itself, not ParentClass
        let classes: HashSet<&str> = ["MyClass"].into_iter().collect();
        let md = render_class_page(&ns, &classes);
        assert!(md.contains("Inherits: ParentClass\n"), "parent not in output set should be plain text, got: {}", md);
        assert!(!md.contains("[ParentClass]"), "should not contain a link to ParentClass");
    }

    #[test]
    fn private_fields_excluded() {
        let ns = make_namespace();
        let classes = all_classes(&ns);
        let md = render_class_page(&ns, &classes);
        assert!(!md.contains("_internal"));
    }

    #[test]
    fn data_field_in_table() {
        let ns = make_namespace();
        let classes = all_classes(&ns);
        let md = render_class_page(&ns, &classes);
        assert!(md.contains("| count | `number` |"));
    }

    #[test]
    fn method_uses_colon_syntax() {
        let ns = make_namespace();
        let classes = all_classes(&ns);
        let md = render_class_page(&ns, &classes);
        assert!(md.contains("function MyClass:GetCount()"));
    }

    #[test]
    fn function_uses_dot_syntax() {
        let ns = make_namespace();
        let classes = all_classes(&ns);
        let md = render_class_page(&ns, &classes);
        assert!(md.contains("function MyClass.Create(name)"));
    }

    #[test]
    fn self_param_hidden() {
        let ns = make_namespace();
        let classes = all_classes(&ns);
        let md = render_class_page(&ns, &classes);
        // self should not appear in parameter tables
        assert!(!md.contains("| `self`"));
    }

    #[test]
    fn param_description_shown() {
        let ns = make_namespace();
        let classes = all_classes(&ns);
        let md = render_class_page(&ns, &classes);
        assert!(md.contains("The name"));
    }

    #[test]
    fn index_page_links_classes() {
        let ns = make_namespace();
        let index = render_index(&[ns]);
        assert!(index.contains("[MyClass](MyClass.md)"));
    }

    #[test]
    fn escape_pipes_in_table() {
        assert_eq!(escape_pipes("a|b"), "a\\|b");
    }

    #[test]
    fn escape_backticks_in_code() {
        assert_eq!(escape_table_code("`T`"), "'T'");
    }

    #[test]
    fn deprecated_method_strikethrough() {
        let ns = DocNamespace {
            name: "Foo".to_string(),
            defines: vec![DocDefine { extends: vec![], desc: None }],
            fields: vec![DocField {
                name: "OldMethod".to_string(),
                kind: DocFieldKind::Method,
                extends: Some(DocFieldExtends {
                    args: vec![DocParam { name: Some("self".to_string()), view: "Foo".to_string(), desc: None }],
                    returns: vec![],
                }),
                view: None,
                deprecated: true,
                visible: Some("public".to_string()),
                desc: None,
            }],
        };
        let classes: HashSet<&str> = ["Foo"].into_iter().collect();
        let md = render_class_page(&ns, &classes);
        assert!(md.contains("### ~~OldMethod~~"));
        assert!(md.contains("::: warning Deprecated"));
    }
}
