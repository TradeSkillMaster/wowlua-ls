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
   call-field and constructor typing passes), collapse the
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

## Slice 2 status (2026-11-06, first round)

Every field of a workspace `@class` the scan typed (no `@field`, `---@type` or XML
declaration, not a function the scan typed) is harvested, not just its placeholders
(ARCHITECTURE.md, "`@class` field slots"). A field one file writes reads everywhere as
that file reads it; one several files write (two flavor files of one mixin) as the
union of their writes, which each writer reads too. A mixin's own file reads the
fields its XML template and other files give the class; a class declared in several
files reads, in each, the fields another writes. `crossfile-types` over the corpus:
56 of the 188 non-function disagreeing paths on main agree (16 of Baganator's 17 mixin
paths, Auctionator's 7, 23 of DeadlyBossMods', TradeSkillMaster's `OptionalMat.Info`,
`Thread._co`), 2 newly disagree (below). `dump-types`: no hover typed on main is
untyped, 39462 gain a type. Analyses (main → now): TradeSkillMaster 377 → 451 (692
files), DeadlyBossMods 280 → 270, Auctionator 209 → 255, Baganator 218 → 206, every
workspace within twice its files; `check` times unchanged.

Two changes to the harvest's component handling paid for it: a cycle's round
re-analyzes a file only when the part of a member it read changed (`ReadPart`), not
the member as a whole; and a class with one Lua file is harvested with that file's
other data (`FileHarvest::classes`), the same analysis. A harvest analysis reads a
field the scan typed by the scan's type, as on main, but for its placeholders:
discovery can't see the classes a file reads through resolved types, and harvesting
them along edges it misses re-harvested DeadlyBossMods' core in a second context.

`check`: DeadlyBossMods −15 (`undefined-field` on `DBM.Revision`/`ReleaseRevision`,
`DBMGUI.Enums`), UtilityHub −1, libresinfo-2.0 −22, Auctionator −11/+3 (`EditBox:GetNumber`
and `ScrollToOffset` passed extra arguments), TestProject 4 messages changed (an existing
`function & table` false positive now naming the field's `string`), wow-ui-source −23/+141:
the mixins' own files now see their templates' frames, so existing checks reach them —
`SetText`/`SetAtlas`/`SetShown` given numbers, `nil` or tables (the family slice 1
counted as true positives), secret values, extra arguments, and ~20 `undefined-field` /
handler mismatches from `GetParent()`'s bare `Frame` and `SetScript` handler typing that
other files reading those frames already report.

## Slice 2 status (2026-11-07, second round)

The writes round 1 left out reach the harvest, and every file writing a field reads it
as readers do (ARCHITECTURE.md, "`@class` field slots"):
- A class's harvest reads, from one of its files, what another of its files returns
  (`runner.Warper:New()`): a class unit depends on the file units of its own files,
  harvested before it, rather than cutting them (`ComponentFrame::reads_before`).
- A function body's write through a class's global (`MyAddon.flag = v`) gives the class
  the field (`shared::foreign_class_table`, both builds).
- A call field with members written through it whose call the build can't type is the
  scan's placeholder, harvested like any scan-typed field (`keep_call_field_methods`).
- A file writing members onto a class field's value (an optional field's too) reads the
  field with them carried, from its own writes after the fixpoint
  (`Analysis::carry_own_sub_field_writes`), never a harvest of itself; a field's one
  writer reads the members other files carry (`ClassField::carried`).
- A value of a class another file gives (a harvested return, a list element) holds the
  fields the file declaring the class writes as the file's own tables do
  (`Ir::class_stand_ins`); a file writing a field through one without being one of the
  class's files reads the workspace's type. A class one file declares several times reads
  each field off all of the file's copies of it, in both engines, a constructor's `nil`
  entry declaring the field (`Ir::class_peers`).
- A `self` write in a method of a local table the file exports to a namespace field
  is that field's slot (discovery only: a `ForeignField` at the export).
- `c and x or y` gives `x` where the `and` holds (`Ir::typed_operands`), and
  `crossfile-types` keys `a and b` by `b` only when `a` is typed.

`crossfile-types`: 14 findings agree (`DBMTest.timeWarper`, `DBMMod.{lastKillTime,
lastWipeTime, combatInfo.pull}`, `MacroToolkit.{MTPF, usingiconlib}`, `ListRow._frame`,
`ScrollTable._header`, `TalentViewerTWW.db`, `<ns>.TreeViewer.currentTreeID`, and four
`and`-guarded reads the tool keyed by their right operand), none newly disagree.
`check`: −6 false positives (`undefined-field` on keys of their own constructors in
MythicPlusPull and Numdelicious_QoL_Tweaks, `cannot-call` on a callback a guard checks in
DeadlyBossMods), +4 true positives (`type-mismatch` in DeadlyBossMods: a timer field read
off all of its class's copies is `number | string | nil`, a constructor keeping a string
argument unconverted, and is passed to `number` parameters). `dump-types`: no hover typed
on main is untyped, 233 gain a type. Analyses (main → now): TradeSkillMaster 451 → 451
(820 files), DeadlyBossMods 270 → 268, Auctionator 254–256 on both (it varies run to
run), Baganator 206 → 206, UtilityHub 31 → 32; `check` times unchanged.

