//! Property-based tests for the pure type algebra: assignability, union
//! construction, structural subtyping, and the control-flow narrowing helpers.
//!
//! These complement the fixture suite under `tests/`. Fixtures pin specific
//! hand-written scenarios; the properties below assert the *algebraic laws* the
//! type system leans on for soundness — reflexivity of assignability, that
//! `make_union` behaves as a least-upper-bound and normalizes stably, and that
//! flow narrowing never widens a type — across thousands of randomly generated
//! `ValueType`s.
//!
//! Scope notes:
//! * This module lives in the `wowlua_analysis` crate, which depends on
//!   `wowlua_core`. The `ValueType` algebra is reached through `crate::types`
//!   (re-exported from `wowlua_core` by this crate's `lib.rs`) and
//!   `is_table_subtype_impl` is local to this crate (`super::`), so the module is
//!   self-contained and makes no deeper module-path assumptions.
//! * `is_assignable_to`, `make_union`, and the `strip_*`/`filter_*` narrowing
//!   helpers never dereference an arena index, so they are exercised with a
//!   generator that *does* mint `Table(Some(_))`/`Function(Some(_))` indices
//!   (compared only for equality). `is_table_subtype_impl` *does* dereference
//!   table indices, so it gets a populated `Ir` for the real table indices and
//!   an index-free generator for everything else.

use proptest::prelude::*;
use std::sync::Arc;

use super::{is_table_subtype_impl, Analysis, AnalysisConfig};
use crate::pre_globals::PreResolvedGlobals;
use crate::types::{
    EnumKind, FunctionIndex, FunctionShape, TableIndex, TableShape, ValueType,
};

// ── Generators ────────────────────────────────────────────────────────────────

/// Index-free leaves: every variant whose payload carries no arena index, so
/// they are safe to feed to `is_table_subtype_impl` against a populated `Ir`
/// without risking a dangling `Table(Some(_))` dereference.
fn leaf_indexfree() -> BoxedStrategy<ValueType> {
    prop_oneof![
        Just(ValueType::Any),
        Just(ValueType::Nil),
        Just(ValueType::Number),
        Just(ValueType::Userdata),
        Just(ValueType::Thread),
        Just(ValueType::Boolean(None)),
        Just(ValueType::Boolean(Some(true))),
        Just(ValueType::Boolean(Some(false))),
        Just(ValueType::String(None)),
        Just(ValueType::Function(None)),
        Just(ValueType::Table(None)),
        prop::sample::select(vec!["a", "b", "c"])
            .prop_map(|s| ValueType::String(Some(s.to_string()))),
        prop::sample::select(vec!["0", "1", "-1"])
            .prop_map(|s| ValueType::NumberLiteral(s.to_string())),
        prop::sample::select(vec!["T", "U", "K"])
            .prop_map(|s| ValueType::TypeVariable(s.to_string())),
    ]
    .boxed()
}

/// Leaves that additionally mint arena-indexed `Table`/`Function` variants.
/// Safe ONLY for functions that never dereference the index (`is_assignable_to`,
/// `make_union`, `strip_*`, `filter_type_with`/`strip_type_with` with a constant
/// enum-kind closure) — i.e. everything except `is_table_subtype_impl`.
fn leaf_indexed() -> BoxedStrategy<ValueType> {
    prop_oneof![
        8 => leaf_indexfree(),
        1 => (0usize..3).prop_map(|i| ValueType::Table(Some(TableIndex::from(i)))),
        1 => (0usize..3).prop_map(|i| ValueType::Function(Some(FunctionIndex::from(i)))),
    ]
    .boxed()
}

