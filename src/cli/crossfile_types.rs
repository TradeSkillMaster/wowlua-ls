//! `crossfile-types` subcommand: group a workspace's global and field reads and
//! writes by the path they use and list the paths whose type differs between
//! files. A type lost between the file writing a field and a file reading it
//! shows as `any` there, which removes diagnostics instead of adding one, so
//! neither a `check` diff nor a `dump-types` diff can show a loss that already
//! exists.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::error;
use rayon::prelude::*;

use wowlua_ls::analysis::queries::{ext_table_paths, FieldPath, PathRoot, TablePaths, Unkeyed};
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::{config, syntax};

use super::{analyze_file, collect_lua_files, load_workspace, CliResult};

/// An access's path, with the addon namespace told apart per addon root.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    addon_root: Option<PathBuf>,
    path: FieldPath,
}

impl Key {
    /// The last name in the path.
    fn own_name(&self) -> Option<&str> {
        match (self.path.fields.last(), &self.path.root) {
            (Some(field), _) => Some(field.as_str()).filter(|f| *f != "[]"),
            (None, PathRoot::Global(name) | PathRoot::Class(name)) => Some(name),
            (None, PathRoot::AddonNs) => None,
        }
    }

    fn parent(&self) -> Option<Key> {
        let mut parent = self.clone();
        parent.path.fields.pop()?;
        Some(parent)
    }

    fn render(&self) -> String {
        let path = self.path.to_string();
        match self.addon_root.as_deref().and_then(Path::file_name) {
            Some(name) => path.replacen("<ns>", &format!("<ns:{}>", name.to_string_lossy()), 1),
            None => path,
        }
    }
}

/// Where an access is: an index into the workspace's sorted files, its 1-based
/// line and column, and whether it writes the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Site {
    file: usize,
    line: usize,
    col: usize,
    write: bool,
}

struct Access {
    site: Site,
    path: Result<Key, Unkeyed>,
    display: String,
}

/// How the types a path shows in different files disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disagreement {
    /// Typed in some files; `any`, `nil` or not found in others.
    Lost,
    /// Different types.
    Differs,
    /// The same type, optional in some files only.
    Nilable,
    /// The same function, some of its parameters typed in some files only.
    ParamTypes,
    /// The same method, its `self` parameter typed in some files only.
    SelfType,
}

impl Disagreement {
    fn label(self) -> &'static str {
        match self {
            Disagreement::Lost => "untyped in some files",
            Disagreement::Differs => "typed differently",
            Disagreement::Nilable => "optional in some files",
            Disagreement::ParamTypes => "parameters typed in some files",
            Disagreement::SelfType => "self typed in some files",
        }
    }

    /// Lost and differing types rank together, ahead of the milder kinds.
    fn rank_group(self) -> u8 {
        match self {
            Disagreement::Lost | Disagreement::Differs => 0,
            Disagreement::Nilable => 1,
            Disagreement::ParamTypes => 2,
            Disagreement::SelfType => 3,
        }
    }

    /// Function signature kinds share a cause per kind, so one line lists them.
    fn is_listed_compactly(self) -> bool {
        matches!(self, Disagreement::ParamTypes | Disagreement::SelfType)
    }
}

#[derive(Debug)]
struct Finding {
    kind: Disagreement,
    key: Key,
    /// Each type the path shows, with its accesses.
    displays: BTreeMap<String, Vec<Site>>,
    /// The accesses a fix would change: the untyped ones for a lost type, the
    /// ones with fewer typed parameters for a signature, else the ones outside
    /// the most common type.
    affected: usize,
    /// Lost or differing paths under this one, which likely follow from it.
    under: Vec<Finding>,
}

impl Finding {
    fn total_affected(&self) -> usize {
        self.affected + self.under.iter().map(Finding::total_affected).sum::<usize>()
    }
}

#[derive(Default)]
struct Report {
    accesses: usize,
    unkeyed: BTreeMap<Unkeyed, usize>,
    /// Accesses on, and the number of, the paths more than one file uses.
    cross_file_accesses: usize,
    cross_file_paths: usize,
    disagreeing: usize,
    /// The disagreeing paths no other one is folded into, most affected first.
    findings: Vec<Finding>,
}