Open, by cause:
- Constructor entries (slice 3): `DBMMod.{categorySort, id, localization…, revision,
  vb…, addon}`, `DBMPanel.frame`, `DBTBar.{frame, enlarged}`, `ScannerData.{continentID,
  mapID}`, the `UtilityHub` class constructor's entries.
- Generic call results read cross-file coarse (the rule): `MacroToolkit.db.*`,
  `UtilityHub.Database.global.*` (`AceDB:New(...)` profile fields).
- The scan's guess for a call it can't type (slice 5 retires it): `TalentViewerTWW.treeId`
  is written only a call into a library the workspace lacks, so its file reads `any`
  and readers the scan's `table`. The writer can't take the guess (passing it to a typed
  parameter is a false positive there, as for `MacroToolkit.categoryID`), nor readers
  drop it (7 typed hovers).
- `UtilityHub.tempSelectedPreset`: two files the scan doesn't attribute to the class
  write it through the class's global, the only typed write a coarse generic result
  (`CopyTable(x)`); harvested, its bare `table` absorbs a preset in the other writer's
  local merge (14 hovers typed on main).
- `Glider.SpeedDisplay.Speed`: a member the class's only Lua file writes onto a field's
  value and never reads: the build puts the write on the call's object, while the field
  resolves through the scan's table, so the write shows the scan's `Cooldown`.
- Function-valued mixin fields (`AuctionatorDataProviderMixin.{onPreserveScroll,
  onSearchStarted}`): signatures, slice 4.
- Displays: a write of a constructor showing the constructor (`MacroToolkit.TextureNames`),
  `(string[])[]` against `string[][]` (`MacroToolkit.scripts`), a saved variable's table
  lifted as the global's (`MythicPlusPull.DB`).
- Deliberate: a subclass writing an inherited field reads its own writes
  (`BaseInput._escValue`); an XML `parentArray` stays `any` in the file appending to
  it (`TopButtons`); a declared `@field` read through a union with a local record
  (`ScrollTable.ColInfoBase.justifyH`).

## Slice 3 status (2026-11-07)

