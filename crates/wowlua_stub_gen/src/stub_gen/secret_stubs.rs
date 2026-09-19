//! Secret values (retail 12.x): turn the secrecy keys of Blizzard's
//! `APIDocumentationGenerated` tables into `@secret-*` annotations and
//! `secret<T>` return/field/payload types on the stub text.
//!
//! Ketho's generated annotations drop every secrecy key, so the index is built
//! from the raw retail documentation and applied as a text rewrite over every
//! stub file (vendor and generated) before scanning. The classic branches carry
//! no secrecy keys, which keeps the metadata retail-only by construction.

use super::*;
use crate::secrets::{PreconditionFailure, SecretArgsPolicy, SecretPrecondition, SecretPredicate};

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
    /// `@secret-clears` of a curated `C_Secrets` guard ([`SECRET_CLEARS`]).
    pub(in crate::stub_gen) clears: Option<GuardAnnotation>,
    /// `@secret-restriction-guard` of a curated restriction guard ([`RESTRICTION_GUARDS`]).
    pub(in crate::stub_gen) restriction_guard: Option<GuardAnnotation>,
    /// `@secret-precondition`s from the secret predicate table.
    pub(in crate::stub_gen) preconditions: Vec<SecretPrecondition>,
}

/// A curated guard annotation: its leading word (predicate list, `*`,
/// restriction type, or the position of the parameter naming the restriction),
/// the bound parameters (position, documented name), and the `== Value` result
/// that clears, if not `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::stub_gen) struct GuardAnnotation {
    pub(in crate::stub_gen) head: String,
    pub(in crate::stub_gen) head_param: Option<usize>,
    pub(in crate::stub_gen) params: Vec<(usize, String)>,
    pub(in crate::stub_gen) equals: Option<&'static str>,
}

impl GuardAnnotation {
    /// `head param… [== Value]`, naming each parameter as the stub does.
    fn text(&self, params: &[String]) -> String {
        let stub_name = |index: usize, doc_name: &String| params.get(index).unwrap_or(doc_name).clone();
        let mut words = vec![match self.head_param {
            Some(index) => stub_name(index, &self.head),
            None => self.head.clone(),
        }];
        words.extend(self.params.iter().map(|(index, doc_name)| stub_name(*index, doc_name)));
        if let Some(value) = self.equals {
            words.push(format!("== {value}"));
        }
        words.join(" ")
    }
}

impl StubSecrecy {
    fn is_empty(&self) -> bool {
        self.when.is_empty() && self.args.is_none() && self.aspects.is_empty()
            && !self.entries.iter().any(|(_, secret)| *secret)
            && self.clears.is_none() && self.restriction_guard.is_none() && self.preconditions.is_empty()
    }

    /// Whether a failing precondition returns nothing, which makes every return nilable.
    fn nilable_returns(&self) -> bool {
        self.preconditions.iter().any(SecretPrecondition::returns_nothing)
    }

    /// `params` are the stub's own parameter names, which can differ from the
    /// documentation's (annotations name the parameter at the same position).
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
        if let Some(clears) = &self.clears {
            lines.push(format!("---@secret-clears {}", clears.text(params)));
        }
        if let Some(guard) = &self.restriction_guard {
            lines.push(format!("---@secret-restriction-guard {}", guard.text(params)));
        }
        lines.extend(self.preconditions.iter().map(|p| {
            let mut words = vec!["---@secret-precondition".to_string(), p.name.clone()];
            words.extend(p.failure.map(|f| f.name().to_string()));
            words.extend(p.doc.clone());
            words.join(" ")
        }));
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
    /// Structure name → the predicates of every API that returns it secret
    /// (`@secret-when` on the `@class`), sorted by name.
    pub(in crate::stub_gen) structure_predicates: HashMap<String, Vec<SecretPredicate>>,
}

impl SecretIndex {
    pub(in crate::stub_gen) fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.events.is_empty() && self.structures.is_empty()
    }
}

/// Resolves which entry-level flags are secret predicates and preconditions.
struct PredicateTable<'a> {
    secret: HashMap<&'a str, Option<&'a str>>,
    /// Preconditions of the secret predicate table, with their failure mode and documentation.
    preconditions: HashMap<&'a str, (Option<PreconditionFailure>, Option<&'a str>)>,
    other: HashSet<&'a str>,
}

