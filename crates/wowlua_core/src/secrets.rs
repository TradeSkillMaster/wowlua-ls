//! Secret values (retail WoW 12.x): the stub metadata model and the runtime
//! rules the analysis engine encodes.
//!
//! While addon restrictions are active (combat, encounters, Mythic+, PvP, …)
//! many APIs return *secret* values. Tainted (addon) code may store, return and
//! pass them around, but inspecting one is an immediate Lua error. The type
//! system models a value that may be secret as [`ValueType::Secret`] — every
//! secret-producing API is conditional on game state, so "may be" is the only
//! secrecy there is.
//!
//! Every rule below is tagged **VERIFIED** (stated by Blizzard, as quoted on
//! warcraft.wiki.gg `Secret_Values` and `Patch_12.0.0/Planned_API_changes`) or
//! **ASSUMED** (undocumented; confirm in-game by feeding `secretwrap(...)` values
//! to the operation from a tainted addon). Changing a rule here is the only edit
//! needed to change what propagates and what is diagnosed.
//!
//! Facts the engine needs no rule constant for: storing a secret in a table
//! value, a local, or an upvalue, returning it, and passing it to a Lua function
//! are VERIFIED allowed (the value just flows); `type(secret)` VERIFIED returns
//! the real type.

use crate::ast::Operator;
use crate::types::ValueType;

/// What happens when tainted code applies an operation to a secret value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretRule {
    /// Allowed; the result is secret whenever an operand is.
    Propagate,
    /// Allowed; the result is an ordinary (non-secret) value.
    Allowed,
    /// Immediate Lua error. These are what the `secret-*` diagnostics report.
    /// The result is treated as an ordinary value: execution stops at the error,
    /// so later uses of the result would only repeat the report.
    Error,
}

/// Binary operator with at least one possibly-secret operand (full operand
/// types, secret members included).
pub fn binary_op_rule(op: Operator, lhs: &ValueType, rhs: &ValueType) -> SecretRule {
    match op {
        // VERIFIED: "Tainted code is allowed to concatenate secret values that
        // are strings or numbers" (the result is a secret string).
        Operator::Concatenate => SecretRule::Propagate,
        // VERIFIED: "Tainted code is not allowed to perform arithmetic on secret values."
        Operator::Add | Operator::Subtract | Operator::Multiply
        | Operator::Divide | Operator::Modulo | Operator::Hat => SecretRule::Error,
        // VERIFIED: "not allowed to compare ... secret values".
        Operator::LessThan | Operator::GreaterThan
        | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals => SecretRule::Error,
        // VERIFIED (12.0.0): equality comparing possibly-secret values of
        // different types, or two nils, no longer errors and yields a
        // non-secret result. Same-type equality still errors.
        Operator::Equals | Operator::NotEquals => {
            if may_share_runtime_type(lhs, rhs) { SecretRule::Error } else { SecretRule::Allowed }
        }
        // `and`/`or` return one of their operands, keeping only the secrecy of
        // the operands `and_or_secrets` lists; the truth test of the left
        // operand is covered by `truth_test_rule`.
        Operator::And | Operator::Or => SecretRule::Propagate,
        Operator::Not | Operator::ArrayLength | Operator::None => SecretRule::Allowed,
    }
}

/// The secret values an `and`/`or` result can carry: Lua returns an operand
/// unchanged, but only an operand that isn't tested on the way keeps its
/// secrecy. `a and b` yields `b`, or `a` when falsy — `nil` (never secret) or a
/// secret `false`, whose truth test errors and leaves an ordinary value. `a or b`
/// yields `b`, or `a` when truthy — only a non-boolean secret survives that test.
/// VERIFIED (follows from `truth_test_rule`).
pub fn and_or_secrets(op: Operator, lhs: &ValueType, rhs: &ValueType) -> Vec<ValueType> {
    let mut secrets = secret_members(rhs);
    if op == Operator::Or {
        secrets.extend(secret_members(lhs).into_iter().filter(|t| !matches!(t.strip_opaque(), ValueType::Boolean(_))));
    }
    secrets
}

