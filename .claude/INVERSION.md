# Cross-file typing inversion

The active effort (decided 2026-11-01, after the release following v0.35.2). Every slice
of it follows this document; a slice that needs to deviate changes this document first.

## Why

The server has two type systems. The per-file engine (`analysis/`) evaluates calls,
expressions, narrowing, generics and metatables. The file boundary has its own: the coarse
syntactic scan (`annotations/scan_globals.rs` and the pattern scanners beside it) records
what a file writes as `FieldValueKind` records, and the two workspace build contexts
(`pre_globals/mod.rs::BuildContext`, `pre_globals/build_on_stubs.rs::BuildOnStubsContext`)
materialize those into tables. Every idiom the engine handles by evaluation needs a
hand-written approximation at the boundary, the idiom space is open, and the
approximations interact. Since v0.33 most fix commits were one such approximation, each
fix moved the corpus somewhere else, and the `crossfile-types` report still lists the same
families after them: module tables assigned in initializers, locale tables, constructor
entries holding calls or expressions, list fields, method returns, implicit `self`.

## Principle

One inference algorithm. The scan discovers **slots**: which cross-file-visible paths a
file writes (namespace, global, `@class` root, any depth), where, and with which author
annotations. The per-file engine, run on the defining files through the deferred harvest
(`analysis/deferred.rs`), supplies every slot's **type**. The boundary is a cache of
engine results, never a translation of syntax into types.

This is the roadmap's north star executed, and the harvest already is that mechanism: it
runs the real engine on a defining file, unions the resolved right-hand sides of every
write to a slot (`field_assignments`), keeps nilability, lifts the result into ext space
(`TableShape`, `FunctionSig`, class refs), installs it into `overlay_fields`, memoizes per
file in `DeferredHarvests` (dependency graph, cut contexts, in-progress cycle breaking)
and is invalidated by rebuilds and `deferred_dep_files`. Today it is gated to placeholder
slots (`field_is_coarse_placeholder`: unannotated `any` or bare `table`), to `@class`
roots, `{}`-written fields, call fields, created globals and deferred returns, and its
result must "say more than `any`". The inversion removes those gates and roots, then
retires the scan's typing.

## Rules (carried from the roadmap; not negotiable per slice)

- Never an eager workspace fixpoint. Demand-driven, memoized, invalidated on edits.
- Author annotations are authoritative: a `@field`, `---@type`, `@param`, `@return` is
  never overridden by a harvested type. Only scan-inferred types are replaced.
- Nilability is preserved: a slot is the union of every writer's right-hand side across
  every writing file; a `nil` write keeps `T?`; `lateinit` (`T!`) reads non-nil.
- Generics stay coarse unless the harvest binds them exactly.
- A sharper type may surface a correct new diagnostic. That is intended. The gate
  triages additions; true positives are reported, not suppressed.
- No new scan patterns. A slot the harvest does not yet cover stays as it is until its
  slice; nothing is added to `scan_globals.rs`, the pattern scanners or the build
  contexts to type it.

## Slices

Each slice ships alone, in this order. A slice is done when all of its acceptance items
hold; a later slice never starts on a tree where an earlier one's acceptance regressed.

1. **Namespace and plain-global table fields, any depth, any right-hand side.** The
   family with the largest read counts: `DBM.Options` (`any` in 1187 reads across 71
   files), `Auctionator.Database` and `Auctionator.Shopping.ListManager` (mixin instances
   assigned in initializers), locale globals created guarded in several files
   (`if not L then L = {} end`) and declared in one, `MT.MTPF.mode`-style deep writes on
   non-namespace roots the scan currently ignores. Existence of such slots is discovered
   by the scan (extend discovery, never typing); the type comes from the harvest.
2. **`@class` fields.** Drop the placeholder gate: every scan-inferred class field is
   harvested, including list element types (`string[][]` read as `table`), protected
   fields read in subclasses, and fields assigned `nil` in one method and a value in
   another. Declared `@field` types stay authoritative.
3. **Constructor entries.** Entries of namespace, global and `@class` constructors whose
   value is a call, a reference or an expression (`Addon = LibStub(..):NewAddon(..)`,
   `IsRetail = A == B`) take the engine's type of the entry, through the write the
   `CtorEntryScan` already emits.