/// Recursive `ValueType` strategy over a given leaf strategy, defined once. The
/// `Union`/`Intersection` arms are opt-in so callers can carve out constrained
/// sub-strategies *by construction* (rather than generating-then-`prop_assume!`-
/// filtering, which inflates proptest's global-reject count and breaks under
/// higher case counts). The non-union/non-intersection composite arms
/// (`OpaqueAlias`/`TableShape`/`FunctionSig`) are always present, so adding a new
/// `ValueType` variant means editing this one place.
///
/// Unions are built through `make_union` (so they are normalized, exactly like
/// values flowing through the real system); intersections are built raw (the type
/// system has no intersection normalizer). The flags gate only the *top-level*
/// arms at each recursion level — equivalently, when a flag is false that variant
/// appears nowhere in the produced value.
fn value_type_from_opts(
    leaf: BoxedStrategy<ValueType>,
    include_union: bool,
    include_intersection: bool,
) -> BoxedStrategy<ValueType> {
    leaf.prop_recursive(4, 32, 3, move |inner| {
        let mut arms: Vec<(u32, BoxedStrategy<ValueType>)> = Vec::new();
        if include_union {
            arms.push((
                3,
                prop::collection::vec(inner.clone(), 1..4)
                    .prop_map(ValueType::make_union)
                    .boxed(),
            ));
        }
        if include_intersection {
            arms.push((
                2,
                prop::collection::vec(inner.clone(), 2..4)
                    .prop_map(ValueType::Intersection)
                    .boxed(),
            ));
        }
        arms.push((
            1,
            (prop::sample::select(vec!["Op1", "Op2"]), inner.clone())
                .prop_map(|(n, t)| ValueType::OpaqueAlias(n.to_string(), Box::new(t)))
                .boxed(),
        ));
        arms.push((
            1,
            (prop::sample::select(vec!["f", "g"]), inner.clone())
                .prop_map(|(n, t)| ValueType::TableShape(Box::new(TableShape::new(vec![(n.to_string(), t)]))))
                .boxed(),
        ));
        arms.push((
            1,
            inner
                .prop_map(|r| {
                    ValueType::FunctionSig(Box::new(FunctionShape {
                        params: vec![],
                        returns: vec![r],
                        is_vararg: false,
                    }))
                })
                .boxed(),
        ));
        proptest::strategy::Union::new_weighted(arms)
    })
    .boxed()
}

/// General generator (may contain `Table(Some)`/`Function(Some)` and every
/// composite variant).
fn arb_value_type() -> BoxedStrategy<ValueType> {
    value_type_from_opts(leaf_indexed(), true, true)
}

/// Index-free generator, safe against a populated `Ir`.
fn arb_value_type_indexfree() -> BoxedStrategy<ValueType> {
    value_type_from_opts(leaf_indexfree(), true, true)
}

/// Generator that never produces an `Intersection`. Used by the union
/// least-upper-bound / commutativity / associativity laws (which rely on
/// `is_assignable_to` as their oracle and would otherwise trip the documented
/// intersection-in-union completeness gap, see
/// `intersection_in_union_is_a_known_assignability_gap`) and as the *actual* in
/// the intersection-target arm laws (an `Intersection` actual is caught by an
/// earlier arm with different quantifier nesting).
fn arb_no_intersection() -> BoxedStrategy<ValueType> {
    value_type_from_opts(leaf_indexed(), true, false)
}

/// Generator that produces neither a `Union` nor an `Intersection`, used as the
/// *actual* in `union_expected_iff_any_member`: that law's `any`-member oracle
/// only matches the `(actual, Union(types))` arm, which fires only for an actual
/// that is not itself a `Union`/`Intersection` (those hit earlier arms whose
/// quantifier nesting differs).
fn arb_non_union_non_intersection() -> BoxedStrategy<ValueType> {
    value_type_from_opts(leaf_indexed(), false, false)
}