impl<'a> PredicateTable<'a> {
    fn new(docs: &'a BlizzardApiDocs) -> Self {
        let mut secret = HashMap::new();
        let mut preconditions = HashMap::new();
        let mut other = HashSet::new();
        for p in &docs.predicates {
            if p.kind == "Secret" {
                secret.insert(p.name.as_str(), p.documentation.as_deref());
            } else {
                if p.kind == "Precondition" && p.secret_table {
                    let failure = p.failure_mode.as_deref().and_then(PreconditionFailure::parse);
                    preconditions.insert(p.name.as_str(), (failure, p.documentation.as_deref()));
                }
                other.insert(p.name.as_str());
            }
        }
        Self { secret, preconditions, other }
    }

    /// The secrecy preconditions among an entry's flags, in flag order.
    fn preconditions(&self, flags: &[String]) -> Vec<SecretPrecondition> {
        flags.iter().filter_map(|flag| {
            let (failure, doc) = self.preconditions.get(flag.as_str())?;
            Some(SecretPrecondition { name: flag.clone(), failure: *failure, doc: doc.map(str::to_string) })
        }).collect()
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
/// documentation. A per-spell or per-power-type flag ("may be flagged as never
/// or always secret") doesn't cancel the exemption: the stubs can't see it. The
/// threat predicates exempt *pairs* of tokens ("one unit token is the player …
/// while the other is a nameplate"), which one parameter can't express, so
/// they aren't listed and exempt nothing.
const PREDICATE_EXEMPT_UNITS: &[(&str, &[&str])] = &[
    // "when the unit isn't player-controlled or in the party/raid"
    ("SecretWhenUnitIdentityRestricted", &["player", "pet"]),
    // "under regular unit identity secrecy rules, except in PvP when the queried unit is a player"
    ("SecretWhenUnitNameIdentityRestricted", &["player", "pet"]),
    // "when the unit isn't player-controlled"
    ("SecretWhenUnitHealthMaxRestricted", &["player", "pet"]),
    // "when the unit isn't player-controlled"
    ("SecretWhenUnitPowerMaxRestricted", &["player", "pet"]),
    // "if the unit being queried for cast information is not the player or their pet"
    ("SecretWhenUnitSpellCastRestricted", &["player", "pet"]),
    // "if the subject unit is not the active player"
    ("SecretWhenLossOfControlInfoRestricted", &["player"]),
    // "except for unit tokens under the player's direct control"
    ("SecretWhenUnitPossessionRestricted", &["player", "pet"]),
];

/// A curated guard: the function, the annotation's leading word, the bound
/// parameters (documented names), and the `== Value` result that clears, if not
/// `false`.
type GuardSpec = (&'static str, &'static str, &'static [&'static str], Option<&'static str>);

/// `Enum.SecrecyLevel.NeverSecret`: "Will never yield secret values when queried."
const NEVER_SECRET: Option<&str> = Some("Enum.SecrecyLevel.NeverSecret");

/// `C_Secrets` guards and the predicates their clearing result rules out. The
/// docs link no guard to a predicate, so this is curated by name, citing each
/// guard's documentation. Parameters bind the clear to later calls whose
/// arguments at the same positions match, so a parameter is listed only where
/// the guarded APIs take the same value at the same position. Not listed:
/// `CanCompareUnitTokens` (a precondition check), `GetPowerTypeSecrecy` (power
/// APIs take the power type second), `GetSpellCastSecrecy` (cast APIs take a
/// unit, not a spell), and `ShouldTotemSpellBeSecret` (totem APIs take a slot).
const SECRET_CLEARS: &[GuardSpec] = &[
    // "If false, all APIs that are tagged as potentially returning secrets will never do so."
    ("C_Secrets.HasSecretRestrictions", "*", &[], None),
    // "Returns true if queries for aura data will generally produce secret values."
    ("C_Secrets.ShouldAurasBeSecret", "SecretWhenAurasRestricted,SecretWhenUnitAuraRestricted", &[], None),
    // "Returns true if a given aura index will produce secret values if queried."
    ("C_Secrets.ShouldUnitAuraIndexBeSecret", "SecretWhenAurasRestricted,SecretWhenUnitAuraRestricted", &["unit", "index"], None),
    // "Returns true if a given aura instance ID will produce secret values if queried."
    ("C_Secrets.ShouldUnitAuraInstanceBeSecret", "SecretWhenAurasRestricted,SecretWhenUnitAuraRestricted", &["unit", "auraInstanceID"], None),
    // "Returns true if a given aura slot ID will produce secret values if queried."
    ("C_Secrets.ShouldUnitAuraSlotBeSecret", "SecretWhenAurasRestricted,SecretWhenUnitAuraRestricted", &["unit", "slot"], None),
    // "Returns true if a given spell identifier would, if applied as an aura,
    // produce secret values when queried."
    ("C_Secrets.ShouldSpellAuraBeSecret", "SecretWhenAurasRestricted,SecretWhenUnitAuraRestricted", &["spellIdentifier"], None),
    // "Queries the base secrecy for a spell if queried as an aura."
    ("C_Secrets.GetSpellAuraSecrecy", "SecretWhenAurasRestricted,SecretWhenUnitAuraRestricted", &["spellIdentifier"], NEVER_SECRET),
    // "Returns true if queries for cooldown data will generally produce secret values."
    ("C_Secrets.ShouldCooldownsBeSecret", "SecretWhenCooldownsRestricted", &[], None),
    // "Returns true if a given spell identifier will produce secret values for cooldowns if queried."
    ("C_Secrets.ShouldSpellCooldownBeSecret", "SecretWhenCooldownsRestricted", &["spellIdentifier"], None),
    // "Queries the base secrecy for a spell if queried as a cooldown."
    ("C_Secrets.GetSpellCooldownSecrecy", "SecretWhenCooldownsRestricted", &["spellIdentifier"], NEVER_SECRET),
    // "Returns true if a given action bar slot ID will produce secret values for cooldowns if queried."
    ("C_Secrets.ShouldActionCooldownBeSecret", "SecretWhenCooldownsRestricted", &["actionID"], None),
    // "Returns true if a given spellbook item will produce secret values for cooldowns if queried."
    ("C_Secrets.ShouldSpellBookItemCooldownBeSecret", "SecretWhenCooldownsRestricted", &["spellBookItemSlotIndex", "spellBookItemSpellBank"], None),
    // "Returns true if information about a totem slot will produce secret values if queried."
    ("C_Secrets.ShouldTotemSlotBeSecret", "SecretWhenTotemSlotSecret", &["slot"], None),
    // "Returns true if queries that compare units will produce secret values."
    ("C_Secrets.ShouldUnitComparisonBeSecret", "SecretWhenUnitComparisonRestricted", &["unit1", "unit2"], None),
    // "Returns true if queries for maximum unit health will produce secret values."
    ("C_Secrets.ShouldUnitHealthMaxBeSecret", "SecretWhenUnitHealthMaxRestricted", &["unit"], None),
    // "Returns true if queries for unit identity (such as name or GUID) will produce
    // secret values." Name identity is identity secrecy with a PvP exception.
    ("C_Secrets.ShouldUnitIdentityBeSecret", "SecretWhenUnitIdentityRestricted,SecretWhenUnitNameIdentityRestricted", &["unit"], None),
    // "Returns true if queries for unit power will produce secret values."
    ("C_Secrets.ShouldUnitPowerBeSecret", "SecretWhenUnitPowerRestricted", &["unit", "powerType"], None),
    // "Returns true if queries for maximum unit power will produce secret values."
    ("C_Secrets.ShouldUnitPowerMaxBeSecret", "SecretWhenUnitPowerMaxRestricted", &["unit", "powerType"], None),
    // "Returns true if queries for spell casting information for a unit would
    // produce secret values when queried." Bound by unit: cast APIs take no spell.
    ("C_Secrets.ShouldUnitSpellCastBeSecret", "SecretWhenUnitSpellCastRestricted", &["unit"], None),
    // "Returns true if queries for spell casting information for a specific unit
    // will generally produce secret values."
    ("C_Secrets.ShouldUnitSpellCastingBeSecret", "SecretWhenUnitSpellCastRestricted", &["unit"], None),
    // "Returns true if queries for unit statistics will produce secret values."
    ("C_Secrets.ShouldUnitStatsBeSecret", "SecretWhenUnitStatsRestricted", &[], None),
    // "Returns true if queries for unit threat status will produce secret values."
    ("C_Secrets.ShouldUnitThreatStateBeSecret", "SecretWhenUnitThreatStateRestricted", &["unit", "mobUnit"], None),
    // "Returns true if queries for unit threat values will produce secret values."
    ("C_Secrets.ShouldUnitThreatValuesBeSecret", "SecretWhenUnitThreatValuesRestricted", &["unit", "mobUnit"], None),
];

/// Guards that prove an addon restriction inactive, and the documentation
/// function each is validated against.
const RESTRICTION_GUARDS: &[(GuardSpec, &str)] = &[
    // "Returns true if an addon restriction type is in an active state."
    (("C_RestrictedActions.IsAddOnRestrictionActive", "type", &[], None), "C_RestrictedActions.IsAddOnRestrictionActive"),
    // "Returns the current state of an addon restriction type." `Inactive`: "State
    // used when an addon restriction is not being enforced." (`Activating` counts
    // as active.)
    (("C_RestrictedActions.GetAddOnRestrictionState", "type", &[], Some("Enum.AddOnRestrictionState.Inactive")), "C_RestrictedActions.GetAddOnRestrictionState"),
    // ASSUMED: combat lockdown is the `Combat` restriction (undocumented). The
    // documentation entry overrides its namespace to the global; Ketho's
    // annotations also declare it on the namespace.
    (("InCombatLockdown", "Combat", &[], None), "C_RestrictedActions.InCombatLockdown"),
    (("C_RestrictedActions.InCombatLockdown", "Combat", &[], None), "C_RestrictedActions.InCombatLockdown"),
];

/// The documented function `key` (`Namespace.Name` or a global name).
fn doc_function<'a>(docs: &'a BlizzardApiDocs, key: &str) -> Option<&'a BlizzardFunction> {
    docs.functions.iter().find(|f| match &f.namespace {
        Some(ns) => key.strip_prefix(ns.as_str()).and_then(|rest| rest.strip_prefix('.')) == Some(f.name.as_str()),
        None => key == f.name,
    })
}

