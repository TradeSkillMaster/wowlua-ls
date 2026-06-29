# wowlua_ls — WoW Lua Language Server

A Language Server Protocol implementation for Lua (World of Warcraft API dialect). Provides hover, go-to-definition, completion, signature help, find references, rename, and diagnostics.

For deep architecture internals, see [ARCHITECTURE.md](.claude/ARCHITECTURE.md) — type inference, narrowing, generics, builder pattern, cross-file references, metatable inference, flavor filtering, plus the relocated source-file deep notes, config internals, inlay hints, the per-feature annotation/type-system reference, and the diagnostics module reference. Pointers below lead to the relevant section.

For the full test-file catalog and the embedded-assertion (`hover:`/`def:`/`diag:`/…) format, see [TESTING.md](.claude/TESTING.md).

For Neovim diagnostic integration details (push/pull namespaces, `workspace_diagnostics` flag, line-shifting, edit zone handling), see [NEOVIM_DIAGNOSTICS.md](.claude/NEOVIM_DIAGNOSTICS.md).

## Architecture

### Workspace crates
The project is a cargo workspace of layered library crates plus a thin binary. The layering is **one-directional and enforced at compile time** — a crate can only use the crates below it:

- **`wowlua_syntax`** (`crates/wowlua_syntax/`) — leaf crate: lexer, parser, CST (`syntax/`), typed AST (`ast.rs`). Depends on nothing else.
- **`wowlua_core`** (`crates/wowlua_core/`) — the shared type vocabulary: IR types (`types.rs`), flavor bitmask (`flavor.rs`), and the annotation type *definitions* embedded in the IR (`annotations.rs`: `AnnotationType`, `TuplePosition`, `ParamInfo`, `Visibility`, `KEYOF_SELF_TARGET`). Re-exports `syntax`/`ast`.
- **`wowlua_analysis`** (`crates/wowlua_analysis/`) — the per-file analysis engine and everything in its dependency cycle: `analysis/`, `annotations/` (parsing/scanning; re-exports the core type defs), `pre_globals/`, `diagnostics/`, `config.rs`, `xml_scan.rs`. Owns `MAX_COMPLETIONS`. A `test-util` feature exposes `#[cfg(test)]` construction helpers (`ClassDecl`/`ExternalGlobal::for_test`, `PreResolvedGlobals::push_ext_*`) to higher crates' test builds.
- **`wowlua_lsp`** (`crates/wowlua_lsp/`) — `lsp/` (server loop + handlers), `plugins/` (Lua plugin engine), `toc/` (.toc parsing), `has_shebang`. Owns the `embedded-stubs` feature (`lsp/main_loop/stub_loading.rs`).
- **`wowlua_stub_gen`** (`crates/wowlua_stub_gen/`) — the offline stub-generation tool (`stub_gen/`); above `wowlua_lsp` because it drives a workspace scan. Forwards `embedded-stubs` to `wowlua_lsp`.
- **`wowlua_doc`** (`crates/wowlua_doc/`) — Markdown doc generation (`doc_gen.rs`, `doc_gen_md.rs`); depends only on `wowlua_analysis`, parallel to `wowlua_lsp`.
- **`wowlua_ls`** (root `src/`) — thin binary: `main.rs` + `cli/`, plus a facade `lib.rs` that re-exports every lower crate's modules so `wowlua_ls::<module>` (tests, CLI) and intra-crate `crate::<module>` paths resolve unchanged.

**Conventions for working across the split:**
- Each crate's `lib.rs` re-exports the modules of the crates below it (`pub use wowlua_core::{...}`), so the original `crate::syntax::…`/`crate::types::…`/etc. paths inside moved code resolve without edits. When a type/const must move *down* a layer, re-export it from its original module path so existing `crate::<old_path>::Name` references keep working (e.g. `annotations/mod.rs` re-exports the core annotation type defs).
- Items that cross a crate boundary must be `pub`, not `pub(crate)` — within each split-out crate, `pub(crate)` was promoted to `pub`. **Module-level privacy is preserved**: the `Ir`/`PreResolvedGlobals` arena fields and the `lsp` `main_loop` struct fields are plain `private` (never `pub(crate)`), so the promotion didn't touch them and they stay encapsulated behind their routing surfaces.
- The detailed module descriptions below use pre-split `src/<module>` paths; those files now live under `crates/<crate>/src/<module>` per the mapping above (only `main.rs`, `cli/`, and the facade `lib.rs` remain in the root `src/`).
- `stubs/` stays at the workspace root. Code that locates it via `env!("CARGO_MANIFEST_DIR")` (the `include_bytes!` in `stub_loading.rs`, the regenerate-stubs output path) uses `../../stubs` to climb from `crates/<crate>/` to the root.