/// The realistic domain of guards passed to `filter_type_with`/`strip_type_with`:
/// the result types of Lua's `type()` builtin (and small unions of them). These
/// helpers are only ever invoked with such guards by `type(x) == "..."` narrowing
/// — feeding them arbitrary types (e.g. opaque aliases, which don't self-match
/// under `matches_type_guard_with`) tests behavior the engine never exercises.
fn arb_type_guard() -> BoxedStrategy<ValueType> {
    let base = prop_oneof![
        Just(ValueType::String(None)),
        Just(ValueType::Number),
        Just(ValueType::Boolean(None)),
        Just(ValueType::Table(None)),
        Just(ValueType::Function(None)),
        Just(ValueType::Nil),
        Just(ValueType::Thread),
        Just(ValueType::Userdata),
    ];
    prop_oneof![
        3 => base.clone(),
        1 => prop::collection::vec(base, 2..4).prop_map(ValueType::make_union),
    ]
    .boxed()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn assignable(a: &ValueType, b: &ValueType) -> bool {
    a.is_assignable_to(b)
}

/// Semantic type equivalence: mutual assignability. Used for laws like
/// commutativity where the *representation* (member order) is not canonical but
/// the *type* must be the same.
fn type_eq(a: &ValueType, b: &ValueType) -> bool {
    a.is_assignable_to(b) && b.is_assignable_to(a)
}

/// Flatten a value into its top-level union members (a singleton list for a
/// non-union).
fn members(t: &ValueType) -> Vec<ValueType> {
    match t {
        ValueType::Union(m) => m.clone(),
        other => vec![other.clone()],
    }
}

/// A normalized union must have no nested-`Union` members and no duplicates.
fn is_normalized_union(t: &ValueType) -> bool {
    match t {
        ValueType::Union(m) => {
            let no_nested = m.iter().all(|x| !matches!(x, ValueType::Union(_)));
            let no_dups = (0..m.len()).all(|i| !m[i + 1..].contains(&m[i]));
            no_nested && no_dups
        }
        _ => true,
    }
}

/// Constant enum-kind oracle for the `type()`-guard narrowing helpers — we are
/// testing the union-algebra plumbing, not enum classification, so no index is
/// ever a real enum here.
fn no_enum(_: TableIndex) -> EnumKind {
    EnumKind::NotEnum
}

/// Structural "is a narrowing of" check: every top-level member of `narrowed` is
/// justified by some member of `original` (either the same member, or a strict
/// refinement of it such as `true` for a `boolean`). This is the precise
/// "never widens" invariant and — unlike `narrowed.is_assignable_to(original)` —
/// it compares member-to-member, so it sidesteps the intersection-in-union
/// assignability gap (which is an oracle artifact, not a narrowing bug).
/// Generated unions are normalized, so members are never themselves unions and
/// `is_assignable_to` here never sees a `Union` on either side.
fn is_narrowing_of(narrowed: &ValueType, original: &ValueType) -> bool {
    let orig = members(original);
    members(narrowed)
        .iter()
        .all(|nm| orig.iter().any(|om| nm == om || nm.is_assignable_to(om)))
}

// ── is_assignable_to ────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(768))]

    /// Reflexivity: every type is assignable to itself (the `self == expected`
    /// fast path must hold for every constructible value).
    #[test]
    fn assignable_is_reflexive(t in arb_value_type()) {
        prop_assert!(assignable(&t, &t), "not reflexive: {t:?}");
    }

    /// `Any` is the universal top *and* bottom: everything is assignable to it
    /// and it is assignable to everything.
    #[test]
    fn any_is_top_and_bottom(t in arb_value_type()) {
        prop_assert!(assignable(&t, &ValueType::Any), "{t:?} not assignable to Any");
        prop_assert!(assignable(&ValueType::Any, &t), "Any not assignable to {t:?}");
    }

    /// A union is assignable to a (non-intersection) target iff *every* member
    /// is (`(Union(types), expected)` arm). Uses a raw union so the arm is hit
    /// directly rather than a normalized form. The target is drawn
    /// intersection-free: an `Intersection` target routes a `Union` actual
    /// through the earlier `(actual, Intersection)` arm (decomposing per-target-
    /// member), which for nested intersections computes a different quantifier
    /// nesting than this property's per-union-member oracle — the `Union →
    /// Intersection` case is covered consistently by
    /// `intersection_expected_iff_all_members` instead.
    #[test]
    fn union_actual_iff_all_members(
        ms in prop::collection::vec(arb_value_type(), 1..4),
        e in arb_no_intersection(),
    ) {
        let u = ValueType::Union(ms.clone());
        let all = ms.iter().all(|m| assignable(m, &e));
        prop_assert_eq!(assignable(&u, &e), all, "union={:?} e={:?}", u, e);
    }

    /// A (non-union, non-intersection) value is assignable to a union iff it is
    /// assignable to *some* member (`(actual, Union(types))` arm). The actual is
    /// restricted because a `Union`/`Intersection` actual is caught by an earlier
    /// arm whose quantifier nesting differs (tested separately above/below).
    ///
    /// One deliberate, conservative exception is encoded here: bare `Nil` is
    /// assignable to a union only when `nil` is an *explicit* member (the
    /// optional-parameter rule `(Nil, Union(types)) => types.contains(&Nil)`).
    /// This is a documented false-negative (safe: it never accepts an invalid
    /// assignment), so we mirror it rather than flag it.
    #[test]
    fn union_expected_iff_any_member(
        a in arb_non_union_non_intersection(),
        ms in prop::collection::vec(arb_value_type(), 1..4),
    ) {
        let u = ValueType::Union(ms.clone());
        let expected = if matches!(a, ValueType::Nil) {
            ms.contains(&ValueType::Nil)
        } else {
            ms.iter().any(|m| assignable(&a, m))
        };
        prop_assert_eq!(assignable(&a, &u), expected, "a={:?} union={:?}", a, u);
    }

    /// A (non-intersection) value is assignable to an intersection iff it is
    /// assignable to *all* members (`(actual, Intersection(types))` arm). The
    /// actual is restricted because an `Intersection` actual is caught by the
    /// earlier `(Intersection, Intersection)` arm, whose `all(expected: any(actual))`
    /// nesting differs from this property's `all(member)` oracle. No `Nil`
    /// exception applies to intersection targets.
    #[test]
    fn intersection_expected_iff_all_members(
        a in arb_no_intersection(),
        ms in prop::collection::vec(arb_value_type(), 2..4),
    ) {
        let i = ValueType::Intersection(ms.clone());
        let all = ms.iter().all(|m| assignable(&a, m));
        prop_assert_eq!(assignable(&a, &i), all, "a={:?} inter={:?}", a, i);
    }

    /// An intersection is assignable to a (non-intersection) target iff *some*
    /// member is (`(Intersection(types), expected)` arm). The target is drawn
    /// intersection-free (an `Intersection` target hits the earlier
    /// `(Intersection, Intersection)` arm with different quantifier nesting).
    #[test]
    fn intersection_actual_iff_any_member(
        ms in prop::collection::vec(arb_value_type(), 2..4),
        e in arb_no_intersection(),
    ) {
        let i = ValueType::Intersection(ms.clone());
        let any = ms.iter().any(|m| assignable(m, &e));
        prop_assert_eq!(assignable(&i, &e), any, "inter={:?} e={:?}", i, e);
    }
}