fn is_untyped(display: &str) -> bool {
    matches!(display, "any" | "nil" | "<none>")
}

/// A display with an unknown type spelled one way: an unresolved `?` and an
/// optional `any?` are `any`, and a parameter typed `any` is an untyped one.
fn normalize(display: &str) -> String {
    let mut out = String::with_capacity(display.len());
    for c in display.chars() {
        if c == '?' {
            let ends_type = |s: &str| s.ends_with(|p: char| p.is_alphanumeric() || "_])}>\"…".contains(p));
            if !ends_type(&out) {
                out.push_str("any");
                continue;
            }
            if out.ends_with("any") && !ends_type(&out[..out.len() - 3]) {
                continue;
            }
        }
        out.push(c);
    }
    untype_any_params(&out)
}

/// `display` with every signature's `name: any` parameters, nested ones too,
/// written as bare names.
fn untype_any_params(display: &str) -> String {
    let mut out = String::with_capacity(display.len());
    let mut rest = display;
    while let Some(i) = rest.find("fun(") {
        out.push_str(&rest[..i + 4]);
        let Some((params, after)) = split_params(&rest[i + 4..]) else {
            rest = &rest[i + 4..];
            break;
        };
        let params: Vec<String> = params.iter().map(|p| {
            let p = untype_any_params(p);
            p.strip_suffix(": any").map_or(p.clone(), str::to_string)
        }).collect();
        out.push_str(&params.join(", "));
        out.push(')');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The parameters (`name: type`) of a signature whose `fun(` precedes `params`,
/// and what follows their `)`.
fn split_params(params: &str) -> Option<(Vec<&str>, &str)> {
    let mut depth = 0usize;
    let mut start = 0;
    let mut out = Vec::new();
    for (i, c) in params.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' if depth == 0 => {
                let last = params[start..i].trim();
                if !last.is_empty() || !out.is_empty() { out.push(last); }
                return Some((out, &params[i + 1..]));
            }
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(params[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    None
}

/// A function signature's parameters and what follows them.
fn split_signature(display: &str) -> Option<(Vec<&str>, &str)> {
    split_params(display.strip_prefix("fun(")?)
}

/// How many of a function signature's parameters have a type.
fn typed_params(display: &str) -> usize {
    split_signature(display).map_or(0, |(params, _)| params.iter().filter(|p| p.contains(':')).count())
}

/// Parameters as `(name, type)`, and what follows them.
type Signature<'a> = (Vec<(&'a str, Option<String>)>, &'a str);

/// A function signature's parameters as names and types (an optional
/// parameter's `?` moved to its type), and what follows them.
fn signature_params(display: &str) -> Option<Signature<'_>> {
    let (params, rest) = split_signature(display)?;
    let params = params.into_iter().map(|p| match p.split_once(':') {
        Some((name, ty)) => {
            let name = name.trim();
            let optional = if name.ends_with('?') { "?" } else { "" };
            (name.trim_end_matches('?'), Some(format!("{optional}{}", ty.trim())))
        }
        None => (p.trim_end_matches('?'), None),
    }).collect();
    Some((params, rest))
}

/// When the signatures differ only in which parameters they type (the most
/// typed one types every parameter another does, the same way), whether
/// `self` is the only parameter some leave untyped.
fn untyped_params_only(displays: &[&str]) -> Option<bool> {
    let sigs: Vec<Signature<'_>> = displays.iter()
        .map(|d| signature_params(d))
        .collect::<Option<_>>()?;
    let (most, most_rest) = sigs.iter().max_by_key(|(params, _)| params.iter().filter(|p| p.1.is_some()).count())?;
    let (mut any_untyped, mut only_self) = (false, true);
    for (params, rest) in &sigs {
        if rest != most_rest || params.len() != most.len() { return None; }
        for ((name, ty), (most_name, most_ty)) in params.iter().zip(most) {
            if name != most_name { return None; }
            match ty {
                Some(_) if ty != most_ty => return None,
                None if most_ty.is_some() => {
                    any_untyped = true;
                    only_self &= *name == "self";
                }
                _ => {}
            }
        }
    }
    any_untyped.then_some(only_self)
}

/// `display` without its `nil` members.
fn without_nil(display: &str) -> String {
    display.split(" | ")
        .filter(|m| *m != "nil")
        .map(|m| m.strip_suffix('?').unwrap_or(m))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn classify(displays: &BTreeMap<String, Vec<Site>>) -> Option<Disagreement> {
    if displays.len() < 2 || displays.keys().all(|d| is_untyped(d)) {
        return None;
    }
    if displays.keys().any(|d| is_untyped(d)) {
        return Some(Disagreement::Lost);
    }
    let keys: Vec<&str> = displays.keys().map(String::as_str).collect();
    let mut stripped = keys.iter().map(|d| without_nil(d));
    let first = stripped.next();
    Some(match untyped_params_only(&keys) {
        Some(true) => Disagreement::SelfType,
        Some(false) => Disagreement::ParamTypes,
        None if stripped.all(|d| Some(&d) == first.as_ref()) => Disagreement::Nilable,
        None => Disagreement::Differs,
    })
}

/// How many accesses a fix of a `kind` disagreement would change.
fn affected(kind: Disagreement, displays: &BTreeMap<String, Vec<Site>>) -> usize {
    let accesses_where = |keep: &dyn Fn(&str) -> bool| -> usize {
        displays.iter().filter(|(d, _)| keep(d)).map(|(_, sites)| sites.len()).sum()
    };
    match kind {
        Disagreement::Lost => accesses_where(&|d| is_untyped(d)),
        Disagreement::ParamTypes | Disagreement::SelfType => {
            let most = displays.keys().map(|d| typed_params(d)).max().unwrap_or(0);
            accesses_where(&|d| typed_params(d) < most)
        }
        Disagreement::Differs | Disagreement::Nilable => {
            let common = displays.values().map(Vec::len).max().unwrap_or(0);
            accesses_where(&|_| true) - common
        }
    }
}

fn unkeyed_label(kind: Unkeyed) -> &'static str {
    match kind {
        Unkeyed::Untyped => "untyped",
        Unkeyed::FileLocal => "file-local",
        Unkeyed::BuiltIn => "built-in",
        Unkeyed::Unnamed => "unnamed",
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn file_accesses(
    path: &Path,
    file: usize,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &config::ProjectConfigs,
    ext_paths: &TablePaths,
) -> Vec<Access> {
    let Ok(text) = syntax::read_source_file(path) else { return Vec::new() };
    if wowlua_ls::has_shebang(&text) { return Vec::new(); }
    let tree = syntax::parser::parse(&text);
    let accesses = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut result = analyze_file(&tree, pre_globals, configs, path);
        result.adopt_write_root_versions();
        result.path_accesses(&tree, ext_paths)
    }));
    let Ok(accesses) = accesses else {
        error!("PANIC: {}", path.display());
        return Vec::new();
    };
    let numbers = line_numbers::LinePositions::from(text.as_str());
    let addon_root = configs.addon_root_for(path).map(Path::to_path_buf);
    accesses.into_iter().map(|access| {
        let pos = numbers.from_offset(access.offset as usize);
        let site = Site { file, line: pos.0.0 as usize + 1, col: pos.1 + 1, write: access.write };
        let path = access.path.map(|path| Key {
            addon_root: matches!(path.root, PathRoot::AddonNs).then(|| addon_root.clone()).flatten(),
            path,
        });
        Access { site, path, display: normalize(&access.display) }
    }).collect()
}