### Source files
A concise map of the source tree. Several files carry deep mechanism notes too large for an always-loaded map; those are relocated to [ARCHITECTURE.md — Source-file deep notes](.claude/ARCHITECTURE.md#source-file-deep-notes) with a pointer left on the relevant bullet.

- `src/main.rs` — CLI entry point: `evaluate` subcommand, `test-query` subcommand (hover/def/sig/completions/diagnostics), `dump-types` subcommand (hover regression baselines), `doc` subcommand (markdown API doc generation), otherwise starts LSP
- `src/doc_gen.rs` — Documentation data model: `DocNamespace`, `DocDefine`, `DocField`, `DocParam` structs, standalone type formatter operating on `PreResolvedGlobals`, class/field iteration with visibility and source locations
- `src/doc_gen_md.rs` — Markdown documentation renderer: takes `Vec<DocNamespace>` and produces VitePress-compatible `.md` files (one per class + `index.md`)
- `src/types.rs` — IR type definitions: `ValueType`, `Expr`, `Symbol`, `Scope`, `Function`, `TableInfo`, `FieldInfo`, `EnumKind`, deferred check structs, index aliases, `EXT_BASE`
- `src/analysis/` — Core per-file analysis engine (`Analysis` struct):
  - `mod.rs` — `Ir` struct definition, scope-chain walking helpers, two-tier lookups, core helpers
  - `prescan.rs` — Phase 0: class/alias pre-scan, annotation type resolution, generic inference
  - `build_ir.rs` — Phase 1: AST walk, scope/symbol/function/table creation, correlated return inference
  - `lower_expression.rs` — Expression lowering from AST to IR `Expr`: literals, identifiers (`NameRef`, `DotAccess`, `BracketAccess`, `MethodCall`), function calls, binary ops, table constructors, inline `@as` casts
  - `narrowing.rs` — Type narrowing from control flow guards: `GuardNarrow` enum, `OrTermEffect`, flavor narrowing detection, `@flavor-narrows`, type filter/strip for scope-specific refinement
  - `resolve.rs` — Phase 2: fixpoint type resolution loop, expression resolver, backward param-type inference
  - `resolve_call.rs` — Function call resolution: `CallSiteInfo`, argument count/type checking, return type determination, overload matching, generic binding
  - `checks.rs` — Diagnostic check orchestration via `run_diagnostics()`, name-token collection for access diagnostics
  - `queries/` — LSP query methods, split per feature (all `impl AnalysisResult` blocks over the `pub(crate)` fields defined in `mod.rs`). `mod.rs` holds shared imports/re-exports (`ReferenceTarget`, `HighlightKind`, `CallSiteResult`, `OutgoingCallResult`, `DATA_REPLACE_START/END`) and the cross-module helpers (`return_type_at_slot`, `format_vararg_return`, `format_vararg_param`). Submodules: `nav.rs` (token/symbol/field resolution helpers like `find_symbol_at`, `find_field_at`, `scope_at_offset`), `format.rs` (type/signature formatting), `hover.rs`, `definition.rs`, `completion.rs`, `signature.rs`, `references.rs`, `rename.rs`, `highlights.rs` (document highlights), `inlay_hints.rs`, `code_lens.rs`, `call_hierarchy.rs`, `document_symbols.rs`, `embedded_strings.rs` (event/`expression<C,R>` string hover/completion/definition)
  - `semantic_tokens.rs` — LSP semantic-token classification (emits `function` on bare name tokens that resolve to functions; classifies the name chain of a function/method *definition* header as `class`/`property`/`method`/`function`; `defaultLibrary`/`deprecated` modifiers). Narrow by design — the analysis owns only the dotted-chain cases a type-blind TextMate grammar can't get right; everything else is left to the editor grammar. Encoded by `main_loop/semantic_token_encoding.rs::encode_semantic_tokens`. See [ARCHITECTURE.md — Semantic-token classification](.claude/ARCHITECTURE.md#semantic-token-classification-analysissemantic_tokensrs).
- `src/pre_globals/` — Precomputed global type database:
  - `mod.rs` — `PreResolvedGlobals` struct, 5-phase build from WoW API stubs, type parameter substitution, class/alias/function registration. Multi-definition go-to-definition is served from runtime-only `*_by_name`/`*_all` maps — see [ARCHITECTURE.md — Multi-definition go-to-definition](.claude/ARCHITECTURE.md#multi-definition-go-to-definition-pre_globalsmodrs).
  - `build_on_stubs.rs` — `BuildOnStubsContext` for workspace incremental builds on precomputed stubs: scope/symbol/function/table cloning, type parameter substitution, field resolution. **Built-in stub name reuse is ADDITIVE** (a workspace `@class` colliding with a stub class name merges onto the stub `TableInfo`; never replaces it) — see [ARCHITECTURE.md — Built-in stub name reuse is additive](.claude/ARCHITECTURE.md#built-in-stub-name-reuse-is-additive-pre_globalsbuild_on_stubsrs).
- `src/annotations/` — Annotation system (types, parsing, cross-file scanning):
  - `mod.rs` — Core types (`AnnotationType`, `ParamInfo`, `Visibility`, `ClassDecl`, `AliasDecl`, `AnnotationBlock`), comment extraction (`extract_annotations`), full-file class/alias discovery (`scan_all_annotations`), line-level `@tag` dispatch (`parse_annotation_lines`), tuple-union lowering (`lower_tuple_form_cases`), re-exports from submodules
  - `annotation_types.rs` — Type expression parsing: `parse_type()`, `parse_overload()`, `parse_return_line()`, `format_annotation_type()`, `substitute_alias_type_params()`, `match_projection()`, and internal helpers (`split_at_top_level`, `extract_type_prefix`, etc.)
  - `annotation_scanning.rs` — Shared types (`ExternalGlobal`/`ExternalGlobalKind`/`FieldValueKind`), constants (`ADDON_NS_NAME`), shared helpers (`extract_type_annotation_for_assign`, `extract_inline_type_annotation`, `is_select_varargs`), `scan_method_typed_self_fields()`, `scan_diagnostic_directives()`, type conversion (`resolve_annotation_type`, `reduce_to_fun_alias`). The 3-tier self-field scanners (all gated on recognized local `@class` receivers) — see [ARCHITECTURE.md — Self-field scanners](.claude/ARCHITECTURE.md#self-field-scanners-annotationsannotation_scanningrs).
  - `scan_globals.rs` — `scan_file_globals[_with_synth]()`, `scan_dynamic_global_prefixes()` (`_G["PREFIX"..k]` auto-allowed globals), and `scan_created_globals()` (`@creates-global` calls). The coarse global scan is detection-only; a final `root.descendants()` pass registers the *existence* of in-function/field/multi-target writes the main loop skips — see [ARCHITECTURE.md — Coarse global scan & descendants pass](.claude/ARCHITECTURE.md#coarse-global-scan--descendants-pass-annotationsscan_globalsrs).
  - `scan_defclass.rs` — `scan_defclass_calls()` with constructor self-field extraction, defclass chain walking, and type inference helpers
  - `scan_built_name.rs` — `scan_built_name_calls()` with builder-chain field extraction, generic substitution, and `@builds-field` resolution
  - `scan_callback.rs` — `scan_callback_registries()` collects callback registries (`@generates-events`) + `path = { ... }` string-array constants; powers event-name completion + `unknown-callback-event`. See [ARCHITECTURE.md — Callback registries](.claude/ARCHITECTURE.md#callback-registries-annotationsscan_callbackrs).
- `src/flavor.rs` — 3-flavor bitmask (retail/classic/classic_era matching WoW's install-folder names), `from_ketho_mask()` that collapses Ketho's 4-bit (mainline/mists/bcc/classic_era) into ours (mists and bcc both map to classic), name parsing, and narrowing helpers for `wrong-flavor-api`. Also owns flavor-aware `deprecated` (`deprecation_suppressed`) and `interface_number_flavor`/`parse_interface_flavors` — see [ARCHITECTURE.md — Flavor-aware `deprecated`](.claude/ARCHITECTURE.md#flavor-aware-deprecated-flavorrs) and [Flavor filtering](.claude/ARCHITECTURE.md#flavor-filtering-flavors-config--toc-detection--flavor-narrows--wrong-flavor-api).
- `src/diagnostics/` — Trait-based diagnostic architecture with centralized catalog (see [Diagnostics](#diagnostics) below)
- `src/syntax/parser.rs` — Recursive descent + Pratt parser producing arena-based `SyntaxTree`
- `src/syntax/tree.rs` — Arena-based syntax tree: `SyntaxTree`, `Node`, `Token`, `NodeId`, `TokenId`, `TreeBuilder` with checkpoint support; also high-level API wrappers (`SyntaxNode`, `SyntaxToken`, `TextRange`, `TextSize`, `TokenAtOffset`, `NodeOrToken`)
- `src/syntax/syntax_kind.rs` — `SyntaxKind` enum (unified token + node kinds)
- `src/syntax/lexer.rs` — Tokenization
- `src/ast.rs` — AST node definitions and casts over `SyntaxNode` (uses `define_ast_node!` macro)
- `src/config.rs` — Project configuration: `.wowluarc.json` loading, `.toc` `SavedVariables` parsing, ignore patterns, `library` patterns (scanned but diagnostics suppressed, supports absolute paths — and relative paths that escape the workspace, e.g. `../shared`, which `load_if_exists` resolves against the config dir into `library_absolute` — for external directories), diagnostic overrides, allowed globals, `inference.backward_param_types`, `inference.correlated_return_overloads`, `hint.*` inlay hint config, `addon_root` for multi-addon namespace isolation (`addon_root_for()` / `addon_roots()`). Key policies — [`flavors_for` vs `addon_flavors_for`](.claude/ARCHITECTURE.md#flavors_for-vs-addon_flavors_for) and the [nested-config isolate-vs-inherit policy](.claude/ARCHITECTURE.md#nested-config-policy-isolate-vs-inherit) (which settings are isolated to the nearest `.wowluarc.json`, the `library` inherited-downward exception, and the built-in `.github/` default-ignore) — live in ARCHITECTURE.md.
- `src/stub_gen/` — Stub generation: fetches WoW API stubs, Classic globals from wiki/BlizzardInterfaceResources, GlobalStrings and GlobalColor from wago.tools DB2, and serializes precomputed `PreResolvedGlobals` blob (replaces former Python scripts). Split by pipeline stage:
  - `mod.rs` — shared data model (the `Blizzard*`/`ApiDocData`/`UtilTableInfo`/`*Regexes` structs, type aliases, and constants), module declarations, and the `pub use orchestrate::regenerate_stubs` entry-point re-export. Submodule helper fns are re-exported (`pub(crate) use <mod>::*`) so each stage sees the others via `use super::*`.
  - `orchestrate.rs` — `regenerate_stubs()`, the top-level pipeline that clones sources, runs every discovery pass, and serializes the blob
  - `sources.rs` — remote/source I/O: HTTP fetches, git shallow-clone, wiki/resource/DB2 fetching, on-disk wiki cache
  - `blizzard.rs` — Blizzard `APIDocumentationGenerated` parsing + function/struct/enum/event/script-object stub generation
  - `wiki.rs` — warcraft.wiki.gg wikitext parsing, widget-method enrichment, and wiki-derived stub generation, plus type-name normalization/inference. **Multi-form `{{apisig}}` → `@overload`**: a wiki page may list several signatures in one apisig (separated by `{{=}}` or whitespace). `parse_wikitext` emits the primary form as the function and every *additional same-name* form as an `@overload` (e.g. the legacy spell-book family `GetSpellInfo(spell)` / `GetSpellInfo(index, bookType)`, `IsSpellInRange(spellName, unit)` / `(index, bookType, unit)`). Forms with a **different** name (the `X(itemLocation)` / `XByID(itemInfo)` shared-page idiom) are separate functions, get their own stubs, and are skipped — the name-equality gate (`parse_apisig_call_args`, arity-deduped) is what keeps those from being mis-folded into bogus overloads. Widget-method *enrichment* (`parse_widget_wiki_annotations`) deliberately only injects parameter/return **types** into Ketho's existing signatures — it never expands the param list, so a method whose only fuller source is the wiki `{{widgetmethod}}` page (e.g. `GameTooltip:SetInboxItem`'s optional `attachIndex`) is not picked up.
  - `globals_csv.rs` — wago.tools DB2 CSV parsing for GlobalStrings/GlobalColors and the resulting global stubs
  - `framexml.rs` — FrameXML Lua scanning: inferred returns, runtime fields, utility tables/mixins
  - `xml_frames.rs` — XML frame/mixin extraction and inheritance resolution
  - `classic.rs` — Classic-flavor stubs, flavor-map computation, API-doc dir scanning, and constant inference
  - `util.rs` — filesystem/name-scan helpers, scan-path collection, validation (`validate_stub_counts`), and misc utilities
  - `tests.rs` — `#[cfg(test)]` unit tests
- `src/xml_scan.rs` — XML frame/template scanning: parses `.xml` files for `<Frame>`, `<Button>`, `<Texture>`, etc. elements, extracting `ClassDecl` (virtual templates) and `ExternalGlobal` (non-virtual named frames) entries. Handles `parentKey`/`parentArray` child fields, `KeyValue` typed fields, `inherits`/`mixin`/`secureMixin` parent chains, `$parent` name resolution, `intrinsic="true"` custom element types, and implicit parentKey on special elements (NormalTexture, HighlightTexture, etc.)
- `src/lsp/main_loop/` — LSP server loop and request handling, split per concern. `mod.rs` holds the struct/enum definitions (`Document`, `WorkspaceState`, `PendingEditMap`, `RebuildScope`, `ClientSupport`, etc. — kept here so descendant submodules can access their private fields), the server core (`start_ls`, `main_loop`, `send_response`, `with_doc_at_position`, `cast_req`/`cast_not`, `use_utf8`, `scan_stubs_for_test()`), and the `#[cfg(test)]` module. Submodules: `state.rs` (`WorkspaceState`/`PendingEditMap` methods, self-field/defclass merging), `scan.rs` (workspace/file/XML scanning, warm-build), `rebuild.rs` (incremental `maybe_rebuild_workspace` + the `*_match`/`*_changed_names` comparators), `stub_loading.rs` (precomputed-stub loading, `load_precomputed_stubs`/`stub_file_contents`/`stubs_dir`), `semantic_token_encoding.rs` (`encode_semantic_tokens`), `conversions.rs` (URI/position/range/document-symbol conversions), `handlers.rs` (request/notification dispatch, parse/analyze), `diagnostics_handlers.rs` (diagnostic building/shifting/publishing), `hierarchy.rs` (call/type hierarchy, workspace symbol search, cross-workspace references/implementations — incl. the per-`ws_generation` `xfile_analysis_cache`; see [ARCHITECTURE.md — Cross-file analysis cache](.claude/ARCHITECTURE.md#cross-file-analysis-cache-lspmain_loophierarchyrs)), `code_actions.rs` (quick fixes, code actions, annotation-stub generation), `refactor.rs` (extract variable/function)
- `src/lsp/diagnostics.rs` — Diagnostic publishing with `@diagnostic` suppression and project-wide config overrides
- `src/lsp/uri.rs` — URI/path conversion utilities (percent-encoding, Windows drive letters, spaces)

### Two-tier index space (EXT_BASE)
External globals (WoW API stubs) use indices >= `EXT_BASE` (1,000,000). Per-file locals use indices < `EXT_BASE`. All lookup functions (`sym()`, `func()`, `table()`, `expr()`) route via `idx >= EXT_BASE` check. This avoids cloning ~9000 external symbols per file.

### IR read-only query surface (post-build consumers)
The `Ir` arenas (`symbols`, `functions`, `tables`, `exprs`, `scopes`) carry load-bearing, convention-only invariants — the EXT_BASE two-tier routing, the symbol/function overlay fallbacks, and scope-chain walking. **Post-build consumers (everything in `src/diagnostics/`) must read the IR through the narrow query surface on `Ir` (and the delegators re-exposed on `AnalysisResult`/`Analysis`), never by indexing the raw arena `Vec`s directly.** The surface is defined in `analysis/mod.rs` next to the existing routing helpers:

- **Indexed routing accessors** (handle EXT_BASE + overlays): `sym(idx)`, `func(idx)`, `expr(idx)`, `try_expr(idx)` (fallible), `table(idx)`, `scope(idx)`, `try_scope(idx)` (fallible).
- **Local-arena iterators** (per-file entries only; external stub entries live in `self.ext` and are deliberately excluded): `local_symbols()`, `local_functions()`, `local_tables()`, `local_exprs()`, `local_scopes()` — each yields `(TypedIndex, &T)` so callers get the correctly-typed local index for free instead of reconstructing `FunctionIndex(usize)` etc.
- **Scope-0 (global/file-level) helpers**: `scope0_local_symbols()` (iterate the file's own scope-0 bindings as `(&SymbolIdentifier, SymbolIndex)`; external `scope0_symbols` are chained explicitly by the `_G`-redirect completion path), `scope0_global_symbol(id)` (local scope-0 lookup with the external `scope0_symbols` fallback, no framexml — encapsulates the manual `scopes[0].symbols.get(..).or(ext.scope0_symbols.get(..))` pattern).
- **Scope-chain walk**: `get_symbol(...)`, `scope_at_offset(...)`, `ancestor_scopes(start)`.
- **Resolved-type cache**: `resolved_expr_cache_get(ExprId) -> Option<&ValueType>` on `AnalysisResult` — avoids raw `resolved_expr_cache[id.val()]` indexing in diagnostics.

The shared `unwrap_to_inner_expr(ir: &Ir, id)` helper in `diagnostics/mod.rs` also takes `&Ir` (not a raw `&[Expr]` slice) so its expr lookups route through `expr()`. Rule of thumb when writing a new diagnostic: iterate user code via `analysis.local_*()`, resolve any index via `analysis.sym/func/expr/table/scope(idx)`, and never write `analysis.ir.symbols[...]` / `.functions[...]` / `.exprs[...]` / `.tables[...]` / `.scopes[...]`. (Method calls like `analysis.ir.expr(idx)`, `.ir.get_field(...)`, `.ir.classes`, and the deferred-check collection fields such as `.ir.call_resolutions` / `.ir.binary_op_sites` are fine — they don't bypass routing.)

**The boundary is enforced at the type level**: the five arena fields on `Ir` are private to the `analysis` module, and `PreResolvedGlobals`'s five arena fields are private to `pre_globals`, with `pub` routing accessors as the only cross-module entry. Keep these fields private; do not widen them — add a new accessor on the relevant surface instead. Full enforcement detail (which modules are forced through the surface, the `PreResolvedGlobals` symmetric treatment, the `doc_gen` `push_ext_*` test-util helpers): [ARCHITECTURE.md — IR read-only query surface — enforcement detail](.claude/ARCHITECTURE.md#ir-read-only-query-surface--enforcement-detail).

### Key query functions (in `queries/nav.rs`)
- `find_symbol_at(offset)` — Resolves direct names: gets token at offset → scope lookup → returns `(SymbolIndex, name)`
- `find_field_at(offset)` — Resolves dot/colon chains (`x.y.z`): walks table fields to find the target field's `ExprId`
- `scope_at_offset(offset)` — Finds innermost scope containing offset via `block_scopes` ranges
- `get_symbol(id, scope_idx)` — Walks scope hierarchy upward; at scope 0 also checks `ext.scope0_symbols` (in `analysis/mod.rs`)

### Inlay hints (in `queries/inlay_hints.rs`)
`inlay_hints(tree, config)` collects six categories of inline annotations controlled by `InlayHintConfig` (`.wowluarc.json` `hint.*`, enabled by default unless noted): parameter names, variable types, function return types, for-loop variable types, parameter types (default-off), and chained method return types (default-off). All type hints use `format_type_depth(_, 1)` (class names only). The LSP handler in `main_loop/handlers.rs` converts byte offsets to LSP positions. Per-category suppression rules and the full algorithm: [ARCHITECTURE.md — Inlay hints](.claude/ARCHITECTURE.md#inlay-hints-analysisqueriesinlay_hintsrs).

### Code lens (in `queries/code_lens.rs` + `main_loop/handlers.rs`)
Three lens kinds: "N usages" (two-stage resolve via `code_lens_targets`), "N implementations" on `@class` declarations, and "overrides Parent" on methods (both pre-resolved via `code_lens()`). See [ARCHITECTURE.md](.claude/ARCHITECTURE.md#code-lens-in-queriesrs--main_looprs) for algorithm details.

### Per-file analysis phases (in `src/analysis/`)
1. **Phase 0: prescan_classes_and_aliases** — Import external classes/aliases, scan local `@class`/`@alias` declarations
2. **Phase 1: build_ir** — Walk AST, create scopes/symbols/functions/tables, lower expressions to `Expr` IR
3. **Phase 2: resolve_types** — Fixpoint loop resolving expressions until no progress

### Diagnostics
Diagnostics use a trait-based architecture with a centralized catalog in `src/diagnostics/mod.rs`:
- `DiagnosticDef` struct (`code: &str`, `severity`) with `emit()` method for creating `WowDiagnostic` instances
- `DiagnosticPass` trait with `visit_node()` (AST walk), `run()` (full-analysis pass), and `run_inject()` (inject-field pipeline) methods
- `run_all()` orchestrates all passes in three phases: `run` passes, `visit_node` passes (AST walk), and `run_inject` passes (type-mismatch → inject-field pipeline)
- All diagnostic code constants are defined centrally in `mod.rs` (e.g. `DEPRECATED`, `TYPE_MISMATCH`, `SAFETY_LIMIT`)
- `CATALOG` array collects all definitions for validation; `DEFAULT_DISABLED_CODES` lists opt-in codes; `CODE_ALIASES` maps LuaLS codes to ours

The ~40 diagnostic modules under `src/diagnostics/` and their per-module behavior / false-positive rationale are catalogued in [ARCHITECTURE.md — Diagnostics — module reference](.claude/ARCHITECTURE.md#diagnostics--module-reference). The authoritative code list is the `CATALOG` array in `src/diagnostics/mod.rs`; the user-facing reference is [`docs/reference/diagnostics.md`](docs/reference/diagnostics.md).

To add a new diagnostic: add a `DiagnosticDef` constant to `mod.rs`, create `src/diagnostics/new_thing.rs` implementing `DiagnosticPass`, add `mod new_thing;` to `mod.rs`, register the pass in `run_all()`, and add the constant to `CATALOG`. Suppression via `@diagnostic disable:new-thing` works automatically by matching the code string. Some modules are "hybrid": they implement `DiagnosticPass` for the post-analysis phase AND export `pub(crate)` helper functions called from `build_ir.rs` / `resolve.rs` during IR construction. **Also add the diagnostic to the table in `README.md`.**

## Documentation

`docs/` contains the user-facing documentation site (VitePress). `docs/reference/annotations.md` is the annotation reference, `docs/reference/diagnostics.md` is the diagnostics reference, and `docs/guide/` has topical guides. When adding new features, annotations, or diagnostics, update the relevant docs pages. When removing something from `README.md`, consider where users will discover it instead — if nowhere, move it to a less prominent section rather than deleting it. CLAUDE.md is for developer/AI-facing architecture notes only — do not put user-facing documentation here.

## Bug fixes

When fixing a bug, always add a regression test covering the fix. Add test assertions to the appropriate existing test file (see the test file layout in [.claude/TESTING.md](.claude/TESTING.md)) using the annotation format (`hover:`, `def:`, `sig:`, `diag:`, etc.). Run `cargo test --workspace` to confirm the new test passes.

### Investigating false positives in real addon code

**CRITICAL**: When reproducing a diagnostic false positive reported in a real addon (e.g. TradeSkillMaster), **always use `--scan-dir` pointing to the FULL addon root** — not a subdirectory. A partial scan misses cross-file classes, defclass calls, inherited fields, and addon namespace resolution, producing many spurious diagnostics that don't exist with the full scan. First reproduce the exact diagnostic with a full scan before investigating the code.

```bash
# WRONG — partial scan produces false positives that mask the real issue:
cargo run -- test-query /path/to/addon/SubLib/Source/File.lua:386:1 --with-stubs --scan-dir /path/to/addon/SubLib

# RIGHT — full workspace scan for accurate diagnostics:
cargo run -- test-query /path/to/addon/SubLib/Source/File.lua:386:1 --with-stubs --scan-dir /path/to/addon
```

## Conventions

- Byte offsets are `u32` throughout the IR (not `usize`)
- `SymbolIndex`, `FunctionIndex`, `TableIndex`, `ExprId` are all `usize` type aliases
- Symbol versions track reassignments: `local x = 1; x = "hi"` creates two versions
- External data is immutable after `PreResolvedGlobals::build()`
- `@meta` files suppress all diagnostics (they're declaration-only stubs)
- **Never special-case specific functions** (e.g. `tinsert`, `table.insert`) in the LS engine code. Behavior differences should be expressed through stub annotations (`@generic`, `@overload`, etc.) so the general type system handles them.
- **Workspace rebuild comparisons** (`main_loop/rebuild.rs`): `globals_match()`, `classes_match()`, `aliases_match()`, `events_match()`, and `self_fields_match()` use allow-list field comparisons to ignore positional/display-only fields (e.g. `def_range`, `field_ranges`, `byte_range`). When adding a new semantic field to `ExternalGlobal`, `ClassDecl`, `AliasDecl`, `EventDecl`, or `TypedSelfField`, you **must** add it to the corresponding `*_match()` function — otherwise edits that change that field won't trigger a workspace rebuild.
- **Structured logging**: Use `log::info!`, `log::warn!`, `log::error!`, `log::debug!` instead of `eprintln!`. The logger (`env_logger`) is initialized in `main.rs`; library code uses `log::` macros directly. `RUST_LOG` env var controls filtering at runtime.
- **Zero warnings policy**: Always run `cargo build --workspace` and `cargo clippy --workspace` after completing changes and ensure there are zero warnings before considering work done. If clippy suggests a fix, apply it. Do not add `#[allow(clippy::...)]` suppressions unless there's a documented reason in a code comment. The policy is encoded in the root `[workspace.lints]` table (inherited by every member crate via `lints.workspace = true`): rustc `warnings = "deny"`, the whole `clippy::all` group denied, plus the promoted pedantic lints `redundant_else`, `stable_sort_primitive`, and `cloned_instead_of_copied`. Because `warnings = "deny"` also applies to rustc, plain `cargo build`/`cargo test` now fail on warnings too — keep test/bench code clean, not just lib/bin (`cargo clippy --workspace --all-targets` is the full check). Since that gates ordinary builds, the toolchain is pinned in `rust-toolchain.toml` so a floating-stable compiler update can't surface a new default-on warning and break a build (including the CI release build); bump that pin deliberately and re-run the full clippy check when you do. When promoting a new lint, make the tree clean under it first. The `fuzz/` crate is workspace-excluded and does not inherit these.
- **No real addon code in source**: Never use code from real addons (e.g. TradeSkillMaster) in source comments, test names, or examples. Always generalize to fictional/generic examples.
- **Never `git stash` in a worktree**: All worktrees of a repo share a single stash stack (it lives on the common git dir, not per-worktree). Concurrent workspaces running `git stash push` / `pop` will clobber each other's entries. To shelve changes, use a per-worktree WIP commit (`git commit -m WIP`, reset later) or write to a uniquely-named ref (`git stash create` + `git update-ref refs/wip/<name>`).

### Type-system & annotation features
These annotation-syntax and `ValueType` features are conventions — load-bearing typing rules — but their mechanism detail lives in [ARCHITECTURE.md — Annotation & type-system feature reference](.claude/ARCHITECTURE.md#annotation--type-system-feature-reference) to keep this file's always-loaded footprint small. One-line index (each links to the full note):

- `@field name? type` — optional field; `?` stripped from the name, type wrapped in `Union(T, nil)`. [→](.claude/ARCHITECTURE.md#field-name-type-optional-fields)
- `@alias (opaque) Name type` — nominally distinct alias (`ValueType::OpaqueAlias`); different opaque aliases aren't mutually assignable. [→](.claude/ARCHITECTURE.md#alias-opaque-name-type-nominal-aliases)
- `@class (partial) Foo` / `(exact)` — parse-only class modifiers (accepted, no diagnostic effect). [→](.claude/ARCHITECTURE.md#class-partial-foo--exact)
- `@class Foo : table<K, V>` — class inheriting dictionary key/value types (typed `pairs()`). [→](.claude/ARCHITECTURE.md#class-foo--tablek-v-dictionary-inheritance)
- `T & U` — intersection type (`AnnotationType::Intersection` / `ValueType::Intersection`). [→](.claude/ARCHITECTURE.md#t--u-intersection-type)
- `T!` — non-nil assertion / lateinit (`AnnotationType::NonNil`; sets `FieldInfo.lateinit`). [→](.claude/ARCHITECTURE.md#t-non-nil-assertion--lateinit)
- `{field: type, ...}` — anonymous table shape (`AnnotationType::TableLiteral`). [→](.claude/ARCHITECTURE.md#field-type--anonymous-table-shape)
- `[T1, T2, ...]` — LuaLS tuple syntax, lowered to an integer-keyed `TableLiteral`. [→](.claude/ARCHITECTURE.md#t1-t2--luals-tuple-syntax)
- `?T` — prefix-optional shorthand, identical to `T?`. [→](.claude/ARCHITECTURE.md#t-prefix-optional-shorthand)
- Comma-separated `@return T1, T2` — LuaLS single-line multi-return. [→](.claude/ARCHITECTURE.md#comma-separated-return-t1-t2-luals-single-line-multi-return)
- LuaLS-only `@diagnostic` codes — accepted silently (`LUALS_ONLY_CODES`), suppress nothing. [→](.claude/ARCHITECTURE.md#luals-only-diagnostic-codes)
- Number-literal types (`0` / `-1` / `0xFF` → `ValueType::NumberLiteral`, kept last in the enum). [→](.claude/ARCHITECTURE.md#number-literal-types-0---1--0xff)
- `ValueType::FunctionSig(Box<FunctionShape>)` — inline, cross-file-safe function signature (runtime-only). [→](.claude/ARCHITECTURE.md#valuetypefunctionsigboxfunctionshape-inline-function-signature)
- `ValueType::TableShape(Box<TableShape>)` — inline, cross-file-safe table shape (runtime-only). [→](.claude/ARCHITECTURE.md#valuetypetableshapeboxtableshape-inline-anonymous-table-shape)
- `NarrowKind::NumCompare { op, bound }` — numeric case discrimination (`if x > 1`). [→](.claude/ARCHITECTURE.md#narrowkindnumcompare--op-bound--numeric-case-discrimination)
- `...T` — variadic return (`AnnotationType::VarArgs`; sets `Function.has_vararg_return`). [→](.claude/ARCHITECTURE.md#t-variadic-return)
- `@generic T, ...M` — variadic generic; excess args collected into an `Intersection`. [→](.claude/ARCHITECTURE.md#variadic-generics-generic-t-m)
- `@narrows-arg N` — in-place argument narrowing (incl. `Mixin(self.Child, M)` field targets). [→](.claude/ARCHITECTURE.md#narrows-arg-n-in-place-argument-type-mutation)
- `@returns-class-name` — return-value class guard (`GetObjectType() == "Class"`; never hard-code the method name). [→](.claude/ARCHITECTURE.md#returns-class-name-return-value-class-guard)
- `@creates-global N` — implicit named-global side effect (`CreateFrame`; never hard-code creating-fn names). [→](.claude/ARCHITECTURE.md#creates-global-n-implicit-named-global-side-effect)
- `@generates-events N [Field]` — synthesized event enum table on a class (never hard-code the method/field). [→](.claude/ARCHITECTURE.md#generates-events-n-field-synthesized-event-enum-table-on-a-class)
- `@callback-event-arg N` — callback-registry event-name validation/completion (pairs with `@generates-events`). [→](.claude/ARCHITECTURE.md#callback-event-arg-n-callback-registry-event-name-validationcompletion)
- `@requires T: Constraint` — method availability gating (`param-constraint-mismatch`). [→](.claude/ARCHITECTURE.md#requires-t-constraint-method-availability-gating)
- Mixin-object params accept the data type; methods-typed params are strict (replaces the former `@shape`). [→](.claude/ARCHITECTURE.md#mixin-object-params-accept-the-data-type-methods-typed-params-are-strict)
- `@return self<X>` — re-parameterized self return (`Function.returns_self_type_args`). [→](.claude/ARCHITECTURE.md#return-selfx-re-parameterized-self-return)
- `@alias Foo<K,V> V[]` — parameterized alias, with constraint enforcement at every use site. [→](.claude/ARCHITECTURE.md#alias-fookv-v-parameterized-alias)
- Deferred constructor self-field type args — cross-file generic recovery (harvested lazily). [→](.claude/ARCHITECTURE.md#deferred-constructor-self-field-type-args-cross-file-generic-recovery)

## Testing

```bash
# Run all tests. Use --workspace: the root is a package-in-a-workspace, so a bare
# `cargo test` runs only the root package's tests and skips the lower crates'
# unit tests (syntax/core/analysis/lsp).
cargo test --workspace

# Check all diagnostics across a workspace (the primary way to verify diagnostic behavior)
cargo run -- check /path/to/addon
# Filter to a specific file:
cargo run -- check /path/to/addon | grep "FileName.lua"
# Include hints (default is warnings+errors only):
cargo run -- check /path/to/addon --severity hint

# Evaluate a single file with type info
cargo run -- evaluate tests/annotations.lua

# Test hover/definition/signature/diagnostics at line:col
cargo run -- test-query tests/integration_stubs.lua:4:10 --with-stubs

# Test hover/definition/signature/diagnostics against a real addon project
# Use --scan-dir to load the full workspace so cross-file defclass, @builds-field,
# and addon namespace resolution all work. This is slow but necessary for accurate
# results when investigating issues in real addon code.
cargo run -- test-query /path/to/addon/File.lua:LINE:COL --with-stubs --scan-dir /path/to/addon

# Dump hover types for every identifier in a workspace (for regression baselines)
cargo run --release -- dump-types /path/to/addon --with-stubs > baseline.txt
# After changes, diff to catch hover regressions:
cargo run --release -- dump-types /path/to/addon --with-stubs | diff baseline.txt -

# Smoke test (build + integration tests)
./tests/smoke.sh
```

Test expectations are embedded as comments below code lines, fields separated by double-space (`hover:`, `def:`, `defs:`, `typedef:`, `typedefs:`, `sig:`, `diag:`, `refs:`, `comp:`, `tok:`, `hint:`, `lens:`). Two rules that are easy to trip on:

- **Exhaustive diagnostic checking** — the harness fails on *any* uncovered diagnostic (WARNING/ERROR/HINT), so a new unexpected diagnostic fails the test automatically. Therefore **never write `diag: none`** (it's redundant); only assert `diag: <code>` for a diagnostic that *should* fire. Suppress incidental diagnostics with a top-of-file `---@diagnostic disable: code1, code2`.
- **Default-off codes** (`need-check-nil`, `implicit-nil-return`, `unused-vararg`, `incomplete-signature-doc`, `unknown-*-type`) must live in a subdirectory with an adjacent `.wowluarc.json` that opts in via `diagnostics.enable`.

The full per-file test catalog and the complete field semantics (`defs:`/`typedefs:` counts, `tok:`/`lens:` formats, the harness config-filter path) are in [.claude/TESTING.md](.claude/TESTING.md).

## Stubs
WoW API stubs live in `stubs/`. Scanned at startup by `scan_workspace()` / `scan_stubs_for_test()`. Stubs are precomputed and checked in; they are regenerated by `cargo run -- regenerate-stubs`, which clones [Ketho/vscode-wow-api](https://github.com/Ketho/vscode-wow-api) to a temp directory. Local overrides live in `stubs/overrides/`.

Stub generation (including Classic-only globals from the wiki and BlizzardInterfaceResources) is handled by `src/stub_gen/`. Run `cargo run -- regenerate-stubs` to regenerate precomputed stubs. **Any change to `src/stub_gen/` or `stubs/overrides/` requires regenerating stubs and committing the updated `stubs/precomputed.bin.zst` and `stubs/precomputed-files.bin.zst`.** After resolving rebase/merge conflicts on the precomputed stub binaries (accepting either side), always re-run `cargo run -- regenerate-stubs` to ensure the blob is consistent with the current `stubs/overrides/` content.

### Fixing missing or incorrect API stubs
When a WoW API function is missing or has wrong types, **always fix the root cause in `src/stub_gen/`** rather than adding a one-off override in `stubs/overrides/`. There are thousands of WoW API functions; adding overrides for individual missing APIs doesn't scale and masks systemic discovery bugs. Trace the function through the stub generation pipeline (wiki categories, BlizzardInterfaceResources GlobalAPI.lua branches, Blizzard APIDocumentationGenerated, dedup filters) to find where it's being dropped or mistyped. Overrides in `stubs/overrides/` are reserved for cases where the upstream source data is fundamentally wrong (e.g. Blizzard's documentation declares a numeric enum type but the Lua API actually accepts strings) — not for patching gaps in the generation pipeline.

### Embedded vs external stubs (`embedded-stubs` feature)
The `embedded-stubs` Cargo feature (default on) controls how the binary loads precomputed stubs:
- **With feature (default):** Stubs are baked into the binary via `include_bytes!`. Produces a self-contained executable for standalone release downloads.
- **Without feature (`--no-default-features`):** Stubs are loaded at runtime from a `stubs/` directory next to the executable (resolved via `std::env::current_exe()`). Used for universal editor plugin packages that share one copy of the stubs across per-platform binaries.

Both modes use the same version-checking logic (`BLOB_MAGIC`/`BLOB_VERSION`). The implementation is in `src/lsp/main_loop/stub_loading.rs` (`load_precomputed_stubs()`, `stub_file_contents()`, `stubs_dir()`).

## Fuzzing

Three cargo-fuzz targets in `fuzz/`: `fuzz_lexer`, `fuzz_parser`, `fuzz_analysis` (full pipeline with `PreResolvedGlobals::empty()`). Run after parser/analysis changes. See [Fuzzing](docs/guide/development.md#fuzzing) for setup and usage.

## Profiling

```bash
# Profile against an addon directory (parses + analyzes all .lua files)
cargo run --release -- profile /path/to/addon
```

## Editor Extensions

### Shared TextMate grammar
Both the VS Code and JetBrains plugins use the same Lua TextMate grammar (`editors/vscode/syntaxes/lua.tmLanguage.json`). The JetBrains plugin vendors a copy at `editors/jetbrains/textmate/lua/syntaxes/lua.tmLanguage.json`. **When updating the grammar, copy it to both locations.**

### VS Code Extension Development

The VS Code extension requires two build steps before launching:
1. `cargo build` — build the language server binary
2. `cd editors/vscode && npm run build` — bundle the extension JS (`extension.js` → `dist/extension.js` via esbuild). The `package.json` `main` field points to `./dist/extension.js`, so the extension will fail to activate without this step.

When using `/vscode`, check whether VS Code already has a window open for the target folder **before** launching. If it does, stop and ask the user to close it — VS Code reuses the existing window and silently ignores the new `--extensionDevelopmentPath`, so the dev build won't load. The `--new-window` flag does not reliably fix this. Warning the user *after* launching is too late; the wrong instance is already foregrounded.
