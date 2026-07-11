//! Shared annotation **type definitions** (the data model only).
//!
//! These live in the core crate — below `analysis` — because the IR types in
//! [`crate::types`] embed them (e.g. raw `@return`/`@param` annotation types on
//! `Function`). The annotation *parsing* and *scanning* logic stays in the
//! higher `annotations` module, which re-exports these names so existing
//! `crate::annotations::AnnotationType` paths keep resolving.

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AnnotationType {
    Simple(String),
    Union(Vec<AnnotationType>),
    Array(Box<AnnotationType>),                  // T[], integer[]
    Parameterized(String, Vec<AnnotationType>),  // table<K, V>
    Backtick(Box<AnnotationType>),               // `T` — infer from string literal as class name
    Fun(Vec<ParamInfo>, Vec<AnnotationType>, bool), // fun(x: T): R — params, returns, is_vararg
    NonNil(Box<AnnotationType>),                 // T! — non-nil assertion / lateinit
    Intersection(Vec<AnnotationType>),            // T & U — intersection of types
    TableLiteral(Vec<(String, AnnotationType)>),  // {field: type, ...} — anonymous table shape
    VarArgs(Box<AnnotationType>),                // ...T — variadic return expansion
    IndexedAccess(String, Box<AnnotationType>),  // T[K] — indexed access type
    /// `keyof X` — a string that is one of `X`'s field/method names. `X` is
    /// `self` (the call receiver, resolved per call site) or a class/generic
    /// name. Resolves to a union of `X`'s key string-literals; a non-key literal
    /// is a type-mismatch, and go-to-definition/hover on the literal jump to the
    /// named field. Composes in intersections (`FrameEvent & keyof self`).
    KeyOf(String),
    /// `(T1 name1, T2 name2, ...)` — multi-value return tuple. Only valid in
    /// return position (top-level of `@return`, inside `fun(): ...`, as an
    /// `@alias` body). The optional `description` is per-case text from the
    /// trailing comment on the tuple's line (e.g. `(true, number) success`).
    /// A `Union` whose members are all `Tuple` is a correlated tuple-union
    /// (`(true, number) | (false, nil)`).
    Tuple(Vec<TuplePosition>, Option<String>),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TuplePosition {
    pub typ: AnnotationType,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParamInfo {
    pub name: String,
    pub typ: AnnotationType,
    pub optional: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
}

/// Sentinel target name for `keyof self` — resolved against the call's
/// receiver table rather than another bound generic. See
/// `CallResolution::resolve_keyof_target`.
pub const KEYOF_SELF_TARGET: &str = "self";