/// Group the accesses by path and find the paths whose type differs between files.
fn build_report(accesses: Vec<Access>) -> Report {
    let mut report = Report { accesses: accesses.len(), ..Report::default() };
    let mut by_path: BTreeMap<Key, BTreeMap<String, Vec<Site>>> = BTreeMap::new();
    for access in accesses {
        match access.path {
            // An empty table written says nothing of the shape other files give it.
            Ok(_) if access.site.write && access.display == "table" => {}
            Ok(key) => {
                // The class the workspace scan names after a global or field
                // holding a table (`FooMixin = {}`) is the shape its defining
                // file shows.
                let display = if key.own_name() == Some(access.display.as_str()) { "{…}".to_string() } else { access.display };
                by_path.entry(key).or_default().entry(display).or_default().push(access.site);
            }
            Err(kind) => *report.unkeyed.entry(kind).or_default() += 1,
        }
    }

    // Keyed by path, so a path comes before the paths under it.
    let mut findings: BTreeMap<Key, Finding> = BTreeMap::new();
    for (key, displays) in by_path {
        let mut used_in: Vec<usize> = displays.values().flatten().map(|s| s.file).collect();
        used_in.sort_unstable();
        used_in.dedup();
        if used_in.len() < 2 { continue; }
        report.cross_file_paths += 1;
        report.cross_file_accesses += displays.values().map(Vec::len).sum::<usize>();
        let Some(kind) = classify(&displays) else { continue };
        let affected = affected(kind, &displays);
        findings.insert(key.clone(), Finding { kind, key, displays, affected, under: Vec::new() });
    }
    report.disagreeing = findings.len();

    // Fold a lost or differing path into the nearest one above it.
    let mut top: BTreeMap<Key, Finding> = BTreeMap::new();
    for (key, finding) in findings {
        let mut ancestor = if finding.kind.rank_group() == 0 { key.parent() } else { None };
        let mut finding = Some(finding);
        while let Some(a) = ancestor {
            if let Some(parent) = top.get_mut(&a).filter(|p| p.kind.rank_group() == 0) {
                parent.under.extend(finding.take());
                break;
            }
            ancestor = a.parent();
        }
        if let Some(finding) = finding {
            top.insert(key, finding);
        }
    }
    report.findings = top.into_values().collect();
    report.findings.sort_by(|a, b| a.kind.rank_group().cmp(&b.kind.rank_group())
        .then(b.total_affected().cmp(&a.total_affected()))
        .then_with(|| a.key.cmp(&b.key)));
    report
}