/// The underlying types of `t`'s secret members.
fn secret_members(t: &ValueType) -> Vec<ValueType> {
    match t {
        ValueType::Secret(inner) => vec![(**inner).clone()],
        ValueType::Union(members) => members.iter().flat_map(secret_members).collect(),
        _ => Vec::new(),
    }
}

/// Unary minus on a secret number. VERIFIED error (it is arithmetic).
pub const NEGATE: SecretRule = SecretRule::Error;

/// `#secret`. VERIFIED error ("not allowed to use the length operator").
pub const LENGTH: SecretRule = SecretRule::Error;

/// Indexing a secret: `secret.x`, `secret[k]`, or a method call `secret:m()`
/// (string methods included). VERIFIED error ("not allowed to perform indexed
/// access or assignment ... on secret values").
pub const INDEX: SecretRule = SecretRule::Error;

/// Calling a secret. VERIFIED error ("not allowed to call secret values as-if
/// they were functions").
pub const CALL: SecretRule = SecretRule::Error;

/// A secret start, limit, or step of a numeric `for` loop. ASSUMED error: the
/// loop compares its counter to the limit on every iteration (and the step to
/// zero), which the comparison rule forbids.
pub const NUMERIC_FOR_BOUND: SecretRule = SecretRule::Error;

/// A truth test that Lua forces on a value: `if`/`elseif`/`while`/`repeat`
/// conditions, `not`, and the left operand of `and`/`or`. VERIFIED: tests on
/// non-boolean secrets are allowed (nil is false, everything else true); on
/// boolean secrets they error (`not secretwrap(true)` errors).
pub fn truth_test_rule(t: &ValueType) -> SecretRule {
    if has_secret_boolean(t) { SecretRule::Error } else { SecretRule::Allowed }
}

/// Using a secret as a table key. VERIFIED error when storing (`t[secret] = v`,
/// "not allowed to store secret values as keys in tables"); ASSUMED error on
/// read (`t[secret]`), which would otherwise reveal the value.
pub const TABLE_KEY: SecretRule = SecretRule::Error;

/// Passing a secret to a C API parameter with the given `SecretArguments` policy
/// (no policy = unknown, never diagnosed).
pub fn argument_rule(policy: Option<SecretArgsPolicy>) -> SecretRule {
    match policy {
        // VERIFIED: "will never accept secret values, even from untainted callers".
        Some(SecretArgsPolicy::NotAllowed) => SecretRule::Error,
        // VERIFIED accepted from tainted callers ("resulting in secret strings"
        // for string.format/concat/join); ASSUMED that every such API's results
        // inherit the secrecy of its arguments.
        Some(SecretArgsPolicy::AllowedWhenTainted) => SecretRule::Propagate,
        // VERIFIED (12.1.0) rejected for addon code: `UnitExists(secretwrap("player"))`
        // raises "bad argument #1 to 'UnitExists' (Usage: local result =
        // UnitExists(unit)). Secret values are only allowed during untainted
        // execution for this argument", as does
        // `C_Item.GetItemNameByID(secretwrap(6948))`. Blizzard's generated docs
        // also attach the key to the secret builtins, which do accept secrets
        // (`issecretvalue(secretwrap(5))` works from the same chat context), so
        // those are curated out at stub-generation time.
        Some(SecretArgsPolicy::AllowedWhenUntainted) => SecretRule::Error,
        // No policy: nothing is known about the API, so nothing is diagnosed and
        // the result carries no argument secrecy. This is also how a function that
        // accepts secrets and returns ordinary values is expressed.
        None => SecretRule::Allowed,
    }
}

/// Could two operand types (secret wrappers ignored) hold values of the same
/// Lua type at runtime? Unknown types conservatively answer yes; a `nil` side
/// only matches a possibly-nil other side.
fn may_share_runtime_type(lhs: &ValueType, rhs: &ValueType) -> bool {
    let (l, r) = (runtime_kinds(lhs), runtime_kinds(rhs));
    if l == KIND_UNKNOWN || r == KIND_UNKNOWN {
        return true;
    }
    // Two nils compare without error (VERIFIED), so only non-nil kinds count.
    (l & r & !KIND_NIL) != 0
}