4. **Function signatures.** Method return types read `fun(): any` at call sites while the
   definition shows the declared or body-derived return; implicit `self` displays as
   `fun(self)` at call sites and `fun(self: X)` at the definition (1330 methods on TSM);
   backward-inferred parameter types stay in the defining file. Signatures cross as
   `FunctionSig` with receiver and inferred parameter types.
5. **Retirement.** For every kind the harvest covers: remove the `FieldValueKind` to
   type materialization in both build contexts, delete the pattern scanners that only
   existed to type those slots (`class_var_guesses`, `function_locals` typing,
   `settle_instance_tables`, call-field and constructor typing passes), collapse the
   build contexts where the removed passes were their difference. The cold `build`
   remains for the stub blob, whose sources are declaration files typed by annotations.

## Slice 1 status (2026-11-05, done)

Namespace and plain-global table fields at any depth and workspace globals are slots
harvested from every writing file, read alike by the files writing and reading them
(ARCHITECTURE.md, "Namespace and global table field slots"). The fourth round closed
the 19 paths of this kind the third left open: fields a function defaults and rewrites
on a global its file builds (`DB.list = DB.list or {}`), a `---@type` after a global's
`nil` initializer, an author's `---@type table<T>` the writer's constructor no longer
overrides, a `@class` on a `CreateFromMixins` global an XML template names as its
mixin, a namespace class's field holding a frame with methods defined through it,
fields other files write on frames a call creates by name, a library field written
through a local bound to it in one branch, and an addon namespace two files declare a
class each. `crossfile-types` over `/home/brian/wowlua-ls/.context/addons` reports no
path of this kind (54 to 35 slice-kind paths overall), TradeSkillMaster none; `check`
loses 2 false positives and gains 2 true positives (a number stored in a `string`
field, `SetText` passed a number as 29 times already); no hover typed on main is
untyped (540 gain a type); analyses stay within twice each workspace's file count
(Baganator 218 for 110 files, DeadlyBossMods 280 for 191, TradeSkillMaster 377 for
692), warm times unchanged.

Left to later slices (outside the 27 `<ns>.TutorialData.*` of wow-ui-source, two
addons sharing one namespace for lack of an `addonRoot`):
- 25 instance fields of plain mixin tables, written through `self` or declared by XML
  templates: slice 2.
- 10 constructor entries (`Auctionator.Buying.Events.*`, two flavor files'
  constructors with different keys; `*_LOCALES.enUS`): slice 3.
- `tests/slot-fields`' `LedgerApp.Layout.wide`: a constructor key shows the entry it
  writes, readers the records two files write merged: slice 3.

## Acceptance for every slice

- `crossfile-types` over TradeSkillMaster and `/home/brian/wowlua-ls/.context/addons`
  reports zero disagreeing paths of the slice's kind, except ones documented in the slice
  as cycles the harvest breaks on purpose.
- The before/after gate against the previous main is clean: `check` additions triaged
  (true positives named), `dump-types` typed-to-untyped at zero, run under the load cap
  (`tests/baseline.sh` for the baseline binary, `--profile local` for the new one).
- Cost: `deferred_harvests.analyses()` on the corpus and the warm pass time on
  TradeSkillMaster, DeadlyBossMods and Auctionator are recorded in the slice's commit
  message against the previous slice. Budget: analyses at most twice the file count of
  the workspace, warm time at most three times a `check` of the same tree before the
  slice. A slice over budget is not merged; the harvest's component handling is fixed
  first. Baseline on 2026-11-01: `check` 5.5 s on TSM, 43 s on DeadlyBossMods.
- Fixtures for the kind run on both builds (`scan_dir`, see TESTING.md); the slice adds
  a corpus-shaped fixture per idiom it closes.

## What to stop doing

A report of a type lost at the file boundary is not fixed by a scan or build change any
more. It is assigned to the slice that covers its slot kind, and if that slice has
shipped, it is a bug in the harvest for that kind. The `crossfile-types` report is the
backlog for this effort; user reports are checked against it first.