/// A curated guard's annotation, validated against the documentation: `None`
/// (with a warning) when the function, a parameter, or a predicate is missing,
/// so an upstream rename drops the guard instead of emitting a stale one.
fn guard_annotation(
    docs: &BlizzardApiDocs,
    predicates: &PredicateTable<'_>,
    (_, head, params, equals): &GuardSpec,
    doc_key: &str,
    head_is_predicates: bool,
) -> Option<GuardAnnotation> {
    let Some(func) = doc_function(docs, doc_key) else {
        log::warn!("  Secret values: guard {doc_key} is not documented; skipped");
        return None;
    };
    let param_index = |name: &str| {
        let index = func.arguments.iter().position(|a| a.name == name);
        if index.is_none() {
            log::warn!("  Secret values: guard {doc_key} has no parameter {name}; skipped");
        }
        index
    };
    let mut head_param = None;
    if head_is_predicates {
        if let Some(unknown) = head.split(',').find(|p| *p != "*" && predicates.secret_predicate(p).is_none()) {
            log::warn!("  Secret values: guard {doc_key} names unknown predicate {unknown}; skipped");
            return None;
        }
        // `*` states that no restriction is in force, so it binds no argument.
        if *head == "*" && !params.is_empty() {
            log::warn!("  Secret values: guard {doc_key} binds parameters to `*`; skipped");
            return None;
        }
    } else if crate::secrets::restriction_bit(head).is_none() {
        head_param = Some(param_index(head)?);
    }
    let bound = params.iter()
        .map(|param| Some((param_index(param)?, param.to_string())))
        .collect::<Option<Vec<_>>>()?;
    Some(GuardAnnotation { head: head.to_string(), head_param, params: bound, equals: *equals })
}

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

