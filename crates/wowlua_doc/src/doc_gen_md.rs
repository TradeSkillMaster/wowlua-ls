//! Markdown documentation generation from `DocNamespace` data.
//!
//! Produces VitePress-compatible `.md` snippet files: one per class plus an `index.md`.
//! Per-class files are designed for inclusion via `<!--@include: ./api/ClassName.md-->`
//! so users can write custom prose above the generated API reference.

use crate::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::doc_gen::{DocField, DocFieldKind, DocNamespace, DocParam};

/// Generate markdown documentation files from doc namespaces.
///
/// Creates `{out_dir}/index.md` and `{out_dir}/{ClassName}.md` for each namespace.
pub fn generate_markdown_docs(namespaces: &[DocNamespace], out_dir: &Path) -> std::io::Result<()> {
    let output_root = out_dir.canonicalize()?;
    if !output_root.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("documentation output is not a directory: {}", out_dir.display()),
        ));
    }
    let output_files = output_filenames(namespaces)?;

    // Write per-class pages
    for ns in namespaces {
        let page = render_class_page(ns, &output_files);
        let filename = output_files.get(ns.name.as_str())
            .expect("every namespace filename was validated");
        write_output_file(&output_root, filename, page.as_bytes())?;
    }

    // Write index page
    let index = render_index(namespaces, &output_files);
    write_output_file(&output_root, "index.md", index.as_bytes())?;

    Ok(())
}

/// Build a collision-free mapping from semantic class names to safe filenames.
fn output_filenames<'a>(namespaces: &'a [DocNamespace]) -> io::Result<HashMap<&'a str, String>> {
    let mut output_files = HashMap::default();
    let mut occupied: HashSet<String> = HashSet::default();
    occupied.insert("index.md".to_string());

    for ns in namespaces {
        let filename = class_filename(&ns.name)?;
        let collision_key = filename.to_ascii_lowercase();
        if !occupied.insert(collision_key) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("class name {:?} collides with another documentation output", ns.name),
            ));
        }
        output_files.insert(ns.name.as_str(), filename);
    }

    Ok(output_files)
}

/// Convert a class name to one portable path component.
///
/// Path-like names are rejected instead of normalized. Bytes outside the
/// repository's ordinary class-name alphabet are percent-encoded so the mapping
/// remains deterministic and collision-free without changing the displayed name.
fn class_filename(class_name: &str) -> io::Result<String> {
    let bytes = class_name.as_bytes();
    let has_drive_prefix = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':';
    if class_name.is_empty()
        || class_name == "."
        || class_name == ".."
        || class_name.contains('/')
        || class_name.contains('\\')
        || has_drive_prefix
        || Path::new(class_name).is_absolute()
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("class name {class_name:?} cannot be used for documentation output"),
        ));
    }

    let mut component = String::with_capacity(class_name.len() + 3);
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            component.push(char::from(byte));
        } else {
            let _ = write!(component, "%{byte:02X}");
        }
    }

    // Windows reserves these stems even when an extension is present. Escape
    // the first byte to retain a one-to-one mapping to an ordinary file.
    if is_windows_reserved_stem(&component) {
        let first = component.remove(0);
        component.insert_str(0, &format!("%{:02X}", first as u8));
    }

    component.push_str(".md");
    Ok(component)
}

fn is_windows_reserved_stem(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    let stem = stem.to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

/// Escape the percent markers in encoded filenames for use in Markdown URLs.
fn markdown_href(filename: &str) -> String {
    filename.replace('%', "%25")
}

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

/// Replace an output entry atomically without opening or following the target.
fn write_output_file(output_root: &Path, filename: &str, contents: &[u8]) -> io::Result<()> {
    let destination = output_root.join(filename);
    let Some(parent) = destination.parent() else {
        return Err(io::Error::new(ErrorKind::InvalidInput, "output path has no parent"));
    };
    let canonical_parent = parent.canonicalize()?;
    if canonical_parent != output_root {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("documentation output escapes {}", output_root.display()),
        ));
    }

    let (temporary_path, mut temporary_file) = create_temporary_file(output_root)?;
    if let Err(error) = temporary_file.write_all(contents) {
        drop(temporary_file);
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    drop(temporary_file);

    if let Err(error) = std::fs::rename(&temporary_path, &destination) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(())
}