const KIND_NIL: u8 = 1;
const KIND_BOOLEAN: u8 = 2;
const KIND_NUMBER: u8 = 4;
const KIND_STRING: u8 = 8;
const KIND_TABLE: u8 = 16;
const KIND_FUNCTION: u8 = 32;
const KIND_OTHER: u8 = 64;
const KIND_UNKNOWN: u8 = 0xFF;

fn runtime_kinds(t: &ValueType) -> u8 {
    match t {
        ValueType::Nil => KIND_NIL,
        ValueType::Boolean(_) => KIND_BOOLEAN,
        ValueType::Number | ValueType::NumberLiteral(_) => KIND_NUMBER,
        ValueType::String(_) | ValueType::KeyOf(_) => KIND_STRING,
        // Enum tables are numbers/strings at runtime; the analysis can't see
        // `enum_kind` here, so treat any class table as unknown.
        ValueType::Table(Some(_)) => KIND_UNKNOWN,
        ValueType::Table(None) | ValueType::TableShape(_) => KIND_TABLE,
        ValueType::Function(_) | ValueType::FunctionSig(_) => KIND_FUNCTION,
        ValueType::Userdata | ValueType::Thread => KIND_OTHER,
        ValueType::Secret(inner) | ValueType::OpaqueAlias(_, inner) => runtime_kinds(inner),
        ValueType::Union(members) => members.iter().fold(0, |acc, m| acc | runtime_kinds(m)),
        ValueType::Any | ValueType::TypeVariable(_) | ValueType::Intersection(_) => KIND_UNKNOWN,
    }
}

fn has_secret_boolean(t: &ValueType) -> bool {
    match t {
        ValueType::Secret(inner) => matches!(inner.strip_opaque(), ValueType::Boolean(_)),
        ValueType::Union(members) => members.iter().any(has_secret_boolean),
        _ => false,
    }
}

// ── Addon restrictions ───────────────────────────────────────────────────────

/// `Enum.AddOnRestrictionType` members, indexed by their enum value (VERIFIED:
/// `RestrictedActionsConstantsDocumentation.lua`). A restriction set is a
/// bitmask over these positions.
pub const RESTRICTION_TYPES: [&str; 6] = ["Combat", "Encounter", "ChallengeMode", "PvPMatch", "Map", "Chat"];

/// The enum a `@secret-restriction-guard` argument names its type with
/// (`Enum.AddOnRestrictionType.Combat`).
pub const RESTRICTION_TYPE_ENUM: [&str; 2] = ["Enum", "AddOnRestrictionType"];

/// The bit of a restriction type name (`Combat` → `1 << 0`).
pub fn restriction_bit(name: &str) -> Option<u8> {
    RESTRICTION_TYPES.iter().position(|t| *t == name).map(|i| 1 << i)
}

/// The bit of a restriction type written as `Enum.AddOnRestrictionType.<Type>`.
pub fn restriction_bit_for_path(path: &[String]) -> Option<u8> {
    match path {
        [enum_root, enum_name, member] if *enum_root == RESTRICTION_TYPE_ENUM[0] && *enum_name == RESTRICTION_TYPE_ENUM[1] => {
            restriction_bit(member)
        }
        _ => None,
    }
}

const COMBAT: u8 = 1;
const ENCOUNTER: u8 = 1 << 1;
const CHALLENGE_MODE: u8 = 1 << 2;
const PVP_MATCH: u8 = 1 << 3;
const MAP: u8 = 1 << 4;
const CHAT: u8 = 1 << 5;

