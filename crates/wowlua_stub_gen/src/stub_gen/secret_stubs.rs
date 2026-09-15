//! Secret values (retail 12.x): turn the secrecy keys of Blizzard's
//! `APIDocumentationGenerated` tables into `@secret-*` annotations and
//! `secret<T>` return/field/payload types on the stub text.
//!
//! Ketho's generated annotations drop every secrecy key, so the index is built
//! from the raw retail documentation and applied as a text rewrite over every
//! stub file (vendor and generated) before scanning. The classic branches carry
//! no secrecy keys, which keeps the metadata retail-only by construction.

use super::*;
use crate::secrets::{SecretArgsPolicy, SecretPredicate};

/// How a function, method, or event is annotated.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::stub_gen) struct StubSecrecy {
    /// `@secret-when` predicates with their documentation.
    pub(in crate::stub_gen) when: Vec<SecretPredicate>,
    /// `@secret-args`; Blizzard's default `AllowedWhenUntainted` is left implicit.
    pub(in crate::stub_gen) args: Option<SecretArgsPolicy>,
    /// `@secret-aspect` names.
    pub(in crate::stub_gen) aspects: Vec<String>,
    /// Returns (or payload params), in order, with whether each may be secret.
    pub(in crate::stub_gen) entries: Vec<(String, bool)>,
    /// `@secret-unless`: the unit-token parameter (position, documented name) and
    /// the tokens every predicate exempts.
    pub(in crate::stub_gen) unless: Option<(usize, String, Vec<&'static str>)>,
}

impl StubSecrecy {
    fn is_empty(&self) -> bool {
        self.when.is_empty() && self.args.is_none() && self.aspects.is_empty()
            && !self.entries.iter().any(|(_, secret)| *secret)
    }

    /// `params` are the stub's own parameter names, which can differ from the
    /// documentation's (the exemption names the parameter at the same position).
    fn annotation_lines(&self, params: &[String]) -> Vec<String> {
        let mut lines: Vec<String> = self.when.iter().map(|p| match &p.doc {
            Some(doc) => format!("---@secret-when {} {doc}", p.name),
            None => format!("---@secret-when {}", p.name),
        }).collect();
        if let Some(args) = self.args {
            lines.push(format!("---@secret-args {}", args.annotation_name()));
        }
        lines.extend(self.aspects.iter().map(|a| format!("---@secret-aspect {a}")));
        if let Some((index, doc_name, tokens)) = &self.unless {
            let param = params.get(*index).map(String::as_str).unwrap_or(doc_name);
            lines.push(format!("---@secret-unless {param} {}", tokens.join(" ")));
        }
        lines
    }

    /// Whether the entry named `name` (falling back to position `index`) may be secret.
    fn entry_is_secret(&self, name: Option<&str>, index: usize) -> bool {
        match name.and_then(|n| self.entries.iter().find(|(e, _)| e == n)) {
            Some((_, secret)) => *secret,
            None => self.entries.get(index).is_some_and(|(_, secret)| *secret),
        }
    }
}

/// Secrecy of every documented function, method (`Class:Method`), event, and
/// structure field.
#[derive(Debug, Default)]
pub(in crate::stub_gen) struct SecretIndex {
    pub(in crate::stub_gen) functions: HashMap<String, StubSecrecy>,
    pub(in crate::stub_gen) events: HashMap<String, StubSecrecy>,
    /// Structure name → names of fields that may be secret.
    pub(in crate::stub_gen) structures: HashMap<String, HashSet<String>>,
}

impl SecretIndex {
    pub(in crate::stub_gen) fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.events.is_empty() && self.structures.is_empty()
    }
}

/// Resolves which entry-level flags are secret predicates.
struct PredicateTable<'a> {
    secret: HashMap<&'a str, Option<&'a str>>,
    other: HashSet<&'a str>,
}

impl<'a> PredicateTable<'a> {
    fn new(docs: &'a BlizzardApiDocs) -> Self {
        let mut secret = HashMap::new();
        let mut other = HashSet::new();
        for p in &docs.predicates {
            if p.kind == "Secret" {
                secret.insert(p.name.as_str(), p.documentation.as_deref());
            } else {
                other.insert(p.name.as_str());
            }
        }
        Self { secret, other }
    }