Constructor entries are writes of their file (ARCHITECTURE.md, "Constructor entries"):
a namespace's or global's constructor entries are slots, a nested constructor's under
its key; a `@class`'s own constructors' entries (a declaration's, a `setmetatable`
instance's) are class-field writes, the placeholder rule gone; a nested `---@class` on a
constructor-valued entry is that class's constructor; a class field files write
elements into under run-time keys holds them beside its record, which the files writing
the field read without them. `crossfile-types`: every constructor-entry path listed for
this slice agrees but `DBMMod.addon` (below); disagreeing paths (main → now) Auctionator
142 → 137, Baganator 83 → 82, Coolinator 30 → 29, DeadlyBossMods 436 → 425, NPCScan
81 → 79, Platynator 39 → 38, TestProject 88 → 87, UtilityHub 81 → 46, wow-ui-source
42 → 6, TradeSkillMaster 1412 on both, none rising. `check`: +6 true positives in
DeadlyBossMods (option ids a backward-inferred `string | number | nil` passed to
`number` parameters, `string | true` to `string?`, `number | string` to a `Number`
constraint, now that constructor entries carry the parameters they hold), and a
Syndicator message typed further (`bagID: ?` → `number`). `dump-types`: 11 hovers go
`nil` → `any` / `any?` on fields with other writers (a module's
`self.Options.SoundOption`, `DBTBar.callback` set to a handler), the accepted shift; no
other hover typed on main is untyped, 2185 gain a type. Analyses (main → now):
TradeSkillMaster 451 → 449 (692 files), DeadlyBossMods 268 → 233 (191), Auctionator
256 → 258 (343), Baganator 206 → 177 (110), TalentTreeTweaks 11 → 39 (36), NumyTest
49 → 75 (115), ResearchViewer 4 → 5 (3), every workspace within twice its files;
`check` times unchanged (TradeSkillMaster 2.0–2.3 s, DeadlyBossMods 2.3–2.9 s).

Component handling: a constructor entry holding a data file's table made
ResearchViewer's two code files, a cycle, read a unit outside it, so its first round
could no longer take their discovery analyses (7 analyses for 3 files). A cycle's first
round now takes them anyway, and the second analyzes those files again whatever changed
(`first_analysis_or_stale`): one round of analyses fewer wherever a cycle reads outside
itself (DeadlyBossMods 268 → 233), the same rounds of real analyses.

Open, by cause:
- `DBMMod.addon`, a deliberate cycle: `DBMMod`'s harvest reads `DBM.AddOns` through
  `DBM-Core.lua`, which declares both classes, so the entry's parameter stays the
  scan's in the files reading it.
- `tests/constructor-twin`'s `G_REPLACED.A` (fixture only): the writer shows the
  build's call-instance annotation `Other`, readers `Other | Widget`.
- A parameter whose callers newly disagree (a field the harvest now types passed
  beside other values) loses its backward-inferred type; hover shows what a `type()`
  guard proves inside the guard, the fixpoint reads it unknown there.

## Slice 4 status (2026-11-08, first round)

A workspace function's signature reads in every file as its defining file types it
(ARCHITECTURE.md, "Function signatures"): a `:` method's implicit `self` is the table
it is a member of (the build types it, never the stub blob's), or its `@param self`,
or, defined through the addon namespace, the class its file declares on it, else its
addon's own copy of the namespace; a
parameter no `@param` types is harvested (`deferred_params`, `DeferredSig::params`)
and installed through `symbol_overlay`, so other files' calls check arguments
against it as same-file calls do; a defining file reads its own functions'
external indices as its definitions (`overlay_own_functions`). A bare `function`
field (a function value the scan only saw stored, `self.onUpdate = function() end`)
is a slot like any other value. Displays now agree where both sides hold one type: a
plain global table standing for the class of its name names it, a returned instance
carrying fields written through it shows `Class & { … }` in its own file too,
`@return built: X` shows `X`, a shape as deep as a table collapses as one.
Backward inference, whose types now reach other files' callers, learned two idioms
same-file callers rarely exercised: a guard-promoted `table?` takes the literals
the parameter is compared with (`p == "DBM"` widens it to `table | string?`), and
`if not p then p = d end` accepts nil as `p or d` does; a forward to a parameter
inferred optional is a baseline `T?` hint.

`crossfile-types`: the "self typed in some files" family is gone (TSM 1362 methods,
DeadlyBossMods 352, …, every workspace 0); "parameters typed in some files" 7
functions left (all below); function-valued paths 133 → 70. Disagreeing paths (main
→ now): TSM 1412 → 7, DeadlyBossMods 425 → 24, Auctionator 137 → 18, NumyTest 131 →
4, Baganator 82 → 2, NPCScan 79 → 1, TestProject 87 → 12, UtilityHub 46 → 13,
LibQTip 40 → 1, BlizzMove 30 → 0, Coolinator 29 → 0, Glider 29 → 1, ItemUpgradeTip
26 → 1, PranGearView 27 → 1, TalentTreeTweaks 21 → 0, Syndicator 17 → 5, none rising.
`check`: +7 TSM, +4 Auctionator, +3 DeadlyBossMods, all true positives of calls other
files make now checked as same-file ones are (a `---@alias X ScrollFrame|BaseExtension`
union passed to `Frame?` ×4, a wrapper forwarding to `@param texture Texture` given a
button, `strlower`'d / table-key params given `string|number|nil` / `any?`,
`SetItemLink(nil)` into a body reading the link as a string, `{sortingOrder}` with a
nil branch, `SetText` given numbers, a `number?` into `string.split`);
wow-ui-source −1 and messages naming mixins instead of `{... N fields}`.
`dump-types`: no hover typed on main is untyped (wow-ui-source's 15 `any & any` →
`?`), TSM +48, DeadlyBossMods +93, Auctionator +51, TestProject +60, wow-ui-source
+718 gain a type. Analyses (main → now): TSM 449 → 526 (692 files), DeadlyBossMods
233 → 239 (191), Auctionator 257 → 336 (343), Baganator 177 → 202 (110), Coolinator
40 → 52 (48), Syndicator 34 → 43 (34), wow-ui-source 191 → 201, every workspace
within twice its files; `check` times unchanged (TSM 1.5–1.6 s, DeadlyBossMods 1.6 s).
Harvesting every function (the receiver from the harvest too) cost TSM 1271 and
Baganator 226 > 220: hence the build-typed receiver.

The stub blob is regenerated (pinned to HEAD's sources): the engine's display
reaches `generated/InferredReturns.lua`, where mixin returns written
`{... 28 fields}` (no type) now name the mixin (`FlowLayoutMixin`, …), a few returns
are inferred or sharper (one carries a field its factory writes), one record list
reads a `productType` it typed `string` as unknown, and stub_gen drops all-`any`
returns (`any & any`) and writes the plain return types (`format_inferred_returns`,
not the displayed `Class & { … }`).

Open, by cause (70 function paths):
- Multiply-defined paths, documented: readers see the definition the build keeps (an
  annotated one, else the first; annotated others as its overloads, arity unchecked
  where they disagree), each defining file's write site its own definition, so these
  disagree by design. Flavor files: Auctionator 16 (Legacy/Modern, Classic/Mainline),
  RepByZone 6; test doubles: DeadlyBossMods `Sorting_Test.lua`'s `DBM` methods ×6,
  `DBMTest.CreateSharedModule` (CLI vs WoW environment); a types file beside the
  library: TestProject `LibGraph-2.0.*` ×5. A union of the definitions merged an
  alternative's `any` return into readers' types (hover losses), so it waits.
- Harvest context: the defining file's harvest analysis cuts a read its top-level
  analysis takes (a mixin instance in a namespace slot, its own method through its
  class): Auctionator `AH.GetCurrentPage`, DeadlyBossMods `NewMod`,
  `TableUtils.orderedTable`, `AceGUISharedMediaWidgets-1.0.GetBaseFrameWithWindow`
  ×2, MacroToolkit `FindScript`, NumyTest `GetAndCacheEntryInfo`, Syndicator
  `DumpClassicTooltip`. Resolving a file's own calls to its local definitions fixed
  two and lost 9 hovers (mid-fixpoint returns), so it waits.
- Generics stay coarse (the rule): a synthesized `T1` for a pass-through parameter in
  the defining file (DeadlyBossMods `GetCIDFromGUID`, Syndicator `GetByGuildFullName`,
  `GetGuild`).
- Receivers: methods through a field holding a nested `---@class` constructor aren't
  on the class at all (DeadlyBossMods `TimeWarper.*` ×7, readers `self: table`); a
  file-local table standing for a class by name (`PrivateAuraUnitWatcher.AddAnchor`);
  a template intersection against its frame (`PGVToggleEnchantButton`), a record shape
  against a table (UtilityHub `ShowNotification`).
- Displays: TSM `Mill/Prospect/Transform.Get` (readers' element types come from the
  lift's structural dedup of the entries, the defining file's from the map's coarser
  value type); LibQTip `CallbackRegistry.Fire` ×2 (`@field fun(..., ...: T)` shows its
  vararg twice in its declaring file); TSM `ExtraTooltip.AddLine/AddDoubleLine` (read
  through `GameTooltip|ExtraTooltip`, keyed by the workspace member, shown through the
  first); wow-ui-source `TutorialData.GetFactionData`.
- A writer's union of its own other writes: DeadlyBossMods `DBTBar.Cancel` (an
  instance overwrites the method), `IsEncounterInProgress` (`or` with a bare
  `function`), ItemUpgradeTip `Debug`, NumyConfig `IterateModules`/`NewModule`,
  Platynator `PostInit` ×2.
- Untyped readers: Baganator `IsDialogOpen`, DeadlyBossMods `DBM_GUI_OptionsFrame.*`
  ×2, `LibAdvancedIconSelector-MTK.CreateIconSelectorFrame`, DeadlyBossMods
  `TableUtils.removeEntry` (read through a generic prototype getter).

## Slice 4 status (2026-11-10, second round)

The harvest reads what the defining file reads in more places (ARCHITECTURE.md,
"Function signatures"):
- A function returning an empty table on one path and a class instance on another
  (DeadlyBossMods `DBM:NewMod`) crosses as `table | Class`: the empty table lifts as
  a table of no fields, which a table doesn't absorb, where the union lifted as
  `any`. Every DeadlyBossMods mod's `mod` local, `any` on main, is `table | DBMMod`.
  A plain table a metatable without `__index`/`__call` makes (`orderedTable`) lifts
  as `table`.
- A harvest analysis's call of a function its own file defines, through the
  function's external index (`self:Base()` in another method of a library class),
  takes the file's definition (`Ir::own_definition`), as do a tail call's arity and
  the holders a returned frame carries its fields through; the cut gave `any`
  (`GetBaseFrameWithWindow`, DeadlyBossMods `Timer.TLStart`).
- A parameter a function returns as passed is a generic of it in its file (`T1`);
  the harvest keeps those (`DeferredSig::generics`) and the overlay declares them, so
  a call in any file binds them by its arguments (DeadlyBossMods `GetCIDFromGUID`,
  Syndicator `GetGuild`, `GetByGuildFullName`, `RecoverBattlePetLink`).
- Receivers: methods defined through a field a nested `---@class` declares
  (`---@class TimeWarper` on `test.TimeWarper = { … }`) are the class's, `self`
  included, when the build merges them into it; a method of a call's object
  (`ns.Button = CreateFrame(…, "Template")`, then `function ns.Button:M()`) has the
  receiver the call gives in its file, so it is harvested as a parameter rather than
  typed as the call's object table (PranGearView).
- A function defined inside another (`function ns.X.f()` in an initializer) is a
  namespace slot: the scan discovers it, the definition is a write of the field, its
  file's harvest types it (Baganator `IsDialogOpen`).
- Displays: a function value written into another table or compared (`Other.f =
  ns.f`, `f == ns.Remove`), which the fixpoint leaves unresolved, shows its harvested
  signature (only those: warming every unresolved field read harvested files nothing
  else reads, Baganator +12 analyses); `fun(..., ...: T)` shows its vararg once.

`crossfile-types`: function paths 70 → 53, 19 of the listed ones agreeing and 2 new
(below); "parameters typed in some files" 7 → 5, all multiply-defined. Disagreeing
paths (main → now): DeadlyBossMods 24 → 15, Syndicator 5 → 1, TestProject 12 → 11,
UtilityHub 13 → 12, Baganator 2 → 1, LibQTip 1 → 0, NPCScan 1 → 0, PranGearView
1 → 0, none rising. `check`: DeadlyBossMods −12 +10. Typing every mod's `mod`
reached calls checked against parameters typed by DBM's aliases whose `---|N:
description` members name no type (`undefined-doc-name` on those lines, unchanged):
an undefined type marked optional read as `nil`, which every argument mismatched.
`X?` with `X` undefined is now `any` (`annotations::union_of_resolved`, all three
resolvers; the parameter, return slot, field or alias keeps its place), which removes
main's 10 "expected `nil`" `type-mismatch` and keeps the 325 this round would have
added away; `undefined-field 'groupOptions'` ×2 is gone too. The additions are 6
`inject-field` hints (handlers assigned onto `mod`) and 4 `missing-parameter` before
`@param optionVersion any`, which is required. No other workspace changes.
`dump-types`: no hover typed on main is untyped but values an undefined `X?` typed
`nil`, now `any` (DeadlyBossMods' `color`/`subType`/`colorType`/`iconType`/
`defaultSound`/`checkType` parameters, 21 hovers and 51 in function hovers;
`@type BlizzMoveAPI?` locals in NumyTest, TestProject and ResearchViewer, 4 each;
`C_Engraving.GetRuneFor*Slot`'s `EngravingData?` results in Baganator, 2; a `T?` an
`@as` can't bind in TSM, 2), and 8 DeadlyBossMods reads of such parameters after
`p = p or 0` that read `number` and now `any`;
DeadlyBossMods +7164,
UtilityHub +72, TestProject +36 gain a type (TSM's changed hovers are `fun(...: any,
...)` → `fun(...: any)`). Analyses (main → now): Baganator 202 → 212 (110 files: the
in-function definitions add a cycle round), Coolinator 52 → 54, Auctionator 339 →
338, TSM 526, DeadlyBossMods 239, the others unchanged; `check` times unchanged (TSM
1.3 s, DeadlyBossMods 1.3–1.4 s). The stub blob is regenerated (pinned as in the
first round; the base regen reproduced main's blobs byte-for-byte): `DressUpLink`
reads `boolean?` and three FrameXML functions gain inferred returns (own calls in
harvest analyses), `ColorMixin` loses an `any?` `font` field (`InferredFields`
churn).

Open, by cause (53 function paths):
- Multiply-defined, by design (first round): Auctionator 14, RepByZone 5,
  DeadlyBossMods `Sorting_Test.lua` 5, TestProject `types_LibGraph.lua` 5; the
  parameter family's `GetGroupId`, `CreateSharedModule`, `GetDisenchantAuctionPrice`,
  `GetEmptyResult` are among them.
- Instance methods, by design (new): DeadlyBossMods `DBMMod.PlayerFighting`,
  `OnMatchStart` are methods one mod defines on itself (`function mod:M()`), which
  another mod reaches through `GetModByName` as a `DBMMod`; giving the class every
  mod's methods would put them on all mods. Visible now that `mod` is typed.
  Re-examined 2026-11-18 and kept: their readers are as on `c5b95b27` (`any`,
  `undefined-field`); only the write became visible. The writing file's other mods
  see them because `function f:m()` through a class's local lands on the file's view
  of the class (ARCHITECTURE.md, "Writes through a frame local in its own file").
- Harvest context, needing component work the budget doesn't leave room for
  (Baganator 212 of 220): Auctionator `AH.GetCurrentPage` (a cycle discovery
  doesn't see: `Wrappers.lua` reaches `Scan.lua`'s method only through a slot's
  resolved type, so its harvest reads `Scan.lua` in a context cutting the read
  back); MacroToolkit `FindScript` (a file of a class with several Lua files reading
  a field another of them writes: the class's files include the reader, so the read
  is its own and cut; a cycle with the class's rounds costs a round over every file
  of such classes).
- A correction the gate counts as losses: NumyTest `GetAndCacheEntryInfo`. The build
  drops a `@return` whose type doesn't resolve (`entryInfo`) where the per-file engine
  keeps an `any` slot, so readers read `isNewValue: boolean` in slot 0. Keeping the
  slot corrects 7 NumyTest hovers typed `boolean` by the wrong slot and turns stub
  returns typed by shifted slots into `any` (`select`'s `returns<F, index>`, TSM's
  `ExtraTooltip` reads through it, `GetTalentInfoByID`).
- Discovery: DeadlyBossMods `DBM_GUI_OptionsFrame.{SetCollapsed,
  ScheduleAutoUncollapse}` are defined through `local frame = _G["…"]`, a frame
  another file creates by name (function definitions aren't routed through global
  aliases, and a created global has no table its methods land on);
  `LibAdvancedIconSelector-MTK.CreateIconSelectorFrame` belongs to a library named by
  a local constant (`NewLibrary(MAJOR_VERSION)`) the scan doesn't resolve, beside a
  `NewLocale` of the same name, which declares the class readers read; RepByZone
  `GetEquippedTabard`: the options files read the class through `---@type
  RepByZone` on `GetAddon`, a table without the per-expansion files' methods.
- A writer's union of its own writes (first round): DeadlyBossMods `DBTBar.Cancel`,
  `IsEncounterInProgress`, ItemUpgradeTip `Debug`, Platynator `PostInit` ×2,
  NumyConfig `IterateModules`/`NewModule`. Readers read the function its definition
  gives, whose signature crosses on its own; for them to read the union the field
  can't be a function the build makes (its definition's navigation and signature).
- Displays: TSM `Mill/Prospect/Transform.Get`, `ExtraTooltip.AddLine/AddDoubleLine`,
  wow-ui-source `TutorialData.GetFactionData` (first round); UtilityHub
  `ShowNotification` (one file reads the method through the class field's harvested
  record, whose receiver lifts as a record); wow-ui-source
  `PrivateAuraUnitWatcher.AddAnchor`: a file-level local named like a class the stub
  blob makes of it (stub_gen's utility-table scan), which its file links by name;
  not linking locals by name fixes the path but loses stub_gen's inferred returns for
  Blizzard's own file-local mixins (`FlowLayoutMixin` → `{... 28 fields}`).
- A parameter's backward-inferred bare `table` absorbs the list its later versions
  hold (Baganator `BagTransfers` `matches`, `any[]` before the first round): the
  parameter is inferred from the `table` of `FromBagsToBags`'s harvested `toMove`
  (slice 2's bare-`table` trap).

## Slice 5 status (2026-11-13, first round)

The workspace build no longer types the slots the harvest covers (ARCHITECTURE.md,
"Retired scan typing"):
- A field written a call is the scan's existence placeholder (`any`, no annotation, no
  `table` guess for a call the build can't follow); the call's type is its file's: the
  slot harvest, the class harvest, or the call-field harvest for a field neither
  reaches (a namespace field at file level, the namespace's own `@class`, a member
  written onto a call's object, nested tables under a class field). `call_return_type`,
  its string-argument and field-name class guesses, `resolve_nested_ctor_calls` and the
  global `X = Call()` / `X = ref` typing are gone from the workspace build; a reference
  to a table still is that table (structure, not a type), and an XML `KeyValue`
  `type="global"` still declares its field the global's type.
- The workspace build without the stub database is the same build over an empty base
  (`PreResolvedGlobals::build` is `build_on_stubs(&empty())`): the cold `BuildContext`
  only builds the stub blob, whose declarations keep its typing (no harvest exists
  there). Fixtures with `with_stubs: false` run one build path twice.
- The harvest took over what the scan types papered over: a class whose files read
  fields others of them write is a cycle of its own (rounds, its files reading its
  previous round, `ReadPart::Field`); a call object's members ride on the harvested
  type (`with_call_object`), in the writer's own union too; a slot's harvest goes
  before a call field's at top level (the call's type is one write of the slot); the
  gates accept a union one member of which says more than `any`; a file's own
  workspace function lifts as that function; lifted records carry definition
  locations; a constructor's `nil` entry stays a declaration when the writes replace
  the scan's table.

`crossfile-types` (this round): 85 → 76 disagreeing paths (DeadlyBossMods 15 → 13,
NumyTest 4 → 1, TestProject 11 → 8, Glider 1 → 0), none rising; the later rounds end at
79 (see the fourth round). Of the paths this slice was handed, `TalentViewerTWW.treeId`
(as `any`, see below), `IsEncounterInProgress`, `NumyConfig_Module.{IterateModules,
NewModule}` and `Glider.SpeedDisplay.Speed` agree. `check`: no
additions; removed are 7 `type-mismatch` from `treeId`'s `table` guess, a
`categoryID` `undefined-field` and a `table?` `type-mismatch` (false positives), two
secret warnings on `UnitName("player")`, which the engine knows isn't secret where the
scan read the declared `secret<string>`, an `assign-type-mismatch` through
`IterateModules`, and one `inject-field` hint (`TestReporter.cachedReport`: the class
looks contracted only through build annotations the scan no longer makes). `dump-types`:
104 hovers gain a type; 51 typed hovers go untyped: `treeId` 14 (`table` → `any`, the
retired guess), MacroToolkit 25 (members written through `db` on its AceDB object:
carrying them lets their untyped writes absorb the defaults' types, not carrying them
loses them), `IterateModules` iteration 6 (readers now take the writer's union, see
below), DeadlyBossMods locale write sites 5, a method definition through a call field
1. Analyses (main → now): TSM 526 → 536, DeadlyBossMods 239 → 227, Baganator 212 → 212
(110 files), NPCScan 74 → 153 (78), Coolinator 54 → 71, NumyTest 84 → 101, every
workspace within twice its files; `check` times unchanged (TSM 1.7 s, DeadlyBossMods
2.0 s).

Open, by cause:
- Not retired yet (done in later rounds): `class_var_guesses` / `apply_instance_guesses`
  (method ownership of a call-initialized local, still guessed from its string
  argument), `function_locals`' call kinds (the call is no longer typed from them, only
  discovered), the `MaybeCallable` and field-named-class guesses of the existence pass.
  The self-field and constructor scanners' literal/parameter types stay (exact, not
  guessed), as does `settle_instance_tables`, which isn't scan typing: it applies a
  field's `---@type` to the tables written through it (author annotations win).
- A writer's union with a definition (`DBTBar.Cancel`, `Debug`, `PostInit` ×2): the
  build makes the method, so readers read the definition (slice 4).
- `GetAndCacheEntryInfo` (an unresolved `@return` slot), `tempSelectedPreset`,
  `UtilityHub.Database`, `GameOptions.subcategories`, `MythicPlusPull.DB`,
  `Constants.AllBankIndexes`, `justifyH`, `AuctionatorSaleItemMixin.itemInfo`:
  unchanged from slices 2 and 4, none a scan type.
- `IterateModules` now agrees as the writer's union of the inherited Ace method and
  its own write, over which `for … in` reads nothing: the writer should read its write
  over an inherited copy.
- Write sites still showing the placeholder: a reference written at a field of an
  exported local's constructor (DeadlyBossMods `L.AUTO_TIMER_TEXTS.*`).

## Slice 5 status (2026-11-14, second round)

The first round's seven behavior changes were approved; the hovers it lost are typed again:
- MacroToolkit's members written through `db` (25): a `@class` field's harvest carries the
  call object's members after the call's type (`with_class_call_object`), and the record
  `MT.conditions` lifted from its constructor reads a run-time key as any entry or `nil`
  (`TableShape::closed`).
- The `IterateModules` iterations (6): a write replacing a field a class imported from a
  stub parent gives the class its own field (`Ir::is_inherited_copy`), so the writer and
  every reader read the written method.
- DeadlyBossMods' locale write sites (5): a deep write onto a field the build only says
  exists types it in its file (`Ir::is_scan_any`).
- PranGearView's `UpdateTooltip` definition (1): the definition's name lookup searches the
  receiver's intersection past its first table.
- Two variants the round's fixture (`value-fields-crossfile/round_toolkit*.lua`) found:
  an entry of another file's constructor typed only by its harvest now types a run-time-key
  read (`Addon.kinds[k]`), and the table's hover lists it typed.

Retired: the existence pass's `MaybeCallable` guess (`function & table`) and its
field-named-class / namespace-sub-table guesses (both `any` now), and the class a string
argument names for a call through a function body's call local (`BodyCalls::call_kind`
records the call with no callee). `settle_instance_tables` stays: it isn't scan typing but
`---@type` authority on the tables written through a declared field (`ns.state.running =
true` reads the declared `boolean`), which the plan keeps.

Blob: regenerated with wow-ui-source `live`/`forever` pinned at provenance and HEAD's
vendor/generated files copied in (the pinned base regen reproduced HEAD byte-for-byte). The
files blob is unchanged (no `Inferred*` movement); `precomputed.bin.zst` differs only in four
raw `stub_globals` entries (function-body call-local calls recorded as calls).

Gate against main: `check` adds nothing and removes 7 more `type-mismatch` (`got function &
table`); `crossfile-types` 85 → 79, rising nowhere; `dump-types` typed →
untyped: the approved `treeId` 14 plus the retired guesses, `function & table` → `any` 192
(DeadlyBossMods 83, PranGearView 75, TestProject 23, Glider 6, Syndicator 5) and a field
typed the class its name matches → `any` 15 (`defaultPosition.x: x`, `scale: scale`).
Analyses: as round 1 (TSM 531, DeadlyBossMods 227, Baganator 212, NPCScan 153), every
workspace within twice its files.

## Slice 5 status (2026-11-15, third round)

`class_var_guesses` is retired as a guess: the scan still reads `local w = F("Name")` as
class `Name` (the string given directly or through a local bound once to it), but the
build keeps that reading only when the callee's declaration names a class by the
argument (`@defclass`, or a backtick generic it returns), else moves what was written
through `w` to the fields it is assigned to (`apply_instance_guesses`). A string argument
no longer makes a class of its own: `NewLocale("LibAdvancedIconSelector-MTK", …)` made
the class MacroToolkit's icon library was typed as, holding locale keys; the library's
own table (`LibStub:NewLibrary(MAJOR_VERSION, …)`) is that class now, with its methods,
so `MT.AIS = MT.LS("LibAdvancedIconSelector-MTK")` reads it in every file (no unbound `T`
in its writer) and the `CreateIconSelectorFrame` `undefined-field` false positive is
gone. UtilityHub gains 113 typed hovers the same way.

Gate against main: as the second round (no `check` additions; `crossfile-types` 85 → 79,
rising nowhere; typed → untyped only the approved `treeId` and retired guesses), plus the
gains above. `MacroToolkit.commandinfo` disagrees where `CreateIconSelectorFrame` did:
other files now read it `table<string, number>` while its writer, as on main, reads its
constructor of class constants as `table<string, any>`.

## Slice 5 status (2026-11-15, fourth round)

Members written through an AceDB object at their write sites read typed again
(PranGearView's `db.profile.*.customColor` and `color{High,Medium,Low}` writes, 32
hovers `string` on main, `any` before this round). A member written through a call's
object lives on the object's members table, which the build left at its existence
`any`; the write landed instead on the AceDB defaults' copy of the member, because the
deferred deep write walked its receiver from the root's first value only. It now also
reaches every table the path holds from any value of the root, each member of a union or
intersection along it (`write_path_tables` in `resolve_deep_field_injections`), so the
members table holds the file's writes (`string`, a `val:gsub(...)` beside a typed write
included). A write site whose field nothing in its file types — the build's `any`, or
the members table of a call's object — shows what it writes, carrying those members
(`written_over_placeholder`, hover and `crossfile-types` alike).

`crossfile-types`: 85 → 79 disagreeing paths (DeadlyBossMods 15 → 13, Glider 1 → 0,
NumyTest 4 → 1, TestProject 11 → 11), rising nowhere. TestProject's count holds with one
path swapped: `LibAdvancedIconSelector-MTK.CreateIconSelectorFrame` agrees (third round),
and `MacroToolkit.commandinfo` disagrees because other files now read the harvested
`table<string, number>` while its writer, as on main, reads its constructor of class
constants as `table<string, any>`. `MacroToolkit.db.{profile.scale, profile.width,
global.ebackups}` disagree exactly as on main: members written through the AceDB object
outside its defaults, which a file writes a value (`scale = value`, `ebackups = {}`) rather
than a call; the file writing one reads its own write (`number`, `table`), other files the
members table's `any`, since the call-field harvest only takes calls, and the
`db.profile.width` / `db.global.ebackups` write sites (mainframe.lua:66, restoreframe.lua:248)
are ones the field-chain lookup doesn't reach, showing the build's `any`.

Gate against main: `check` adds nothing (19 false positives removed); typed → untyped is
169, all approved: `treeId` 14, `function & table` → `any` 140 (the second round's 192
less the members whose writes now type them) and a field named like a class → `any` 15;
265 hovers gain a type. Analyses and `check` times as the third round.

## Slice 5 status (2026-11-18, fifth round)

`MacroToolkit.commandinfo` agrees. A string-keyed constructor of references
(`{ ["SHOW"] = MT.X }`) is a map whose `[string]` element the scan types `any`; the
class harvest drops that table for its one writer's constructor (`table<string,
number>`), but the writer kept reading the scan's table: a constructor twin gives it a
record's entries, never a map's element type. The writer now reads its writes there
(`deferred::table_past_own_writes`, a class slot with no others), and the scan's map
where the harvest keeps it for readers: a write of differing entries (two handler
functions, a number beside a string) or no more than a table (a `{}` reset), which make
the harvest's field a bare `table` (`Ir::own_map_slots`, `Ir::own_map_kept`). Records are left to
their twins: taking the writer's constructor for those too turned `nil` entries the
scan declares (`relativeFrame = nil`, assigned later) into `nil` and lost entry types
(DeadlyBossMods' `testDefinition`).

Gate against main: `check` unchanged; `crossfile-types` TestProject 11 → 10, nowhere
else changed; `dump-types` no typed → untyped, 186 MacroToolkit hovers of the class
show `commandinfo: table<string, number>`.

Open, same scan entry, not disagreeing in the corpus: the map at a slice-1 slot (an
addon namespace's or a plain global's field), whose entries are slots but whose element
type stays the scan's `any`. The namespace's reads `table<string, any>` in every file; a
plain global's writer reads its constructor (`table<string, number>`), its readers
`table<string, any>`.

Open, of this round's rule:
- A map two files of the class write, read by a value derived from it in one of them
  (`C.copy = C.info`): a harvest analysis can't tell that another file writes the map,
  so the derived value crosses as that file's writes alone, narrower than the writer
  reads it. Reading the scan's map in harvest analyses instead (the documented rule)
  would make the far commoner derived read of a map one file writes (a function
  returning `C.info[k]`) cross as `any` while its file reads the element type. A proper
  fix needs discovery of which files write a class field.
- A map its file also fills under run-time keys (`C.entries[k] = { … }`) reads, in that
  file as in others, only the constructor's elements: the harvest adds element writes to
  records, not maps (`with_dynamic_elements`).

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
