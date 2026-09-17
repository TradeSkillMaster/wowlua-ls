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

/// Passing a secret to a C API with the given `SecretArguments` policy (no
/// policy = unknown, never diagnosed).
pub fn argument_rule(policy: Option<SecretArgsPolicy>) -> SecretRule {
    match policy {
        // VERIFIED: "will never accept secret values, even from untainted callers".
        Some(SecretArgsPolicy::NotAllowed) => SecretRule::Error,
        // VERIFIED accepted from tainted callers ("resulting in secret strings"
        // for string.format/concat/join); ASSUMED that every such API's results
        // inherit the secrecy of its arguments.
        Some(SecretArgsPolicy::AllowedWhenTainted) => SecretRule::Propagate,
        // VERIFIED that tainted callers are rejected, but Blizzard's generated
        // docs mark nearly every function this way — including `issecretvalue`
        // itself — so the data can't separate real rejections from defaults.
        Some(SecretArgsPolicy::AllowedWhenUntainted) | None => SecretRule::Allowed,
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
    pub args: Option<SecretArgsPolicy>,
    pub aspects: Vec<String>,
    pub guard: Option<SecretGuard>,
    pub unless: Option<SecretExemption>,
}