/// **Known completeness gap in `is_assignable_to` (documented, not a soundness
/// bug).**
///
/// `is_assignable_to` reaches the `(Intersection(types), expected)` arm — "an
/// intersection is assignable to X if *any* member is" — *before* it would ever
/// consult union membership via `(actual, Union(types))`. Consequently an
/// intersection value is NOT recognized as assignable to a union that literally
/// contains it as a member, unless one of its individual members is also
/// assignable to that union on its own.
///
/// This is a conservative *false negative* (it can only ever cause a spurious
/// diagnostic, never accept an invalid assignment), but it does mean `make_union`
/// is not a strict least-upper-bound for intersection inputs — which is why the
/// union LUB / commutativity / associativity properties above draw from
/// `arb_no_intersection`, and why `strip_never_widens` uses a structural oracle
/// rather than `is_assignable_to`.
///
/// This test PINS the current behavior. If `is_assignable_to` is ever taught to
/// check union membership first, this assertion will fail — at which point the
/// `arb_no_intersection` workarounds in this module can be removed.
#[test]
fn intersection_in_union_is_a_known_assignability_gap() {
    let inter = ValueType::Intersection(vec![ValueType::Number, ValueType::Userdata]);
    let u = ValueType::make_union(vec![inter.clone(), ValueType::Nil]);
    // `inter` is literally a member of `u`, yet assignability does not see it:
    assert!(
        members(&u).contains(&inter),
        "test setup: intersection should be a member of the union"
    );
    assert!(
        !inter.is_assignable_to(&u),
        "assignability gap closed — an intersection is now assignable to a union \
         that contains it; remove the arb_no_intersection workarounds in this module"
    );
}