/// The addon restrictions a secret predicate depends on: while every one of
/// them is inactive, the predicate yields no secrets. `None` for predicates that
/// depend on the unit or object queried (identity, health max, power, stats,
/// casts, loss of control, possession, threat, anchoring, curves, formatters);
/// only their own `C_Secrets` guard or `HasSecretRestrictions` clears those.
///
/// Per-spell (and per-totem-aura) "always secret" flags take priority over
/// restrictions, so clearing aura, cooldown, and totem predicates by restriction
/// state is unsound for such spells; the stubs can't see those flags.
pub fn predicate_restrictions(predicate: &str) -> Option<u8> {
    let four = COMBAT | ENCOUNTER | CHALLENGE_MODE | PVP_MATCH;
    Some(match predicate {
        // VERIFIED: "when combat addon restrictions are in effect."
        "SecretWhenInCombat" => COMBAT,
        // VERIFIED: "when combat, encounter, challenge mode, or PvP match addon
        // restrictions are in effect." (the three per-spell/totem predicates add
        // "Individual spells may be flagged as never or always secret, which
        // takes priority over restrictions.")
        "SecretWhenAurasRestricted" | "SecretWhenUnitAuraRestricted" | "SecretWhenCooldownsRestricted"
        | "SecretWhenTotemSlotSecret" => four,
        // VERIFIED: "when encounter, challenge mode, or PvP match addon
        // restrictions are in effect, and when the player is on a
        // communication-restricted map such as a dungeon or raid." ASSUMED that
        // the communication-restricted map is the `Chat` restriction.
        "SecretInChatMessagingLockdown" => ENCOUNTER | CHALLENGE_MODE | PVP_MATCH | CHAT,
        // VERIFIED: "when the player is on an addon-restricted map such as a
        // dungeon or raid."
        "SecretOnRestrictedMaps" => MAP,
        // VERIFIED: "when PvP match addon restrictions are in effect."
        "SecretInActivePvPMatch" => PVP_MATCH,
        // VERIFIED: "This restriction only applies when the player is on an
        // addon-restricted map." (Comparisons of compound tokens such as
        // `boss1target` are documented as always secret.)
        "SecretWhenUnitComparisonRestricted" => MAP,
        // ASSUMED: undocumented; used only by encounter timeline APIs.
        "SecretWhenEncounterEvent" => ENCOUNTER,
        _ => return None,
    })
}

/// One argument binding of a context guard: the guard parameter whose argument
/// must match, and the parameter names the covered APIs give that same value
/// (`unit` is short for `unit=unit`). Several names cover APIs that spell the
/// parameter differently (`UnitHealthMax(unit)` vs `UnitPowerMax(unitToken, …)`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretArgBinding {
    pub guard_param: String,
    pub callee_params: Vec<String>,
}

impl SecretArgBinding {
    /// Parse one `guardParam[=calleeParam[,calleeParam…]]` word.
    fn parse(word: &str) -> Option<Self> {
        let (guard_param, callee) = word.split_once('=').unwrap_or((word, word));
        let callee_params: Vec<String> = callee.split(',').filter(|n| !n.is_empty()).map(str::to_string).collect();
        (!guard_param.is_empty() && !callee_params.is_empty())
            .then(|| Self { guard_param: guard_param.to_string(), callee_params })
    }

    /// Parse a whitespace-separated binding list.
    fn parse_list<'w>(words: impl Iterator<Item = &'w str>) -> Option<Vec<Self>> {
        words.map(Self::parse).collect()
    }
}

/// `@secret-clears <Predicate>[,<Predicate>…]|* [binding…] [== Value]`: a guard
/// whose result proves the listed predicates yield no secrets.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretClears {
    /// The cleared predicates; empty for `*`.
    pub predicates: Vec<String>,
    /// `*`: no API returns secret values (`C_Secrets.HasSecretRestrictions`).
    pub all: bool,
    /// Argument bindings: a later call is cleared only when its argument for
    /// each bound parameter is the same local, field chain, or literal the guard
    /// was given. Always empty for `*`, which states a property of the client
    /// rather than of an argument.
    pub bindings: Vec<SecretArgBinding>,
    /// `== Value`: the result that proves the predicates clear (`None` = a false
    /// result).
    pub equals: Option<String>,
}