    /// `Some(doc)` when `flag` names a secret predicate. Flags used without a
    /// `Predicates` definition (e.g. `SecretWhenCurveSecret`) are recognized by
    /// Blizzard's `Secret{When,In,On}…` naming convention, without documentation.
    fn secret_predicate(&self, flag: &str) -> Option<Option<String>> {
        if let Some(doc) = self.secret.get(flag) {
            return Some(doc.map(str::to_string));
        }
        let conventional = ["SecretWhen", "SecretIn", "SecretOn"].iter().any(|prefix| {
            flag.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_uppercase()))
        });
        (conventional && !self.other.contains(flag)).then_some(None)
    }
}

/// Unit tokens a unit-scoped secret predicate never restricts, per its
/// documentation. Predicates whose docs leave any exception (e.g. "individual
/// spells may be flagged as always secret") aren't listed and exempt nothing.
const PREDICATE_EXEMPT_UNITS: &[(&str, &[&str])] = &[
    // "when the unit isn't player-controlled or in the party/raid"
    ("SecretWhenUnitIdentityRestricted", &["player", "pet"]),
    // "under regular unit identity secrecy rules, except in PvP when the queried unit is a player"
    ("SecretWhenUnitNameIdentityRestricted", &["player", "pet"]),
    // "when the unit isn't player-controlled"
    ("SecretWhenUnitHealthMaxRestricted", &["player", "pet"]),
    // "if the subject unit is not the active player"
    ("SecretWhenLossOfControlInfoRestricted", &["player"]),
    // "except for unit tokens under the player's direct control"
    ("SecretWhenUnitPossessionRestricted", &["player", "pet"]),
];

/// The `@secret-unless` exemption for a function whose secrecy comes only from
/// unit-scoped predicates: its first unit-token parameter and the tokens all of
/// those predicates exempt.
fn unit_exemption(
    when: &[SecretPredicate],
    secrecy: &EntrySecrecy,
    arguments: &[BlizzardParam],
    returns: &[BlizzardParam],
) -> Option<(usize, String, Vec<&'static str>)> {
    if when.is_empty() || secrecy.flags.iter().any(|f| f == "SecretReturns") || returns.iter().any(|r| r.secrecy.value) {
        return None;
    }
    let unit_index = arguments.iter().position(|a| a.type_name.starts_with("UnitToken"))?;
    let mut tokens: Option<Vec<&'static str>> = None;
    for predicate in when {
        let exempt = PREDICATE_EXEMPT_UNITS.iter().find(|(name, _)| *name == predicate.name)?.1;
        tokens = Some(match tokens {
            None => exempt.to_vec(),
            Some(prev) => prev.into_iter().filter(|t| exempt.contains(t)).collect(),
        });
    }
    tokens.filter(|t| !t.is_empty()).map(|t| (unit_index, arguments[unit_index].name.clone(), t))
}

/// Values that are tables or objects at runtime; only their *fields* can be secret.
fn is_table_like(p: &BlizzardParam, structures: &HashSet<&str>, table_types: &HashSet<String>) -> bool {
    p.type_name == "table"
        || p.mixin.is_some()
        || structures.contains(p.type_name.as_str())
        || table_types.contains(&p.type_name)
}