// ── make_union ──────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(768))]

    /// Idempotent dedup: `T | T` collapses to a single `T` (structurally).
    #[test]
    fn union_dedup_is_idempotent(t in arb_value_type()) {
        let once = ValueType::make_union(vec![t.clone()]);
        let twice = ValueType::make_union(vec![t.clone(), t.clone()]);
        prop_assert_eq!(once, twice, "T|T != T for {:?}", t);
    }

    /// `T | T` is the same type as `T`.
    #[test]
    fn union_self_is_self(t in arb_value_type()) {
        let u = ValueType::make_union(vec![t.clone(), t.clone()]);
        prop_assert!(type_eq(&u, &t), "T|T not type-eq T: {:?}", t);
    }

    /// Commutativity (semantic): member order is not part of a union's identity.
    /// Intersection-free inputs (see `arb_no_intersection`) — the oracle is
    /// `type_eq`, which would otherwise trip the documented intersection gap.
    #[test]
    fn union_is_commutative(a in arb_no_intersection(), b in arb_no_intersection()) {
        let ab = ValueType::make_union(vec![a.clone(), b.clone()]);
        let ba = ValueType::make_union(vec![b.clone(), a.clone()]);
        prop_assert!(type_eq(&ab, &ba), "a|b != b|a for a={:?} b={:?}", a, b);
    }

    /// Associativity / flattening (semantic): nested unions flatten.
    /// Intersection-free inputs for the same oracle reason as commutativity.
    #[test]
    fn union_is_associative(a in arb_no_intersection(), b in arb_no_intersection(), c in arb_no_intersection()) {
        let nested = ValueType::make_union(vec![ValueType::make_union(vec![a.clone(), b.clone()]), c.clone()]);
        let flat = ValueType::make_union(vec![a.clone(), b.clone(), c.clone()]);
        prop_assert!(type_eq(&nested, &flat), "(a|b)|c != a|b|c");
    }

    /// `Any` absorbs: `T | Any` is `Any`.
    #[test]
    fn union_any_absorbs(t in arb_value_type()) {
        let u = ValueType::make_union(vec![t.clone(), ValueType::Any]);
        prop_assert!(type_eq(&u, &ValueType::Any), "T|Any != Any for {:?}", t);
    }

    /// Least-upper-bound soundness: every input flows into the union it built.
    /// Restricted to intersection-free inputs: `make_union` is NOT a strict
    /// upper bound for an `Intersection` input, because `is_assignable_to` fails
    /// to recognize an intersection as assignable to a union containing it (see
    /// `intersection_in_union_is_a_known_assignability_gap`).
    #[test]
    fn union_is_upper_bound(ms in prop::collection::vec(arb_no_intersection(), 1..5)) {
        let u = ValueType::make_union(ms.clone());
        for m in &ms {
            prop_assert!(assignable(m, &u), "{:?} not assignable to its union {:?}", m, u);
        }
    }

    /// Normalization is stable: normalizing twice equals normalizing once.
    #[test]
    fn union_normalization_is_stable(ms in prop::collection::vec(arb_value_type(), 1..5)) {
        let once = ValueType::make_union(ms);
        let twice = ValueType::make_union(members(&once));
        prop_assert_eq!(&once, &twice, "make_union not idempotent");
    }

    /// The output is always a well-formed union (no nested unions, no dups).
    #[test]
    fn union_output_is_normalized(ms in prop::collection::vec(arb_value_type(), 1..5)) {
        let u = ValueType::make_union(ms);
        prop_assert!(is_normalized_union(&u), "make_union produced denormalized {:?}", u);
    }
}