impl SecretClears {
    /// Parse the text after `@secret-clears`.
    pub fn parse(rest: &str) -> Option<Self> {
        let (head, equals) = split_equals(rest)?;
        let mut words = head.split_whitespace();
        let predicates = words.next()?;
        let all = predicates == "*";
        let predicates: Vec<String> = if all {
            Vec::new()
        } else {
            predicates.split(',').map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect()
        };
        if !all && predicates.is_empty() {
            return None;
        }
        let bindings = SecretArgBinding::parse_list(words)?;
        // `*` is "no restriction is in force", a property of the client and not
        // of any argument, and silences every report in the region it guards.
        // Binding it would have to be honored there too, so reject the form.
        if all && !bindings.is_empty() {
            return None;
        }
        Some(Self { predicates, all, bindings, equals })
    }
}

/// `@secret-satisfies <Precondition> [binding…]`: a guard whose *true* result
/// proves a [`SecretPrecondition`] holds, so matching calls no longer fail it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretSatisfies {
    pub precondition: String,
    pub bindings: Vec<SecretArgBinding>,
}

impl SecretSatisfies {
    /// Parse the text after `@secret-satisfies`.
    pub fn parse(rest: &str) -> Option<Self> {
        let mut words = rest.split_whitespace();
        let precondition = words.next()?.to_string();
        Some(Self { precondition, bindings: SecretArgBinding::parse_list(words)? })
    }
}

/// `@secret-restriction-guard <param|RestrictionType> [== Value]`: a guard
/// whose result proves an addon restriction inactive.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretRestrictionGuard {
    /// A restriction type name ([`RESTRICTION_TYPES`]), or the parameter whose
    /// `Enum.AddOnRestrictionType` argument names it.
    pub restriction: String,
    /// `== Value`: the result that proves the restriction inactive (`None` = a
    /// false result).
    pub equals: Option<String>,
}

impl SecretRestrictionGuard {
    /// Parse the text after `@secret-restriction-guard`.
    pub fn parse(rest: &str) -> Option<Self> {
        let (head, equals) = split_equals(rest)?;
        let mut words = head.split_whitespace();
        let restriction = words.next()?.to_string();
        words.next().is_none().then_some(Self { restriction, equals })
    }

    /// The fixed restriction bit, when `restriction` names a type rather than a parameter.
    pub fn fixed_bit(&self) -> Option<u8> {
        restriction_bit(&self.restriction)
    }
}

/// `head [== Value]`; `None` when `==` has no value.
fn split_equals(rest: &str) -> Option<(&str, Option<String>)> {
    match rest.split_once("==") {
        Some((head, value)) => {
            let value = value.trim();
            (!value.is_empty() && !value.contains(char::is_whitespace)).then(|| (head, Some(value.to_string())))
        }
        None => Some((rest, None)),
    }
}

/// How a function fails when a secrecy precondition doesn't hold (Blizzard's
/// `FailureMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PreconditionFailure {
    /// `ReturnNothing`: the call returns no values.
    ReturnNothing,
    /// `ReturnWithError`: the call returns no values and reports an error.
    ReturnWithError,
    /// `Error`: the call raises an error.
    Error,
}

impl PreconditionFailure {
    /// Every mode, in annotation spelling.
    pub const NAMES: [&'static str; 3] = ["ReturnNothing", "ReturnWithError", "Error"];

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ReturnNothing" => Some(Self::ReturnNothing),
            "ReturnWithError" => Some(Self::ReturnWithError),
            "Error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Whether `s` names a mode in the wrong case: a typo, not the first word
    /// of a description. A description could plausibly start with any other
    /// word, so only this certain case is rejected.
    pub fn is_miscased(s: &str) -> bool {
        Self::parse(s).is_none() && Self::NAMES.iter().any(|name| name.eq_ignore_ascii_case(s))
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::ReturnNothing => "ReturnNothing",
            Self::ReturnWithError => "ReturnWithError",
            Self::Error => "Error",
        }
    }

    /// Whether a failing call returns nothing, which makes every return nilable.
    pub fn returns_nothing(self) -> bool {
        matches!(self, Self::ReturnNothing | Self::ReturnWithError)
    }
}