/// Structures Blizzard's docs reference without defining (`AuraData`) that API
/// pages transclude from the wiki, sorted.
pub(in crate::stub_gen) fn undefined_structure_names(docs: &BlizzardApiDocs, wiki_pages: &HashMap<String, String>) -> Vec<String> {
    let defined: HashSet<&str> = docs.structures.iter().map(|s| s.name.as_str()).collect();
    let referenced: HashSet<&str> = docs.functions.iter()
        .chain(docs.script_objects.iter().flat_map(|o| &o.functions))
        .flat_map(|f| f.arguments.iter().chain(&f.returns))
        .chain(docs.events.iter().flat_map(|e| &e.payload))
        .chain(docs.structures.iter().flat_map(|s| &s.fields))
        .flat_map(|p| [Some(p.type_name.as_str()), p.inner_type.as_deref()])
        .flatten()
        .filter(|name| !defined.contains(name))
        .collect();
    let mut names: Vec<String> = transcluded_structure_names(wiki_pages).into_iter()
        .filter(|name| referenced.contains(name.as_str()))
        .collect();
    names.sort();
    names
}

/// How secret returns and payloads reach a structure: the predicates of the
/// APIs that return it, or `unconditional` when one returns it secret without a
/// predicate (`SecretReturns`, `SecretValue`). A conditional entry names no
/// condition, so it adds neither.
#[derive(Debug, Default, Clone)]
struct StructReach {
    predicates: Vec<SecretPredicate>,
    unconditional: bool,
}