fn create_temporary_file(output_root: &Path) -> io::Result<(PathBuf, File)> {
    loop {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = output_root.join(format!(
            ".wowlua-doc-{}-{sequence}.tmp",
            std::process::id(),
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

/// Render the index page listing all classes.
fn render_index(namespaces: &[DocNamespace], output_files: &HashMap<&str, String>) -> String {
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
        let filename = output_files.get(ns.name.as_str())
            .expect("every namespace filename was validated");
        let _ = writeln!(
            out,
            "| [{}]({}) | {} |",
            ns.name,
            markdown_href(filename),
            escape_pipes(desc),
        );
    }
    out
}

/// Render a single class snippet (no `# Title` — meant for `<!--@include:-->` embedding).
fn render_class_page(ns: &DocNamespace, output_files: &HashMap<&str, String>) -> String {
    let mut out = String::new();

    // Inheritance — only linkify parents that have a page in the output set
    if let Some(define) = ns.defines.first()
        && !define.extends.is_empty()
    {
        let parents: Vec<String> = define.extends.iter()
            .map(|p| {
                if let Some(filename) = output_files.get(p.view.as_str()) {
                    format!("[{}]({})", p.view, markdown_href(filename))
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
    use std::sync::atomic::{AtomicU64, Ordering};

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

    fn all_classes(ns: &DocNamespace) -> HashMap<&str, String> {
        let mut files: HashMap<&str, String> = crate::collections::HashMap::default();
        files.insert(ns.name.as_str(), class_filename(&ns.name).unwrap());
        for d in &ns.defines {
            for p in &d.extends {
                files.insert(p.view.as_str(), class_filename(&p.view).unwrap());
            }
        }
        files
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
        let classes: HashMap<&str, String> = [("MyClass", "MyClass.md".to_string())]
            .into_iter()
            .collect();
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
        let output_files: HashMap<&str, String> = [("MyClass", "MyClass.md".to_string())]
            .into_iter()
            .collect();
        let index = render_index(std::slice::from_ref(&ns), &output_files);
        assert!(index.contains("[MyClass](MyClass.md)"));
    }

    #[test]
    fn path_like_class_names_are_rejected() {
        for class_name in ["..", "../escape", "/tmp/escape", "sub/file", r"sub\file", "C:escape", r"C:\escape"] {
            let error = class_filename(class_name).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "accepted {class_name:?}");
        }
    }

    #[test]
    fn class_filenames_are_portable_components() {
        assert_eq!(class_filename("AceDB.Schema-3_0").unwrap(), "AceDB.Schema-3_0.md");
        assert_eq!(class_filename("Café").unwrap(), "Caf%C3%A9.md");
        assert_eq!(class_filename("CON").unwrap(), "%43ON.md");
    }

    #[test]
    fn encoded_class_filename_is_linked_literally() {
        let mut ns = make_namespace();
        ns.name = "Café".to_string();
        let output_files = output_filenames(std::slice::from_ref(&ns)).unwrap();

        let index = render_index(std::slice::from_ref(&ns), &output_files);

        assert!(index.contains("[Café](Caf%25C3%25A9.md)"));
    }

    #[test]
    fn output_filename_collisions_are_rejected() {
        let mut first = make_namespace();
        first.name = "Foo".to_string();
        let mut second = make_namespace();
        second.name = "foo".to_string();
        assert_eq!(output_filenames(&[first, second]).unwrap_err().kind(), ErrorKind::InvalidInput);

        let mut index = make_namespace();
        index.name = "INDEX".to_string();
        assert_eq!(output_filenames(&[index]).unwrap_err().kind(), ErrorKind::InvalidInput);
    }

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    fn create_test_dir(label: &str) -> PathBuf {
        loop {
            let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "wowlua-doc-{label}-{}-{sequence}",
                std::process::id(),
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => panic!("failed to create test directory: {error}"),
            }
        }
    }

    #[test]
    fn existing_regular_output_is_replaced() {
        let output = create_test_dir("regular-output");
        std::fs::write(output.join("MyClass.md"), "old contents").unwrap();

        generate_markdown_docs(&[make_namespace()], &output).unwrap();

        let page = std::fs::read_to_string(output.join("MyClass.md")).unwrap();
        assert!(page.contains("function MyClass:GetCount()"));
        let index = std::fs::read_to_string(output.join("index.md")).unwrap();
        assert!(index.contains("[MyClass](MyClass.md)"));
        std::fs::remove_dir_all(output).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn existing_symlink_is_replaced_without_following() {
        use std::os::unix::fs::symlink;

        let base = create_test_dir("symlink-output");
        let output = base.join("output");
        std::fs::create_dir(&output).unwrap();
        let victim = base.join("victim.md");
        std::fs::write(&victim, "unchanged").unwrap();
        let generated = output.join("MyClass.md");
        symlink(&victim, &generated).unwrap();

        generate_markdown_docs(&[make_namespace()], &output).unwrap();

        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "unchanged");
        assert!(!std::fs::symlink_metadata(&generated).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&generated).unwrap().contains("function MyClass:GetCount()"));
        std::fs::remove_dir_all(base).unwrap();
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