/// `@secret-precondition <Name> [FailureMode] [documentation]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretPrecondition {
    pub name: String,
    /// `None` when Blizzard documents no failure mode.
    pub failure: Option<PreconditionFailure>,
    pub doc: Option<String>,
}

impl SecretPrecondition {
    /// Parse the text after `@secret-precondition`.
    pub fn parse(rest: &str) -> Option<Self> {
        let rest = rest.trim();
        let (name, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if name.is_empty() {
            return None;
        }
        let rest = rest.trim_start();
        let (mode, after_mode) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if PreconditionFailure::is_miscased(mode) {
            return None;
        }
        // The failure mode is optional: any other second word starts the description.
        let (failure, doc) = match PreconditionFailure::parse(mode) {
            Some(failure) => (Some(failure), after_mode.trim()),
            None => (None, rest),
        };
        Some(Self { name: name.to_string(), failure, doc: (!doc.is_empty()).then(|| doc.to_string()) })
    }

    /// Whether a failing call returns nothing, which makes every return nilable.
    pub fn returns_nothing(&self) -> bool {
        self.failure.is_some_and(PreconditionFailure::returns_nothing)
    }

    /// A hover bullet: "- Returns nothing when `Name` fails — doc".
    pub fn hover_line(&self) -> String {
        let outcome = match self.failure {
            Some(PreconditionFailure::ReturnNothing) => "Returns nothing when",
            Some(PreconditionFailure::ReturnWithError) => "Returns nothing and reports an error when",
            Some(PreconditionFailure::Error) => "Errors when",
            None => "Precondition:",
        };
        let fails = if self.failure.is_some() { " fails" } else { "" };
        match &self.doc {
            Some(doc) => format!("- {outcome} `{}`{fails} — {doc}", self.name),
            None => format!("- {outcome} `{}`{fails}", self.name),
        }
    }
}

// ── Stub metadata ────────────────────────────────────────────────────────────

/// A `@secret-when` predicate: the named restriction under which a function's
/// (or event's) results become secret, with Blizzard's documentation text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretPredicate {
    pub name: String,
    pub doc: Option<String>,
}

impl SecretPredicate {
    /// A hover bullet: "- <label>: `Name` — doc".
    pub fn hover_line(&self, label: &str) -> String {
        match &self.doc {
            Some(doc) => format!("- {label}: `{}` — {doc}", self.name),
            None => format!("- {label}: `{}`", self.name),
        }
    }
}

/// `@secret-args none|untainted|tainted` — Blizzard's `SecretArguments` policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SecretArgsPolicy {
    /// `none` — `SecretArguments = "NotAllowed"`.
    NotAllowed,
    /// `untainted` — `SecretArguments = "AllowedWhenUntainted"`.
    AllowedWhenUntainted,
    /// `tainted` — `SecretArguments = "AllowedWhenTainted"`.
    AllowedWhenTainted,
}

impl SecretArgsPolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::NotAllowed),
            "untainted" => Some(Self::AllowedWhenUntainted),
            "tainted" => Some(Self::AllowedWhenTainted),
            _ => None,
        }
    }

    pub fn annotation_name(self) -> &'static str {
        match self {
            Self::NotAllowed => "none",
            Self::AllowedWhenUntainted => "untainted",
            Self::AllowedWhenTainted => "tainted",
        }
    }

    /// Blizzard's `SecretArguments` value, named in hover so it can be matched
    /// against the API documentation.
    pub fn blizzard_name(self) -> &'static str {
        match self {
            Self::NotAllowed => "NotAllowed",
            Self::AllowedWhenUntainted => "AllowedWhenUntainted",
            Self::AllowedWhenTainted => "AllowedWhenTainted",
        }
    }

    /// The inverse of [`Self::blizzard_name`].
    pub fn from_blizzard_name(s: &str) -> Option<Self> {
        [Self::NotAllowed, Self::AllowedWhenUntainted, Self::AllowedWhenTainted]
            .into_iter()
            .find(|p| p.blizzard_name() == s)
    }
}