impl StructReach {
    /// Merge `other` in; whether anything changed.
    fn merge(&mut self, other: &StructReach) -> bool {
        let mut changed = other.unconditional && !self.unconditional;
        self.unconditional |= other.unconditional;
        for p in &other.predicates {
            if !self.predicates.iter().any(|q| q.name == p.name) {
                self.predicates.push(p.clone());
                changed = true;
            }
        }
        changed
    }
}

/// Build the index from the retail docs. `wiki_structures` are the wiki's field
/// lists for structures the docs don't define; `table_types` names every `@class`
/// the stubs declare (structures the docs reference without describing field by
/// field, widgets, Lua objects) so such values aren't wrapped as scalars.
pub(in crate::stub_gen) fn build_secret_index(
    docs: &BlizzardApiDocs,
    wiki_structures: &[BlizzardStructure],
    table_types: &HashSet<String>,
) -> SecretIndex {
    let predicates = PredicateTable::new(docs);
    // A wiki page that marks no field's secrecy hasn't been documented for secret
    // values; reaching it would make every field secret on no evidence.
    let doc_structures: HashSet<&str> = docs.structures.iter().map(|s| s.name.as_str()).collect();
    let all_structures: Vec<&BlizzardStructure> = docs.structures.iter()
        .chain(wiki_structures.iter().filter(|s| {
            !doc_structures.contains(s.name.as_str()) && s.fields.iter().any(|f| f.secrecy != ParamSecrecy::default())
        }))
        .collect();
    let structures: HashSet<&str> = all_structures.iter().map(|s| s.name.as_str()).collect();
    let mut index = SecretIndex::default();
    // Structures reached through a secret return/payload; their fields get marked below.
    let mut reach: HashMap<String, StructReach> = HashMap::new();

    // `taint` = whether the entries' own types become `secret<T>`. Widget
    // methods keep their metadata but not the taint: a widget getter returns
    // secrets only for objects that were fed secret values.
    let mut entry_secrecy = |secrecy: &EntrySecrecy, arguments: &[BlizzardParam], entries: &[BlizzardParam], all_key: &str, taint: bool| {
        let when: Vec<SecretPredicate> = secrecy.flags.iter()
            .filter_map(|f| predicates.secret_predicate(f).map(|doc| SecretPredicate { name: f.clone(), doc }))
            .collect();
        let all_key_set = secrecy.flags.iter().any(|f| f == all_key);
        let all = !when.is_empty() || all_key_set;
        let never_all = secrecy.flags.iter().any(|f| f == "ReturnsNeverSecret");
        let mut out_entries = Vec::with_capacity(entries.len());
        for e in entries {
            let maybe = taint && !never_all && !e.secrecy.never && (all || e.secrecy.conditional || e.secrecy.value);
            let table_like = is_table_like(e, &structures, table_types);
            if (maybe && !e.secrecy.never_contents) || (taint && e.secrecy.secret_contents) {
                let entry_reach = StructReach {
                    predicates: when.clone(),
                    unconditional: e.secrecy.value || (when.is_empty() && all_key_set && maybe),
                };
                for name in [Some(&e.type_name), e.inner_type.as_ref()].into_iter().flatten() {
                    if structures.contains(name.as_str()) {
                        reach.entry(name.clone()).or_default().merge(&entry_reach);
                    }
                }
            }
            out_entries.push((e.name.clone(), maybe && !table_like));
        }
        let unless = unit_exemption(&when, secrecy, arguments, entries);
        // "Constant accessors" (wiki `Secret_Values`) apply no aspects for secret
        // arguments but return secrets when any argument is secret, which is the
        // `AllowedWhenTainted` rule; the docs still mark them `AllowedWhenUntainted`.
        let args = if secrecy.flags.iter().any(|f| f == "ConstSecretAccessor") {
            Some(SecretArgsPolicy::AllowedWhenTainted)
        } else {
            secrecy.arguments.as_deref()
                .and_then(SecretArgsPolicy::from_blizzard_name)
                .filter(|p| *p != SecretArgsPolicy::AllowedWhenUntainted)
        };
        StubSecrecy {
            when,
            args,
            aspects: secrecy.aspects.clone(),
            entries: out_entries,
            unless,
            preconditions: predicates.preconditions(&secrecy.flags),
            ..StubSecrecy::default()
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
    for spec in SECRET_CLEARS {
        if let Some(guard) = guard_annotation(docs, &predicates, spec, spec.0, true) {
            index.functions.entry(spec.0.to_string()).or_default().clears = Some(guard);
        }
    }
    for (spec, doc_key) in RESTRICTION_GUARDS {
        if let Some(guard) = guard_annotation(docs, &predicates, spec, doc_key, false) {
            index.functions.entry(spec.0.to_string()).or_default().restriction_guard = Some(guard);
        }
    }

    // Mark structure fields. A reached structure's non-`NeverSecret` scalar fields
    // may be secret (nested structures are reached in turn, with the same
    // predicates); `SecretValue` / `ConditionalSecret` fields may be secret in any
    // structure.
    let by_name: HashMap<&str, &BlizzardStructure> = all_structures.iter().map(|s| (s.name.as_str(), *s)).collect();
    let mut changed = true;
    while changed {
        changed = false;
        let mut names: Vec<String> = reach.keys().cloned().collect();
        names.sort();
        for name in names {
            let Some(st) = by_name.get(name.as_str()) else { continue };
            let parent = reach[&name].clone();
            for f in st.fields.iter().filter(|f| !f.secrecy.never && !f.secrecy.never_contents) {
                for nested in [Some(&f.type_name), f.inner_type.as_ref()].into_iter().flatten() {
                    if structures.contains(nested.as_str()) {
                        let nested_reach = reach.entry(nested.clone());
                        changed |= matches!(nested_reach, std::collections::hash_map::Entry::Vacant(_));
                        changed |= nested_reach.or_default().merge(&parent);
                    }
                }
            }
        }
    }
    for st in all_structures {
        let reached = reach.get(&st.name);
        let fields: HashSet<String> = st.fields.iter()
            .filter(|f| (reached.is_some() && !f.secrecy.never) || f.secrecy.conditional || f.secrecy.value)
            .filter(|f| !is_table_like(f, &structures, table_types))
            .map(|f| f.name.clone())
            .collect();
        if fields.is_empty() {
            continue;
        }
        // A `SecretValue` field is secret whatever the predicates say.
        let clearable = !st.fields.iter().any(|f| f.secrecy.value);
        if let Some(r) = reached.filter(|r| clearable && !r.unconditional && !r.predicates.is_empty()) {
            let mut preds = r.predicates.clone();
            preds.sort_by(|a, b| a.name.cmp(&b.name));
            index.structure_predicates.insert(st.name.clone(), preds);
        }
        index.structures.insert(st.name.clone(), fields);
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

/// `T` → `T?` (`A|B` → `A|B|nil`); already-optional and function types are left alone.
pub(in crate::stub_gen) fn nilable_type(ty: &str) -> String {
    if ty.ends_with('?') || ty.split('|').any(|member| member == "nil") || ty.starts_with("fun(") {
        ty.to_string()
    } else if ty.contains('|') {
        format!("{ty}|nil")
    } else {
        format!("{ty}?")
    }
}

/// Rewrite the type token of a `---@return T name` line: wrap a secret return,
/// and make it nilable when a failing precondition returns nothing.
fn rewrite_return_line(line: &str, secrecy: &StubSecrecy, index: usize) -> String {
    let Some(rest) = line.strip_prefix("---@return ") else { return line.to_string() };
    let mut parts = rest.splitn(3, ' ');
    let Some(ty) = parts.next() else { return line.to_string() };
    let name = parts.next();
    let mut new_ty = if secrecy.entry_is_secret(name, index) { wrap_secret_type(ty) } else { ty.to_string() };
    if secrecy.nilable_returns() {
        new_ty = nilable_type(&new_ty);
    }
    let tail = &rest[ty.len()..];
    format!("---@return {new_ty}{tail}")
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
            if let Some(preds) = index.structure_predicates.get(name) {
                out.extend(preds.iter().map(|p| format!("---@secret-when {}", p.name)));
                changed = true;
            }
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