/// Build the index from the retail docs. `table_types` names every `@class` the
/// stubs declare (structures the docs reference without describing field by field,
/// widgets, Lua objects) so such values aren't wrapped as scalars.
pub(in crate::stub_gen) fn build_secret_index(docs: &BlizzardApiDocs, table_types: &HashSet<String>) -> SecretIndex {
    let predicates = PredicateTable::new(docs);
    let structures: HashSet<&str> = docs.structures.iter().map(|s| s.name.as_str()).collect();
    let mut index = SecretIndex::default();
    // Structures reached through a secret return/payload; their fields get marked below.
    let mut secret_structs: Vec<String> = Vec::new();

    // `taint` = whether the entries' own types become `secret<T>`. Widget
    // methods keep their metadata but not the taint: a widget getter returns
    // secrets only for objects that were fed secret values.
    let mut entry_secrecy = |secrecy: &EntrySecrecy, arguments: &[BlizzardParam], entries: &[BlizzardParam], all_key: &str, taint: bool| {
        let when: Vec<SecretPredicate> = secrecy.flags.iter()
            .filter_map(|f| predicates.secret_predicate(f).map(|doc| SecretPredicate { name: f.clone(), doc }))
            .collect();
        let all = !when.is_empty() || secrecy.flags.iter().any(|f| f == all_key);
        let never_all = secrecy.flags.iter().any(|f| f == "ReturnsNeverSecret");
        let mut out_entries = Vec::with_capacity(entries.len());
        for e in entries {
            let maybe = taint && !never_all && !e.secrecy.never && (all || e.secrecy.conditional || e.secrecy.value);
            let table_like = is_table_like(e, &structures, table_types);
            if (maybe && !e.secrecy.never_contents) || (taint && e.secrecy.secret_contents) {
                for name in [Some(&e.type_name), e.inner_type.as_ref()].into_iter().flatten() {
                    if structures.contains(name.as_str()) {
                        secret_structs.push(name.clone());
                    }
                }
            }
            out_entries.push((e.name.clone(), maybe && !table_like));
        }
        let unless = unit_exemption(&when, secrecy, arguments, entries);
        StubSecrecy {
            when,
            args: secrecy.arguments.as_deref()
                .and_then(SecretArgsPolicy::from_blizzard_name)
                .filter(|p| *p != SecretArgsPolicy::AllowedWhenUntainted),
            aspects: secrecy.aspects.clone(),
            entries: out_entries,
            unless,
        }
    };

    for func in &docs.functions {
        let s = entry_secrecy(&func.secrecy, &func.arguments, &func.returns, "SecretReturns", true);
        if !s.is_empty() {
            let key = match &func.namespace {
                Some(ns) => format!("{ns}.{}", func.name),
                None => func.name.clone(),
            };
            index.functions.insert(key, s);
        }
    }
    for api in &docs.script_objects {
        let Some(class) = SCRIPTOBJECT_CLASS_MAP.iter().find(|(name, _)| *name == api.name).map(|(_, c)| *c) else {
            continue;
        };
        for func in &api.functions {
            let s = entry_secrecy(&func.secrecy, &func.arguments, &func.returns, "SecretReturns", false);
            if !s.is_empty() {
                index.functions.insert(format!("{class}:{}", func.name), s);
            }
        }
    }
    for event in &docs.events {
        let s = entry_secrecy(&event.secrecy, &[], &event.payload, "SecretPayloads", true);
        if !s.is_empty() {
            index.events.insert(event.literal_name.clone(), s);
        }
    }

    // Mark structure fields. A reached structure's non-`NeverSecret` scalar fields
    // may be secret (nested structures are reached in turn); `SecretValue` /
    // `ConditionalSecret` fields may be secret in any structure.
    let by_name: HashMap<&str, &BlizzardStructure> = docs.structures.iter().map(|s| (s.name.as_str(), s)).collect();
    let mut reached: HashSet<String> = HashSet::new();
    while let Some(name) = secret_structs.pop() {
        if !reached.insert(name.clone()) { continue; }
        let Some(st) = by_name.get(name.as_str()) else { continue };
        for f in &st.fields {
            if f.secrecy.never { continue; }
            for nested in [Some(&f.type_name), f.inner_type.as_ref()].into_iter().flatten() {
                if structures.contains(nested.as_str()) && !f.secrecy.never_contents {
                    secret_structs.push(nested.clone());
                }
            }
        }
    }
    for st in &docs.structures {
        let fields: HashSet<String> = st.fields.iter()
            .filter(|f| (reached.contains(&st.name) && !f.secrecy.never) || f.secrecy.conditional || f.secrecy.value)
            .filter(|f| !is_table_like(f, &structures, table_types))
            .map(|f| f.name.clone())
            .collect();
        if !fields.is_empty() {
            index.structures.insert(st.name.clone(), fields);
        }
    }

    log::info!(
        "  Secret values: {} functions/methods, {} events, {} structures annotated",
        index.functions.len(), index.events.len(), index.structures.len(),
    );
    index
}

/// `T` → `secret<T>` (a trailing `?` stays outermost). Container and function
/// types are left alone.
pub(in crate::stub_gen) fn wrap_secret_type(ty: &str) -> String {
    let (base, optional) = match ty.strip_suffix('?') {
        Some(b) => (b, "?"),
        None => (ty, ""),
    };
    if base.contains("secret<") || base.starts_with("table") || base.ends_with("[]") || base.starts_with("fun(") {
        return ty.to_string();
    }
    format!("secret<{base}>{optional}")
}

/// Rewrite the type token of a `---@return T name` line.
fn rewrite_return_line(line: &str, secrecy: &StubSecrecy, index: usize) -> String {
    let Some(rest) = line.strip_prefix("---@return ") else { return line.to_string() };
    let mut parts = rest.splitn(3, ' ');
    let Some(ty) = parts.next() else { return line.to_string() };
    let name = parts.next();
    if !secrecy.entry_is_secret(name, index) {
        return line.to_string();
    }
    let tail = &rest[ty.len()..];
    format!("---@return {}{tail}", wrap_secret_type(ty))
}