fn print_report(out: &mut impl Write, dir: &Path, files: &[PathBuf], report: &Report) -> std::io::Result<()> {
    let site_str = |s: &Site| {
        let name = files[s.file].strip_prefix(dir).unwrap_or(&files[s.file]);
        format!("{}:{}:{}{}", name.display(), s.line, s.col, if s.write { " (write)" } else { "" })
    };
    writeln!(out, "{}: {} in {}; {} of them on {} used by more than one file, {} of which {}",
        dir.display(), plural(report.accesses, "global or field access", "global and field accesses"),
        plural(files.len(), "file", "files"), report.cross_file_accesses,
        plural(report.cross_file_paths, "path", "paths"), report.disagreeing,
        if report.disagreeing == 1 { "disagrees" } else { "disagree" })?;
    let unkeyed: usize = report.unkeyed.values().sum();
    if unkeyed > 0 {
        let parts: Vec<String> = report.unkeyed.iter().map(|(k, n)| format!("{} {}", unkeyed_label(*k), n)).collect();
        writeln!(out, "  {unkeyed} have no path, their receiver being {}", parts.join(", "))?;
    }
    for kind in [Disagreement::ParamTypes, Disagreement::SelfType] {
        let listed: Vec<&Finding> = report.findings.iter().filter(|f| f.kind == kind).collect();
        if listed.is_empty() { continue; }
        let names: Vec<String> = listed.iter().take(8).map(|f| format!("{} ×{}", f.key.render(), f.affected)).collect();
        let more = if listed.len() > 8 { ", …" } else { "" };
        writeln!(out, "  {} with {}: {}{}", plural(listed.len(), "function", "functions"), kind.label(), names.join(", "), more)?;
    }
    for finding in report.findings.iter().filter(|f| !f.kind.is_listed_compactly()) {
        writeln!(out, "\n{}  [{}]", finding.key.render(), finding.kind.label())?;
        let mut rows: Vec<(&String, &Vec<Site>)> = finding.displays.iter().collect();
        rows.sort_by(|a, b| is_untyped(a.0).cmp(&is_untyped(b.0))
            .then(b.1.len().cmp(&a.1.len()))
            .then(a.0.cmp(b.0)));
        for (display, sites) in rows {
            let mut first_per_file: Vec<Site> = Vec::new();
            for site in sites {
                if first_per_file.last().is_none_or(|s| s.file != site.file) {
                    first_per_file.push(*site);
                }
            }
            let shown: Vec<String> = first_per_file.iter().take(3).map(site_str).collect();
            let more = match first_per_file.len() {
                n if n > 3 => format!(", +{} files", n - 3),
                _ => String::new(),
            };
            writeln!(out, "    {}  ×{} in {}: {}{}", display, sites.len(),
                plural(first_per_file.len(), "file", "files"), shown.join(", "), more)?;
        }
        if !finding.under.is_empty() {
            let mut under: Vec<&Finding> = Vec::new();
            let mut stack: Vec<&Finding> = finding.under.iter().collect();
            while let Some(f) = stack.pop() {
                under.push(f);
                stack.extend(f.under.iter());
            }
            under.sort_by(|a, b| b.affected.cmp(&a.affected).then_with(|| a.key.cmp(&b.key)));
            let prefix = finding.key.render();
            let names: Vec<String> = under.iter().take(5).map(|f| {
                let path = f.key.render();
                format!("{} ×{}", path.strip_prefix(&prefix).unwrap_or(&path), f.affected)
            }).collect();
            let more = if under.len() > 5 { ", …" } else { "" };
            writeln!(out, "    and {} under it: {}{}", plural(under.len(), "path", "paths"), names.join(", "), more)?;
        }
    }
    Ok(())
}