/// `@secret-args <policy> [param…]`: the policy and the parameters it applies to.
///
/// Blizzard states one `SecretArguments` policy per function, but the step a
/// secret argument trips is per parameter — the Lua library's numeric argument
/// conversion rejects a secret where the same function's string parameters
/// don't — so the annotation may name the parameters it covers. An empty list
/// covers every parameter, which is what a documented policy means.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretArgs {
    pub policy: SecretArgsPolicy,
    /// Parameter names the policy applies to (`...` for the varargs); empty =
    /// every parameter.
    pub params: Vec<String>,
}

impl SecretArgs {
    /// Parse the text after `@secret-args`.
    pub fn parse(rest: &str) -> Option<Self> {
        let mut words = rest.split_whitespace();
        let policy = SecretArgsPolicy::parse(words.next()?)?;
        Some(Self { policy, params: words.map(str::to_string).collect() })
    }

    /// Whether the policy applies to the parameter named `param`.
    pub fn covers(&self, param: &str) -> bool {
        self.params.is_empty() || self.params.iter().any(|p| p == param)
    }

    /// The annotation text after the tag.
    pub fn annotation_text(&self) -> String {
        std::iter::once(self.policy.annotation_name().to_string())
            .chain(self.params.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What a `@secret-guard` function's boolean result says about its argument(s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SecretGuardKind {
    /// `is-secret` — true: the argument is secret; false: it is not.
    IsSecret,
    /// `accessible` — true: every argument is non-secret; false (single
    /// argument): it is secret.
    Accessible,
    /// `any-secret` — true (single argument): it is secret; false: no argument
    /// is secret.
    AnySecret,
}

impl SecretGuardKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "is-secret" => Some(Self::IsSecret),
            "accessible" => Some(Self::Accessible),
            "any-secret" => Some(Self::AnySecret),
            _ => None,
        }
    }

    /// Does a call returning `truthy` prove its arguments non-secret?
    /// `Some(true)` = every argument is non-secret, `Some(false)` = the (single)
    /// argument is secret, `None` = nothing is known.
    pub fn implies_accessible(self, truthy: bool, arg_count: usize) -> Option<bool> {
        let proves_secret = arg_count == 1;
        match (self, truthy) {
            (Self::IsSecret | Self::AnySecret, true) | (Self::Accessible, false) => {
                proves_secret.then_some(false)
            }
            (Self::IsSecret | Self::AnySecret, false) | (Self::Accessible, true) => Some(true),
        }
    }
}

/// `@secret-guard <param> <kind>`: the named parameter (`...` for varargs).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretGuard {
    pub param: String,
    pub kind: SecretGuardKind,
}

/// `@secret-unless <param> <value>...`: a call passing one of the string
/// literals for `param` (e.g. `unit` = `"player"`) returns ordinary values.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretExemption {
    pub param: String,
    pub values: Vec<String>,
}