/// Rewrite the type token of a `---@param name T` / `---@field name T` line whose
/// name is selected by `is_secret`.
fn rewrite_named_line(line: &str, tag: &str, is_secret: impl Fn(&str) -> bool) -> String {
    let Some(rest) = line.strip_prefix(tag) else { return line.to_string() };
    let mut parts = rest.splitn(3, ' ');
    let (Some(name), Some(ty)) = (parts.next(), parts.next()) else { return line.to_string() };
    if !is_secret(name.trim_end_matches('?')) {
        return line.to_string();
    }
    let tail = &rest[name.len() + 1 + ty.len()..];
    format!("{tag}{name} {}{tail}", wrap_secret_type(ty))
}

/// The `NAME` and parameter names of a `function NAME(a, b) end` line
/// (`NAME` may be `A.B` or `A:B`).
fn function_line_signature(line: &str) -> Option<(&str, Vec<String>)> {
    let rest = line.strip_prefix("function ")?;
    let open = rest.find('(')?;
    let name = &rest[..open];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ':')) {
        return None;
    }
    let params = parse_param_list(rest[open + 1..].split(')').next().unwrap_or(""));
    Some((name, params))
}

/// Apply the index to one stub file. Returns `None` when nothing changed.
pub(in crate::stub_gen) fn apply_secret_annotations(text: &str, index: &SecretIndex) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    let mut changed = false;
    // Start of the current `---` annotation block within `out`.
    let mut block_start = 0;
    let mut struct_fields: Option<&HashSet<String>> = None;
    let mut event: Option<&StubSecrecy> = None;
    for line in text.lines() {
        if let Some((name, params)) = function_line_signature(line) {
            if let Some(secrecy) = index.functions.get(name) {
                let mut return_index = 0;
                for entry in &mut out[block_start..] {
                    if entry.starts_with("---@return ") {
                        *entry = rewrite_return_line(entry, secrecy, return_index);
                        return_index += 1;
                    }
                }
                out.extend(secrecy.annotation_lines(&params));
                changed = true;
            }
            out.push(line.to_string());
            block_start = out.len();
            struct_fields = None;
            event = None;
            continue;
        }
        if !line.starts_with("---") {
            out.push(line.to_string());
            block_start = out.len();
            struct_fields = None;
            event = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("---@class ") {
            let name = rest.split(|c: char| c.is_whitespace() || c == ':').next().unwrap_or("");
            struct_fields = index.structures.get(name);
            out.push(line.to_string());
        } else if let Some(fields) = struct_fields.filter(|_| line.starts_with("---@field ")) {
            let rewritten = rewrite_named_line(line, "---@field ", |n| fields.contains(n));
            changed |= rewritten != line;
            out.push(rewritten);
        } else if let Some(rest) = line.strip_prefix("---@event ") {
            let literal = rest.split_whitespace().nth(1).map(|s| s.trim_matches('"'));
            event = literal.and_then(|l| index.events.get(l));
            out.push(line.to_string());
            if let Some(secrecy) = event {
                out.extend(secrecy.annotation_lines(&[]));
                changed = true;
            }
        } else if let Some(secrecy) = event.filter(|_| line.starts_with("---@param ")) {
            let rewritten = rewrite_named_line(line, "---@param ", |n| secrecy.entry_is_secret(Some(n), usize::MAX));
            changed |= rewritten != line;
            out.push(rewritten);
        } else {
            out.push(line.to_string());
        }
    }
    if !changed {
        return None;
    }
    let mut result = out.join("\n");
    if text.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

/// Rewrite every `.lua` stub file under `dir` in place.
pub(in crate::stub_gen) fn apply_secret_annotations_to_dir(dir: &Path, index: &SecretIndex) {
    let mut paths = Vec::new();
    collect_lua_paths(dir, &mut paths);
    let mut rewritten = 0usize;
    for path in &paths {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        if let Some(new_text) = apply_secret_annotations(&text, index) {
            if let Err(e) = std::fs::write(path, new_text) {
                log::warn!("Failed to write secret annotations to {}: {e}", path.display());
            } else {
                rewritten += 1;
            }
        }
    }
    log::info!("  Secret values: annotated {rewritten} stub file(s) under {}", dir.display());
}