pub fn run(dirs: Vec<PathBuf>, list_accesses: bool) -> CliResult {
    for dir in &dirs {
        if !dir.is_dir() {
            error!("Not a directory: {}", dir.display());
            std::process::exit(1);
        }
    }
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for (i, dir) in dirs.iter().enumerate() {
        if i > 0 { writeln!(out)?; }
        let mut configs = config::ProjectConfigs::default();
        let ws = load_workspace(std::slice::from_ref(dir), &mut configs, true, false, true);
        let ext_paths = ext_table_paths(&ws.pre_globals);
        let mut files = Vec::new();
        collect_lua_files(dir, &mut files, &configs, false);
        files.sort();
        let accesses: Vec<Access> = files.par_iter().enumerate()
            .flat_map_iter(|(i, path)| file_accesses(path, i, &ws.pre_globals, &configs, &ext_paths))
            .collect();
        if list_accesses {
            for access in &accesses {
                let name = files[access.site.file].strip_prefix(dir).unwrap_or(&files[access.site.file]);
                let path = match &access.path {
                    Ok(key) => key.render(),
                    Err(kind) => format!("({})", unkeyed_label(*kind)),
                };
                writeln!(out, "{}:{}:{}{} {} → {}", name.display(), access.site.line, access.site.col,
                    if access.site.write { " (write)" } else { "" }, path, access.display)?;
            }
            writeln!(out)?;
        }
        print_report(&mut out, dir, &files, &build_report(accesses))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fields: &[&str]) -> Key {
        Key {
            addon_root: None,
            path: FieldPath { root: PathRoot::Global("Root".into()), fields: fields.iter().map(|f| f.to_string()).collect() },
        }
    }

    fn access(file: usize, fields: &[&str], display: &str) -> Access {
        Access { site: Site { file, line: 1, col: 1, write: false }, path: Ok(key(fields)), display: normalize(display) }
    }

    fn kinds(displays: &[&str]) -> Option<Disagreement> {
        let map = displays.iter().enumerate()
            .map(|(i, d)| (normalize(d), vec![Site { file: i, line: 1, col: 1, write: false }]))
            .collect();
        classify(&map)
    }

    #[test]
    fn unknown_types_are_spelled_one_way() {
        assert_eq!(normalize("?"), "any");
        assert_eq!(normalize("any?"), "any");
        assert_eq!(normalize("string?"), "string?");
        assert_eq!(normalize("fun(a: any, b?: string): ?"), "fun(a, b?: string): any");
        assert_eq!(normalize("fun(self): fun(owner: any, desc: any)"), "fun(self): fun(owner, desc)");
        assert_eq!(normalize("fun(cb: fun(x: any): number)"), "fun(cb: fun(x): number)");
    }

    #[test]
    fn disagreements_are_classified() {
        assert_eq!(kinds(&["number[]", "table"]), Some(Disagreement::Differs));
        assert_eq!(kinds(&["Settings", "any"]), Some(Disagreement::Lost));
        assert_eq!(kinds(&["fun(): boolean", "<none>"]), Some(Disagreement::Lost));
        assert_eq!(kinds(&["any", "<none>"]), None);
        assert_eq!(kinds(&["string", "string?"]), Some(Disagreement::Nilable));
        assert_eq!(kinds(&["fun(name: string, value)", "fun(name, value)"]), Some(Disagreement::ParamTypes));
        assert_eq!(kinds(&["fun(self: Ledger): number", "fun(self): number"]), Some(Disagreement::SelfType));
        assert_eq!(kinds(&["fun(self): Frame", "fun(self): Frame & {…}"]), Some(Disagreement::Differs));
        assert_eq!(kinds(&["fun(): table", "fun(): ?"]), Some(Disagreement::Differs));
        // A parameter typed differently, not just untyped somewhere, is a real difference.
        assert_eq!(kinds(&["fun(map: fun(value: T): Schema)", "fun(map: function)"]), Some(Disagreement::Differs));
        assert_eq!(kinds(&["fun(self: A)", "fun(self: B)"]), Some(Disagreement::Differs));
        assert_eq!(kinds(&["fun(a?: string)", "fun(a: string)"]), Some(Disagreement::Differs));
        assert_eq!(kinds(&["fun(self: table<string, number>, x)", "fun(self, x)"]), Some(Disagreement::SelfType));
        assert_eq!(kinds(&["fun(self: table<string, number>, x: number)", "fun(self, x)"]), Some(Disagreement::ParamTypes));
    }

    #[test]
    fn parameters_untyped_in_some_files_count_those_accesses() {
        let displays = ["fun(self: Ledger, n: number)", "fun(self, n: number)", "fun(self, n: number)"].iter().enumerate()
            .map(|(i, d)| (d.to_string(), vec![Site { file: i, line: 1, col: 1, write: false }]))
            .fold(BTreeMap::<String, Vec<Site>>::new(), |mut map, (d, sites)| {
                map.entry(d).or_default().extend(sites);
                map
            });
        assert_eq!(classify(&displays), Some(Disagreement::SelfType));
        assert_eq!(affected(Disagreement::SelfType, &displays), 2);
    }

    #[test]
    fn paths_under_a_disagreeing_one_fold_into_it() {
        let report = build_report(vec![
            access(0, &["db"], "{…}"),
            access(1, &["db"], "table"),
            access(0, &["db", "scale"], "number"),
            access(1, &["db", "scale"], "<none>"),
            access(1, &["db", "scale"], "<none>"),
            access(0, &["other"], "string"),
            access(1, &["other"], "any"),
            access(0, &["same"], "string"),
            access(1, &["same"], "string"),
            access(0, &["one_file"], "string"),
            access(0, &["one_file"], "any"),
        ]);
        assert_eq!((report.accesses, report.cross_file_paths, report.disagreeing), (11, 4, 3));
        let top: Vec<(String, usize)> = report.findings.iter().map(|f| (f.key.render(), f.total_affected())).collect();
        assert_eq!(top, [("Root.db".to_string(), 3), ("Root.other".to_string(), 1)]);
        assert_eq!(report.findings[0].under[0].key.render(), "Root.db.scale");
    }

    #[test]
    fn an_empty_table_written_is_no_disagreement() {
        let mut write = access(0, &["module"], "table");
        write.site.write = true;
        let report = build_report(vec![write, access(1, &["module"], "{…}"), access(2, &["module"], "{…}")]);
        assert_eq!(report.disagreeing, 0);
    }
}