/// Secrecy metadata attached to a function (from `@secret-*` annotations).
/// Secret *returns* are expressed in the return types themselves (`secret<T>`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SecretMeta {
    pub when: Vec<SecretPredicate>,
    pub args: Option<SecretArgs>,
    pub aspects: Vec<String>,
    pub guard: Option<SecretGuard>,
    pub unless: Option<SecretExemption>,
    pub clears: Option<SecretClears>,
    pub restriction_guard: Option<SecretRestrictionGuard>,
    pub satisfies: Option<SecretSatisfies>,
    pub preconditions: Vec<SecretPrecondition>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_context_guard_annotations() {
        let clears = SecretClears::parse(" SecretWhenA,SecretWhenB unit mobUnit").unwrap();
        assert_eq!(clears.predicates, ["SecretWhenA", "SecretWhenB"]);
        let names = |c: &SecretClears| c.bindings.iter().map(|b| (b.guard_param.clone(), b.callee_params.clone())).collect::<Vec<_>>();
        assert_eq!(names(&clears), [
            ("unit".to_string(), vec!["unit".to_string()]),
            ("mobUnit".to_string(), vec!["mobUnit".to_string()]),
        ]);
        assert!(!clears.all && clears.equals.is_none());
        // A guard parameter may bind to differently named callee parameters.
        let renamed = SecretClears::parse(" SecretWhenA unit=unitToken,unit").unwrap();
        assert_eq!(names(&renamed), [("unit".to_string(), vec!["unitToken".to_string(), "unit".to_string()])]);
        let all = SecretClears::parse(" *").unwrap();
        assert!(all.all && all.predicates.is_empty());
        let equals = SecretClears::parse(" SecretWhenA spell == Enum.SecrecyLevel.NeverSecret").unwrap();
        assert_eq!((names(&equals).as_slice(), equals.equals.as_deref()),
            (&[("spell".to_string(), vec!["spell".to_string()])][..], Some("Enum.SecrecyLevel.NeverSecret")));
        assert!(SecretClears::parse("").is_none());
        assert!(SecretClears::parse(" SecretWhenA unit ==").is_none());
        assert!(SecretClears::parse(" SecretWhenA unit=").is_none());
        // `*` clears the whole region, so it cannot be bound to an argument.
        assert!(SecretClears::parse(" * unit").is_none());

        let satisfies = SecretSatisfies::parse(" RequiresX unit1 unit2").unwrap();
        assert_eq!(satisfies.precondition, "RequiresX");
        assert_eq!(satisfies.bindings.len(), 2);
        assert!(SecretSatisfies::parse("  ").is_none());

        let args = SecretArgs::parse(" untainted i j").unwrap();
        assert_eq!((args.policy, args.params.as_slice()), (SecretArgsPolicy::AllowedWhenUntainted, &["i".to_string(), "j".to_string()][..]));
        assert!(args.covers("i") && !args.covers("s"));
        // A bare policy covers every parameter.
        assert!(SecretArgs::parse(" none").unwrap().covers("anything"));
        assert!(SecretArgs::parse(" sometimes").is_none());

        let fixed = SecretRestrictionGuard::parse(" Combat").unwrap();
        assert_eq!(fixed.fixed_bit(), Some(1));
        let param = SecretRestrictionGuard::parse(" type == Enum.AddOnRestrictionState.Inactive").unwrap();
        assert_eq!((param.fixed_bit(), param.equals.as_deref()), (None, Some("Enum.AddOnRestrictionState.Inactive")));
        assert!(SecretRestrictionGuard::parse(" type extra").is_none());

        let precondition = SecretPrecondition::parse(" RequiresX ReturnNothing Needs access.").unwrap();
        assert_eq!((precondition.failure, precondition.doc.as_deref()), (Some(PreconditionFailure::ReturnNothing), Some("Needs access.")));
        assert!(precondition.returns_nothing());
        let undocumented_mode = SecretPrecondition::parse(" RequiresY Guarded APIs return nothing.").unwrap();
        assert_eq!((undocumented_mode.failure, undocumented_mode.doc.as_deref()), (None, Some("Guarded APIs return nothing.")));
        assert!(!SecretPrecondition::parse(" RequiresZ Error").unwrap().returns_nothing());
        assert!(SecretPrecondition::parse(" ").is_none());
        // A mode in the wrong case is a typo, not a description.
        assert!(SecretPrecondition::parse(" RequiresY returnnothing Needs access.").is_none());
        assert!(SecretPrecondition::parse(" RequiresY ERROR").is_none());
    }

    #[test]
    fn restriction_sets() {
        let path = |p: &str| p.split('.').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(restriction_bit_for_path(&path("Enum.AddOnRestrictionType.Chat")), Some(1 << 5));
        assert_eq!(restriction_bit_for_path(&path("Other.AddOnRestrictionType.Chat")), None);
        let aura = predicate_restrictions("SecretWhenUnitAuraRestricted").unwrap();
        assert_eq!(aura, COMBAT | ENCOUNTER | CHALLENGE_MODE | PVP_MATCH);
        assert_eq!(predicate_restrictions("SecretWhenInCombat"), Some(COMBAT));
        // Unit-condition predicates depend on the unit, not on restrictions.
        assert_eq!(predicate_restrictions("SecretWhenUnitHealthMaxRestricted"), None);
    }
}