// ── narrowing helpers ───────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(768))]

    /// Flow narrowing never widens: every member of the stripped type is
    /// justified by a member of the original (`strip_nil`, the truthiness guard
    /// `strip_falsy`, and the falsy-region `strip_truthy`). Uses the structural
    /// `is_narrowing_of` oracle so the check is exact even when the original
    /// carries an intersection member (which the strips keep verbatim).
    #[test]
    fn strip_never_widens(t in arb_value_type()) {
        prop_assert!(is_narrowing_of(&t.strip_nil(), &t), "strip_nil widened {:?}", t);
        prop_assert!(is_narrowing_of(&t.strip_falsy(), &t), "strip_falsy widened {:?}", t);
        prop_assert!(is_narrowing_of(&t.strip_truthy(), &t), "strip_truthy widened {:?}", t);
    }

    /// `strip_nil` removes every nil possibility.
    #[test]
    fn strip_nil_removes_nil(t in arb_value_type()) {
        prop_assert!(!t.strip_nil().contains_nil(), "strip_nil left nil in {:?}", t);
    }

    /// `strip_falsy` removes nil and `false` (and collapses a bare boolean to
    /// `true`), so the result carries no residual falsy member.
    #[test]
    fn strip_falsy_removes_falsy(t in arb_value_type()) {
        let stripped = t.strip_falsy();
        let bad = |x: &ValueType| matches!(
            x,
            ValueType::Nil | ValueType::Boolean(Some(false)) | ValueType::Boolean(None)
        );
        prop_assert!(
            members(&stripped).iter().all(|m| !bad(m)),
            "strip_falsy left a falsy member in {:?} -> {:?}", t, stripped
        );
    }

    /// All three flow-narrowing strips are idempotent (applying twice == once).
    #[test]
    fn strip_is_idempotent(t in arb_value_type()) {
        prop_assert_eq!(t.strip_nil().strip_nil(), t.strip_nil(), "strip_nil not idempotent");
        prop_assert_eq!(t.strip_falsy().strip_falsy(), t.strip_falsy(), "strip_falsy not idempotent");
        prop_assert_eq!(t.strip_truthy().strip_truthy(), t.strip_truthy(), "strip_truthy not idempotent");
    }

    /// `@cast`/`type()`-guard narrowing (`filter_type_with` keeps matches,
    /// `strip_type_with` removes them) is idempotent. These have deliberate
    /// "fallback to the guard / to nil when nothing remains" asymmetries (so
    /// they are NOT subtype-monotonic), but re-applying the same guard must be a
    /// no-op — asserted semantically since the fallback makes representation
    /// fragile.
    #[test]
    fn type_guard_narrowing_is_idempotent(t in arb_value_type(), g in arb_type_guard()) {
        let f1 = t.filter_type_with(&g, &no_enum);
        let f2 = f1.filter_type_with(&g, &no_enum);
        prop_assert!(type_eq(&f1, &f2), "filter_type_with not idempotent: {:?} | {:?}", t, g);

        let s1 = t.strip_type_with(&g, &no_enum);
        let s2 = s1.strip_type_with(&g, &no_enum);
        prop_assert!(type_eq(&s1, &s2), "strip_type_with not idempotent: {:?} | {:?}", t, g);
    }

    /// `filter` then `strip` by the same guard annihilates: nothing matching the
    /// guard can survive a keep-only-matches followed by a remove-all-matches,
    /// so the result is `Nil` (the documented "stripped everything" sentinel).
    #[test]
    fn filter_then_strip_same_guard_is_nil(t in arb_value_type(), g in arb_type_guard()) {
        let filtered = t.filter_type_with(&g, &no_enum);
        let stripped = filtered.strip_type_with(&g, &no_enum);
        prop_assert_eq!(stripped, ValueType::Nil, "filter|>strip not Nil for {:?} | {:?}", t, g);
    }
}

// ── is_table_subtype_impl (populated Ir) ────────────────────────────────────

/// Build a resolved analysis over `src` with empty precomputed globals, so the
/// only tables are the ones declared in `src`.
fn build_result(src: &str) -> super::AnalysisResult {
    let tree = crate::syntax::parser::Parser::new(src).parse();
    let pre = Arc::new(PreResolvedGlobals::empty());
    let mut analysis = Analysis::new_with_tree(&tree, pre, AnalysisConfig::default());
    analysis.resolve_types();
    analysis.into_result()
}

/// A small class hierarchy plus an unrelated class and a number enum, used to
/// exercise the structural/subclass paths of `is_table_subtype_impl`.
const HIERARCHY_SRC: &str = "\
---@class PtA
---@field a number

---@class PtB : PtA
---@field b number

---@class PtC : PtB
---@field c number

---@class PtD
---@field d number

---@enum PtEnum
local PtEnum = { X = 1, Y = 2 }
";

#[test]
fn table_subtype_respects_class_hierarchy() {
    let result = build_result(HIERARCHY_SRC);
    let ir = &result.ir;
    let cache = &result.resolved_expr_cache[..];
    let tbl = |i: TableIndex| ValueType::Table(Some(i));

    let names = ["PtA", "PtB", "PtC", "PtD"];
    let idxs: Vec<TableIndex> = names
        .iter()
        .map(|n| *ir.classes.get(*n).unwrap_or_else(|| panic!("class {n} not registered")))
        .collect();
    let (a, b, c, d) = (idxs[0], idxs[1], idxs[2], idxs[3]);

    // Subclass instances are subtypes of their ancestors (direct and transitive).
    assert!(is_table_subtype_impl(ir, cache, &tbl(b), &tbl(a)), "B should be subtype of A");
    assert!(is_table_subtype_impl(ir, cache, &tbl(c), &tbl(b)), "C should be subtype of B");
    assert!(is_table_subtype_impl(ir, cache, &tbl(c), &tbl(a)), "C should be subtype of A");

    // Strict-subclassing is asymmetric: an ancestor is not a subtype of its
    // descendant, and unrelated classes are not subtypes of one another.
    assert!(!is_table_subtype_impl(ir, cache, &tbl(a), &tbl(b)), "A should NOT be subtype of B");
    assert!(!is_table_subtype_impl(ir, cache, &tbl(d), &tbl(a)), "D should NOT be subtype of A");

    // A number enum is bidirectionally compatible with `number`.
    let enum_idx = ir
        .local_tables()
        .find(|(_, t)| t.enum_kind == EnumKind::Number)
        .map(|(i, _)| i)
        .expect("number enum table not found");
    assert!(
        is_table_subtype_impl(ir, cache, &tbl(enum_idx), &ValueType::Number),
        "number enum should be subtype of number"
    );
    assert!(
        is_table_subtype_impl(ir, cache, &ValueType::Number, &tbl(enum_idx)),
        "number should be subtype of number enum"
    );
}

#[test]
fn table_subtype_properties() {
    let result = build_result(HIERARCHY_SRC);
    let ir = &result.ir;
    let cache = &result.resolved_expr_cache[..];

    let idxs: Vec<TableIndex> = ["PtA", "PtB", "PtC", "PtD"]
        .iter()
        .map(|n| *ir.classes.get(*n).unwrap())
        .collect();

    // Each real class instance is its own subtype (subclass reflexivity).
    for &i in &idxs {
        let t = ValueType::Table(Some(i));
        assert!(
            is_table_subtype_impl(ir, cache, &t, &t),
            "is_table_subtype not reflexive on table {i:?}"
        );
    }

    // Pick either a real class-instance type or an index-free type, so random
    // pairs exercise both the table-arena paths and the union/intersection
    // decomposition without ever dereferencing a dangling index.
    let real = prop::sample::select(idxs.clone()).prop_map(|i| ValueType::Table(Some(i)));
    let strat = prop_oneof![3 => real, 1 => arb_value_type_indexfree()];

    // The combined relation the engine actually uses everywhere a structural
    // subtype check is needed.
    let sub = |x: &ValueType, y: &ValueType| {
        x.is_assignable_to(y) || is_table_subtype_impl(ir, cache, x, y)
    };

    proptest!(|(x in strat.clone(), y in strat.clone())| {
        // No panic on any pair (implicit), and the combined relation is reflexive.
        prop_assert!(sub(&x, &x), "combined subtype relation not reflexive on {:?}", x);

        // Documented relationship: a genuine subclass is always a structural
        // subtype. (This is the soundness core of is_table_subtype_impl.)
        if let (ValueType::Table(Some(p)), ValueType::Table(Some(q))) = (&x, &y)
            && ir.is_subclass_of(*p, *q)
        {
            prop_assert!(
                is_table_subtype_impl(ir, cache, &x, &y),
                "subclass {:?} not a structural subtype of {:?}", p, q
            );
        }
    });
}
