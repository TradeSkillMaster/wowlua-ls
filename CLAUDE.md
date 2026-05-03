# wowlua_ls — WoW Lua Language Server

A Language Server Protocol implementation for Lua (World of Warcraft API dialect). Provides hover, go-to-definition, completion, signature help, find references, rename, and diagnostics.

For deep architecture internals (type inference, narrowing, generics, builder pattern, cross-file references, metatable inference, flavor filtering, etc.), see [ARCHITECTURE.md](.claude/ARCHITECTURE.md).

## Architecture

### Source files
- `src/main.rs` — CLI entry point: `evaluate` subcommand, `test-query` subcommand (hover/def/sig/completions/diagnostics), otherwise starts LSP
- `src/types.rs` — IR type definitions: `ValueType`, `Expr`, `Symbol`, `Scope`, `Function`, `TableInfo`, `FieldInfo`, deferred check structs, index aliases, `EXT_BASE`
- `src/analysis/` — Core per-file analysis engine (`Analysis` struct):
  - `mod.rs` — `Ir` struct definition, scope-chain walking helpers, two-tier lookups, core helpers
  - `prescan.rs` — Phase 0: class/alias pre-scan, annotation type resolution, generic inference
  - `build_ir.rs` — Phase 1: AST walk, scope/symbol/function/table creation, correlated return inference
  - `lower_expression.rs` — Expression lowering from AST to IR `Expr`: literals, identifiers (`NameRef`, `DotAccess`, `BracketAccess`, `MethodCall`), function calls, binary ops, table constructors, inline `@as` casts
  - `narrowing.rs` — Type narrowing from control flow guards: `GuardNarrow` enum, `OrTermEffect`, flavor narrowing detection, `@flavor-narrows`, type filter/strip for scope-specific refinement
  - `resolve.rs` — Phase 2: fixpoint type resolution loop, expression resolver, backward param-type inference
  - `resolve_call.rs` — Function call resolution: `CallSiteInfo`, argument count/type checking, return type determination, overload matching, generic binding
  - `checks.rs` — Diagnostic check orchestration via `run_diagnostics()`, name-token collection for access diagnostics
  - `queries.rs` — LSP query methods: hover, definition, completion, signature help, references, rename, inlay hints, code lens
  - `semantic_tokens.rs` — LSP semantic-token classification. Narrow by design: walks only bare `Name` tokens (skips field/method access and parameters) and emits a `function` token when the symbol resolves to a function value. Everything else is left to the editor's built-in Lua grammar, so coloring matches pre-feature behavior. Modifiers: `defaultLibrary` for stub symbols (via `is_stub_symbol()` — `idx - EXT_BASE < stub_symbols_end`, boundary captured at `load_precomputed_stubs()` time), `deprecated` when the resolved function is `@deprecated`. Legend is the `SEMANTIC_TOKEN_TYPES` / `SEMANTIC_TOKEN_MODIFIERS` arrays; encoded into LSP wire format by `main_loop.rs::encode_semantic_tokens`.
- `src/pre_globals/` — Precomputed global type database:
  - `mod.rs` — `PreResolvedGlobals` struct, 5-phase build from WoW API stubs, type parameter substitution, class/alias/function registration
  - `build_on_stubs.rs` — `BuildOnStubsContext` for workspace incremental builds on precomputed stubs: scope/symbol/function/table cloning, type parameter substitution, field resolution
- `src/annotations/` — Annotation system (types, parsing, cross-file scanning):
  - `mod.rs` — Core types (`AnnotationType`, `ParamInfo`, `Visibility`, `ClassDecl`, `AliasDecl`, `AnnotationBlock`), comment extraction (`extract_annotations`), full-file class/alias discovery (`scan_all_annotations`), line-level `@tag` dispatch (`parse_annotation_lines`), tuple-union lowering (`lower_tuple_form_cases`), re-exports from submodules
  - `annotation_types.rs` — Type expression parsing: `parse_type()`, `parse_overload()`, `parse_return_line()`, `format_annotation_type()`, `substitute_alias_type_params()`, `match_projection()`, and internal helpers (`split_at_top_level`, `extract_type_prefix`, etc.)
  - `annotation_scanning.rs` — Shared types (`ExternalGlobal`/`ExternalGlobalKind`/`FieldValueKind`), constants (`ADDON_NS_NAME`), shared helpers (`extract_type_annotation_for_assign`, `extract_inline_type_annotation`, `is_select_varargs`), `scan_method_typed_self_fields()`, `scan_diagnostic_directives()`, type conversion (`resolve_annotation_type`, `reduce_to_fun_alias`)
  - `scan_globals.rs` — `scan_file_globals[_with_synth]()`, workspace-level `synthesize_return_only_overloads_for_body()`, and private synth_* helpers
  - `scan_defclass.rs` — `scan_defclass_calls()` with constructor self-field extraction, defclass chain walking, and type inference helpers
  - `scan_built_name.rs` — `scan_built_name_calls()` with builder-chain field extraction, generic substitution, and `@builds-field` resolution
- `src/flavor.rs` — 3-flavor bitmask (retail/classic/classic_era matching WoW's install-folder names), `from_ketho_mask()` that collapses Ketho's 4-bit (mainline/mists/bcc/classic_era) into ours (mists and bcc both map to classic), name parsing, and narrowing helpers for `wrong-flavor-api`
- `src/diagnostics/` — Trait-based diagnostic architecture with centralized catalog (see [Diagnostics](#diagnostics) below)
- `src/syntax/parser.rs` — Recursive descent + Pratt parser producing arena-based `SyntaxTree`
- `src/syntax/tree.rs` — Arena-based syntax tree: `SyntaxTree`, `Node`, `Token`, `NodeId`, `TokenId`, `TreeBuilder` with checkpoint support; also high-level API wrappers (`SyntaxNode`, `SyntaxToken`, `TextRange`, `TextSize`, `TokenAtOffset`, `NodeOrToken`)
- `src/syntax/syntax_kind.rs` — `SyntaxKind` enum (unified token + node kinds)
- `src/syntax/lexer.rs` — Tokenization
- `src/ast.rs` — AST node definitions and casts over `SyntaxNode` (uses `define_ast_node!` macro)
- `src/config.rs` — Project configuration: `.wowluarc.json` loading, `.toc` `SavedVariables` parsing, ignore patterns, diagnostic overrides, allowed globals, `inference.backward_param_types`, `inference.correlated_return_overloads`, `hint.*` inlay hint config, `addon_root` for multi-addon namespace isolation (`addon_root_for()` / `addon_roots()`)
- `src/stub_gen.rs` — Stub generation: fetches WoW API stubs, Classic globals from wiki/BlizzardInterfaceResources, and serializes precomputed `PreResolvedGlobals` blob (replaces former Python scripts)
- `src/lsp/main_loop.rs` — LSP server loop, request handlers, `scan_stubs_for_test()`
- `src/lsp/diagnostics.rs` — Diagnostic publishing with `@diagnostic` suppression and project-wide config overrides
- `src/lsp/uri.rs` — URI/path conversion utilities (percent-encoding, Windows drive letters, spaces)

### Two-tier index space (EXT_BASE)
External globals (WoW API stubs) use indices >= `EXT_BASE` (1,000,000). Per-file locals use indices < `EXT_BASE`. All lookup functions (`sym()`, `func()`, `table()`, `expr()`) route via `idx >= EXT_BASE` check. This avoids cloning ~9000 external symbols per file.

### Key query functions (in `queries.rs`)
- `find_symbol_at(offset)` — Resolves direct names: gets token at offset → scope lookup → returns `(SymbolIndex, name)`
- `find_field_at(offset)` — Resolves dot/colon chains (`x.y.z`): walks table fields to find the target field's `ExprId`
- `scope_at_offset(offset)` — Finds innermost scope containing offset via `block_scopes` ranges
- `get_symbol(id, scope_idx)` — Walks scope hierarchy upward; at scope 0 also checks `ext.scope0_symbols` (in `analysis/mod.rs`)

### Inlay hints (in `queries.rs`)
`inlay_hints(tree, config)` collects six categories of inline annotations controlled by `InlayHintConfig` (from `.wowluarc.json` `hint.*` fields, enabled by default unless noted):
1. **Parameter names** (`collect_param_name_hints`) — iterates `call_resolutions`, emits `InlayHintKind::PARAMETER` before each argument. Suppressed when: arg text matches param name (case-insensitive), param is `self`, param is vararg, or param name is empty.
2. **Variable types** (`collect_local_type_hints`) — walks `LocalAssignStatement` nodes, emits `InlayHintKind::TYPE` after each name token. Suppressed when: variable has `@type` annotation, resolved type is `Any`/`Nil`/`Function`, or RHS is a function literal. Per-variable check (not per-statement).
3. **Function return types** (`collect_function_return_hints`) — matches functions by `def_node.start`, emits after the parameter list close paren. Suppressed when: function has `@return` annotation, `returns_self`, or `explicit_void_return`.
4. **For-loop variable types** (`collect_forin_type_hints`) — walks `ForInLoop` nodes, emits after each name token. Suppressed when: variable has `@type` annotation or resolved type is `Any`.
5. **Parameter types** (`collect_param_type_hints`, **disabled by default**, `hint.parameterTypes`) — walks `FunctionDefinition` nodes, emits `InlayHintKind::TYPE` after each parameter name token. Suppressed when: parameter has a `@param` annotation, resolved type is `Any`/`Nil`, or parameter is `self`.
6. **Chained method return types** (`collect_chained_return_hints`, **disabled by default**, `hint.chainedReturnTypes`) — iterates `call_resolutions`, emits `InlayHintKind::TYPE` at the closing `)` of calls whose return value is used as the receiver of a subsequent field/method access. Suppressed when: return type is `Any`/`Nil` or formats to `"?"`. Only intermediate calls in a chain get hints (the final call is covered by variable type hints).

All type hints use `format_type_depth(resolved, 1)` (depth 1) to avoid expanding table fields with newlines — inlay hints show class names only, not field listings.

LSP handler in `main_loop.rs` converts `InlayHintData` (byte offsets) to LSP `InlayHint` (line/column positions). Config is built from `ProjectConfigs` per-file hierarchy.

### Code lens (in `queries.rs` + `main_loop.rs`)
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
- All 60 diagnostic code constants are defined centrally in `mod.rs` (e.g. `DEPRECATED`, `TYPE_MISMATCH`, `SAFETY_LIMIT`)
- `CATALOG` array collects all definitions for validation; `DEFAULT_DISABLED_CODES` lists opt-in codes; `CODE_ALIASES` maps LuaLS codes to ours

Diagnostic modules under `src/diagnostics/` (39 modules implementing `DiagnosticPass` or exporting helpers):

**Type system checks:**
- `type_mismatch.rs` — argument type mismatches against `@param` (`type-mismatch`)
- `return_mismatch.rs` — return type mismatches against `@return` (`return-mismatch`)
- `field_type_mismatch.rs` — field assignment type mismatches against `@field` (`field-type-mismatch`)
- `assign_type_mismatch.rs` — reassignment type mismatches against `@type` (`assign-type-mismatch`)
- `grouped_return_mismatch.rs` — return values not matching any tuple-union `@return` case (`grouped-return-mismatch`)
- `generic_constraint_mismatch.rs` — generic type constraint violations at call sites and class type params (`generic-constraint-mismatch`)
- `missing_return_value.rs` — return statements with fewer values than `@return` (`missing-return-value`)
- `missing_return.rs` — functions missing return statements (`missing-return`)
- `missing_fields.rs` — missing required fields when constructing `@class` tables (`missing-fields`)

**Function/call checks:**
- `call_arity.rs` — argument count validation: `redundant-parameter` (extra args) and `missing-parameter` (insufficient args), handles method calls, varargs, optional params, and projected arity from `params<F>`
- `discard_returns.rs` — ignored `@nodiscard` return values (`discard-returns`)
- `multi_return_projection.rs` — `returns<F>` truncation when F has >1 return annotation (`multi-return-projection`)

**Variable/field/global checks:**
- `undefined_global.rs` — references to unresolved global names (`undefined-global`)
- `undefined_field.rs` — accessing nonexistent fields on `@class` tables (`undefined-field`)
- `unused_local.rs` — unreferenced local variables (`unused-local`, HINT)
- `redefined_local.rs` — same-scope local variable redefinition (`redefined-local`)
- `duplicate_index.rs` — duplicate keys in table constructors (`duplicate-index`)
- `duplicate_set_field.rs` — setting a field already set on `@class` tables (`duplicate-set-field`)
- `inject_field.rs` — setting undeclared fields on `@class` tables (`inject-field`, HINT)
- `create_global.rs` — implicit global creation via assignment or function definition (`create-global`)

**Access control:**
- `access.rs` — `@private`/`@protected` visibility violations (`access-private`, `access-protected`)
- `need_check_nil.rs` — field/method access on possibly-nil values (`need-check-nil`, default-disabled)
- `wrong_flavor_api.rs` — calls to APIs not available in project-declared flavors (`wrong-flavor-api`)

**Annotation validation:**
- `function_annotation_checks.rs` — comprehensive function-level annotation validation: `@param` name mismatches (`undefined-doc-param`), duplicate `@param` (`duplicate-doc-param`), `@return` type resolution, `@overload` type resolution, `@generic` on class methods (`redundant-class-generic`), and `params<F>` position/shape validation
- `annotation_metadata.rs` — annotation comment scanning: duplicate `@constructor` (`duplicate-constructor`), `@constructor` return validation (`constructor-return`), `@builds-field` without `@return self` (`builds-field-not-self`), `@return ClassName` instead of `@return self` (`return-self-class-name`, HINT), bare `return` with all-optional `@return` types (`implicit-nil-return`, HINT), duplicate `@field` (`duplicate-doc-field`), duplicate `@alias` (`duplicate-doc-alias`)
- `malformed_annotation.rs` — unknown or incomplete `---@` annotations (`malformed-annotation`)
- `doc_field_no_class.rs` — `@field` annotations not preceded by `@class` (`doc-field-no-class`)
- `doc_func_no_function.rs` — function-level annotations (`@param`, `@return`, `@overload`, `@generic`, `@nodiscard`, `@deprecated`, `@constructor`, `@builds-field`, `@built-name`, `@built-extends`, `@flavor-narrows`, `@type-narrows`, `@defclass`) not attached to a function definition (`doc-func-no-function`)
- `undefined_doc_class.rs` — undefined class names in `@class Foo: Parent` inheritance and circular inheritance chains (`undefined-doc-class`, `circle-doc-class`)
- `undefined_doc_name.rs` — undefined type names in annotations (`undefined-doc-name`)
- `unknown_diag_code.rs` — unknown code in `@diagnostic` directives (`unknown-diag-code`)
- `incomplete_signature_doc.rs` — functions with partial `@param`/`@return` annotations (`incomplete-signature-doc`, HINT, default-disabled)

**AST & style checks:**
- `ast_checks.rs` — AST-traversing pass consolidating: empty blocks (`empty-block`, HINT), unbalanced assignments (`unbalanced-assignments`), redundant values (`redundant-value`), redundant return values (`redundant-return-value`), code after break (`code-after-break`, HINT), unreachable code after return (`unreachable-code`, HINT), count-down loops (`count-down-loop`), unused functions (`unused-function`, HINT), redundant return (`redundant-return`, HINT), deprecated symbol usage (`deprecated`)
- `trailing_space.rs` — lines ending with whitespace (`trailing-space`, HINT)
- `not_precedence.rs` — `not x <cmp> y` parsing as `(not x) <cmp> y` (`not-precedence`, HINT)
- `unused_vararg.rs` — functions declaring `...` but never referencing it (`unused-vararg`, HINT, default-disabled)

**Unknown-type diagnostics (strict typing, all default-disabled):**
- `unknown_param_type.rs` / `unknown_return_type.rs` / `unknown_local_type.rs` / `unknown_field_type.rs` — sites whose type couldn't be inferred (`unknown-param-type`, `unknown-return-type`, `unknown-local-type`, `unknown-field-type`, HINT). See [ARCHITECTURE.md — Unknown-type diagnostics](.claude/ARCHITECTURE.md#unknown-type-diagnostics-strict-typing).

**Special:**
- `safety-limit` (ERROR) — emitted when analysis is incomplete due to safety limits

To add a new diagnostic: add a `DiagnosticDef` constant to `mod.rs`, create `src/diagnostics/new_thing.rs` implementing `DiagnosticPass`, add `mod new_thing;` to `mod.rs`, register the pass in `run_all()`, and add the constant to `CATALOG`. Suppression via `@diagnostic disable:new-thing` works automatically by matching the code string. Some modules are "hybrid": they implement `DiagnosticPass` for the post-analysis phase AND export `pub(crate)` helper functions called from `build_ir.rs` / `resolve.rs` during IR construction. **Also add the diagnostic to the table in `README.md`.**

## Documentation

`docs/` contains the user-facing documentation site (VitePress). `docs/reference/annotations.md` is the annotation reference, `docs/reference/diagnostics.md` is the diagnostics reference, and `docs/guide/` has topical guides. When adding new features, annotations, or diagnostics, update the relevant docs pages. When removing something from `README.md`, consider where users will discover it instead — if nowhere, move it to a less prominent section rather than deleting it. CLAUDE.md is for developer/AI-facing architecture notes only — do not put user-facing documentation here.

## Bug fixes

When fixing a bug, always add a regression test covering the fix. Add test assertions to the appropriate existing test file (see test file layout below) using the annotation format (`hover:`, `def:`, `sig:`, `diag:`, etc.). Run `cargo test` to confirm the new test passes.

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
- `@field name? type` — the `?` is stripped from the field name at parse time in `annotations.rs` and the type is wrapped in `Union(type, nil)`. Field HashMap keys never contain `?`. Same pattern as `@param name?` handling.
- `@class (partial) Foo` — Parsed in `annotations/mod.rs` by stripping the `(partial)` prefix before the class name. `(exact)` is also recognized. Currently parse-only — the modifier is accepted but has no effect on diagnostics.
- `@class Foo : table<K, V>` — Class inheriting dictionary key/value types. Parent name parsing in `annotations/mod.rs` uses `split_at_top_level` to handle commas inside `<>`. During inheritance resolution (prescan.rs pass 3, pre_globals/mod.rs, pre_globals/build_on_stubs.rs), `table<K,V>` parents set `key_type`/`value_type` on the child's `TableInfo` without adding to `parent_classes`. This enables typed `pairs()` iteration over class-typed tables. The `undefined-doc-class` diagnostic skips parameterized parents (those containing `<`).
- `T & U` (intersection type) — `AnnotationType::Intersection(Vec<AnnotationType>)` / `ValueType::Intersection(Vec<ValueType>)`. Parsed via `&` with higher precedence than `|` (split `|` first, then `&` inside each union member). An intersection is assignable to X if ANY member is; X is assignable to an intersection if assignable to ALL members. Field access checks all member tables. Used by `CreateFrame` stub to combine frame type with template mixin (`T & Tp`).
- `T!` (non-nil assertion / lateinit) — `AnnotationType::NonNil(Box<inner>)` wraps the inner type. Resolves to the inner type with nil stripped. On `@field` or `---@type`, sets `FieldInfo.lateinit = true`, which suppresses `field-type-mismatch` for nil assignments and ensures the field's resolved type is non-nil (no `need-check-nil` on access). Hover shows `T!`.
- `{field: type, ...}` (anonymous table shape) — `AnnotationType::TableLiteral(Vec<(String, AnnotationType)>)`. Parsed in `parse_type()` when the string starts with `{` and ends with `}`, splitting on `,` at top level and then `field: type` pairs. Resolves via `materialize_table_literal()` in `prescan.rs` which creates a `TableInfo` with the specified fields. Supports optional fields (`field?: type`) which become `Union(type, nil)`. Works in `@param`, `@return`, `@type`, `@alias`, and inside intersections (`T & {field: type}`).
- `...T` (variadic return) — `AnnotationType::VarArgs(Box<AnnotationType>)`. When `@return ...T` is the last return annotation, it fills all remaining return slots with type `T`. Stored as `Function.has_vararg_return: bool`. The vararg portion is optional (no `missing-return-value` for it) and `redundant-return-value` is suppressed. Multiple returns must use separate `@return` lines (comma-separated multi-return on a single `@return` line is not supported).
- `@alias Foo<K,V> V[]` (parameterized alias) — `AliasDecl` has `type_params: Vec<String>`. Stored in `ir.parameterized_aliases` / `ext.parameterized_aliases` as `(Vec<String>, AnnotationType)` (type params + raw body). When `Parameterized("Foo", args)` is encountered during resolution, `substitute_alias_type_params()` replaces type param names in the body with concrete args, then the substituted type is resolved normally. Supports colon syntax (`@alias Foo<K,V>: V[]`) and spaces in type params (`@alias Foo<K, V>`).
- **Never special-case specific functions** (e.g. `tinsert`, `table.insert`) in the LS engine code. Behavior differences should be expressed through stub annotations (`@generic`, `@overload`, etc.) so the general type system handles them.
- **Structured logging**: Use `log::info!`, `log::warn!`, `log::error!`, `log::debug!` instead of `eprintln!`. The logger (`env_logger`) is initialized in `main.rs`; library code uses `log::` macros directly. `RUST_LOG` env var controls filtering at runtime.
- **Zero warnings policy**: Always run `cargo build` and `cargo clippy --lib` after completing changes and ensure there are zero warnings before considering work done. If clippy suggests a fix, apply it. Do not add `#[allow(clippy::...)]` suppressions unless there's a documented reason in a code comment.
- **No real addon code in source**: Never use code from real addons (e.g. TradeSkillMaster) in source comments, test names, or examples. Always generalize to fictional/generic examples.
- **Never `git stash` in a worktree**: All worktrees of a repo share a single stash stack (it lives on the common git dir, not per-worktree). Concurrent workspaces running `git stash push` / `pop` will clobber each other's entries. To shelve changes, use a per-worktree WIP commit (`git commit -m WIP`, reset later) or write to a uniquely-named ref (`git stash create` + `git update-ref refs/wip/<name>`).

## Testing

```bash
# Run all tests
cargo test

# Check all diagnostics across a workspace (the primary way to verify diagnostic behavior)
cargo run -- check /path/to/addon --with-stubs
# Filter to a specific file:
cargo run -- check /path/to/addon --with-stubs | grep "FileName.lua"
# Include hints (default is warnings+errors only):
cargo run -- check /path/to/addon --with-stubs --severity hint

# Evaluate a single file with type info
cargo run -- evaluate tests/annotations.lua

# Test hover/definition/signature/diagnostics at line:col
cargo run -- test-query tests/integration_stubs.lua:4:10 --with-stubs

# Test hover/definition/signature/diagnostics against a real addon project
# Use --scan-dir to load the full workspace so cross-file defclass, @builds-field,
# and addon namespace resolution all work. This is slow but necessary for accurate
# results when investigating issues in real addon code.
cargo run -- test-query /path/to/addon/File.lua:LINE:COL --with-stubs --scan-dir /path/to/addon

# Smoke test (build + integration tests)
./tests/smoke.sh
```

### Test file layout
- `tests/integration_test.rs` — Unified test harness with `TestConfig`
- `tests/integration.lua` — Basic hover/def: primitives, functions, scopes, varargs, addon namespace
- `tests/integration_stubs.lua` — Stubs hover/def: external globals, Frame type
- `tests/annotations.lua` — Annotation features: @param, @return, @type, @class, @field, @alias
- `tests/overloads.lua` — Overload resolution (--with-stubs)
- `tests/deep-inheritance.lua` — 5-level class hierarchy (--with-stubs)
- `tests/signature-help.lua` — Signature help with `sig:` assertions (--with-stubs)
- `tests/diagnostics/` — Semantic diagnostics with `diag:` assertions and @diagnostic suppression; `.wowluarc.json` enables `need-check-nil` + `implicit-nil-return`
- `tests/need-check-nil/` — Nil-checking diagnostics with nil-guard narrowing; `.wowluarc.json` enables the default-off `need-check-nil` code
- `tests/access-modifiers/` — Private/protected field access diagnostics; `.wowluarc.json` enables `inference.implicit_protected_prefix`
- `tests/references.lua` — Find references and rename
- `tests/undefined-global.lua` — Undefined global diagnostics (--with-stubs)
- `tests/undefined-field.lua` — Undefined field on @class tables diagnostics
- `tests/undefined-doc-class.lua` — Undefined class names in `@class Foo: Parent` inheritance position
- `tests/undefined-doc-name.lua` — Undefined type names in annotations (`@param`, `@return`, `@type`, `@field`, `@alias`, fun()/inline table shapes)
- `tests/circle-doc-class.lua` — Circular @class inheritance chain diagnostics
- `tests/generics.lua` — Generic type parameters with `@generic`
- `tests/generics-projections.lua` — `params<F>` and `returns<F>` utility-type projections with generic registry pattern
- `tests/generics-projections-e2e.lua` — End-to-end generic registry class exercising `params<F>` and `returns<F>` through class fields and table constructors
- `tests/call-func-generics.lua` — Class type parameter substitution into `@overload` resolution and `returns<F>` projections for callable tables and for-in loops
- `tests/funcall-access.lua` — Dot/colon access on function call return values
- `tests/builder-pattern/` — `@builds-field` and `@return built` builder pattern with edge cases and diagnostics; `.wowluarc.json` enables `need-check-nil`
- `tests/return-overloads.lua` — Tuple-union `@return` (`(A, B) | (C, D)`) sibling narrowing and variadic return expansion (`@return ...T`)
- `tests/tuple-union-returns.lua` — Focused tuple-union coverage: single-tuple shorthand, labels, per-case descriptions, `fun()` and `@alias` propagation, mixing/arity diagnostics
- `tests/cast.lua` — `@cast` (replace/add/remove) and `@as` inline expression type assertions
- `tests/annotation-completion.lua` — Annotation doc-comment completions: tag names, `@param` names, type suggestions
- `tests/string-literal-completion.lua` — String literal completions in `==`/`~=` comparisons against string literal union types: field access, simple variables, method call returns, single-quote, partial typed, nested field access
- `tests/event-hover/` — Event payload hover via `@event` annotation: multi-param line-breaking, single-param inline, empty payload, custom event types; uses `scan_dir` to load event declarations from `events.lua`
- `tests/call-hierarchy.lua` — Call hierarchy queries: `call_hierarchy_item_at` (functions and methods), `outgoing_calls_from_function` (grouped call ranges, nested function exclusion), `call_sites_for_function` (incoming call sites with enclosing function), `enclosing_function_at`, `call_hierarchy_display_name` (method vs function formatting)
- `tests/code-lens.lua` — Code lens assertions via `lens:` field: top-level functions, local functions, class methods (colon syntax), table functions (dot syntax), "N implementations" on `@class` declarations counting direct subclasses, "overrides Parent" on methods overriding parent class methods, multi-level inheritance (grandparent override resolution)
- `tests/type-narrows.lua` — `@type-narrows` custom type guard narrowing (then-branch, early-exit, else-branch, assert, method-style)
- `tests/type-guard.lua` — `type()` guard narrowing for symbols and field chains (`type(x) == "string"`, `type(obj.field) == "table"`, `type(x) ~= "nil"`)
- `tests/literal-bool-ret.lua` — Literal boolean return type union discrimination (`@return true`/`@return false` on union member methods)
- `tests/correlated-locals.lua` — Correlated local variable narrowing: locals assigned in every branch of if/elseif (no else) are narrowed together
- `tests/lateinit/` — `T!` non-nil assertion / lateinit fields: `@field` and `---@type` with `!` suffix; `.wowluarc.json` enables `need-check-nil`
- `tests/count-down-loop.lua` — Numeric for-loop step direction diagnostics (`count-down-loop`)
- `tests/incomplete-signature-doc/` / `tests/incomplete-signature-doc-meta/` — `incomplete-signature-doc` HINT for functions with partial `@param`/`@return` annotations; `-meta` asserts `@meta` files suppress the diagnostic. Each dir has a `.wowluarc.json` enabling the default-off code.
- `tests/stylistic.lua` — Stylistic HINT diagnostics: `empty-block`, `redundant-return`, `trailing-space`
- `tests/not-precedence.lua` — Operator precedence: `not x <cmp> y` parses as `(not x) <cmp> y` (`not-precedence`)
- `tests/syntax-coverage.lua` — Broad syntax construct coverage: numeric literals, long strings, unary/binary operators, repeat/until, for-step, semicolons, no-paren calls, anonymous functions, multi-dot definitions, code-after-break, long bracket comments, forward-declared locals, nested function returns, bracket-keyed tables, multi-target assignment, conditional function defs, higher-order functions, module patterns, closures, reassignment, colon methods
- `tests/convergence.lua` — Fixpoint convergence regression: 60 reverse-order function calls testing inner loop optimization
- `tests/metatable-type-i.lua` — Metatable type inference: `setmetatable()` + `__index` field propagation, chained metatables, self-referential `mt.__index = mt`, factory functions, instance field priority (--with-stubs)
- `tests/infinite-loop.lua` — Infinite loop handling (`while true` / `repeat until false`): only branching returns produce confident non-nilable return types and suppress `missing-return`
- `tests/overlay.lua` — Per-file overlay where fields are added to class-typed local variables (runtime field additions)
- `tests/structural-subtype.lua` — Structural subtyping: table literals assignable to `@class` types when field shapes match
- `tests/accessor-modifiers.lua` — `@accessor` annotation for transparent access modifier fields (private/protected through accessor methods)
- `tests/semantic-tokens.lua` — Semantic-token classification via the `tok:` assertion: function/method/class/namespace/parameter/property/variable tokens with `defaultLibrary`/`deprecated` modifiers (--with-stubs)
- `tests/inlay-hints/` — Inlay hint assertions via `hint:` field: parameter names, variable types, return types, for-loop types, parameter types (disabled by default, enabled in test config), suppression cases (name match, annotated, nil, function RHS, void function, multi-assignment, pairs/ipairs, self param, Any param); `.wowluarc.json` enables all hint categories
- `tests/backward-inference.lua` — Backward param-type inference signals: arithmetic/unary/concat, typed-argument propagation, annotated-param precedence, conflict fallback, overload-aware arity selection (2-arg call must pick the 2-arg `@overload`, not the 3-arg primary)
- `tests/backward-inference-disabled/` — Verifies `inference.backward_param_types: false` in `.wowluarc.json` disables the inference pass
- `tests/correlated-return-inference/` — Synthesized correlated return-only overloads (default-on; explicit `inference.correlated_return_overloads: true`): basic 2-tuple narrowing, 3-tuple, early-exit, skip cases (existing `@return`, single return, mismatched arity, mixed tuples, all-nil only, arity 1)
- `tests/correlated-return-inference-disabled/` — Verifies `inference.correlated_return_overloads: false` disables synthesis: nested-scope returns leave callers with `?`
- `tests/correlated-return-inference-disabled-crossfile/` — Cross-file global function with synthesizable return pattern, verifying workspace-scan synthesis path honors the `correlated_return_overloads: false` flag
- `tests/allowed-globals/` — Allowed globals via `.wowluarc.json` config (`globals.read`/`globals.write`), `create-global` diagnostic, and `SLASH_*` auto-detection (default `allow_slash_commands: true`)
- `tests/slash-commands-disabled/` — Verifies `globals.allow_slash_commands: false` makes `SLASH_*` globals trigger `create-global` and `undefined-global`
- `tests/saved-variables/` — `.toc` file `SavedVariables`/`SavedVariablesPerCharacter` auto-discovered as allowed globals; multiple `.toc` files in one directory
- `tests/unused-vararg/` — `unused-vararg` diagnostic for functions declaring `...` but never referencing it; uses `.wowluarc.json` to enable the default-disabled code
- `tests/unknown-types/` — Strict-typing `unknown-param-type` / `unknown-return-type` / `unknown-local-type` / `unknown-field-type` diagnostics; uses `.wowluarc.json` to enable the four default-disabled codes
- `tests/flavor-filter/` — Flavor filtering via `.wowluarc.json` (`flavors`), `@flavor-narrows` annotation, `WOW_PROJECT_ID` narrowing, TOC-based per-file flavor detection, and the `wrong-flavor-api` diagnostic. One subdirectory per scenario (classic-only, multi-flavor, wow-project-guard, annotation-guard, boolean-guard, boolean-guard-crossfile, invalid-annotation, no-config, suppression, toc-suffix, toc-per-line, toc-intersect, toc-header-restrict).
- `tests/framexml-disabled/` — Verifies `framexml: false` in `.wowluarc.json` disables FrameXML globals while keeping core WoW API globals
- `tests/multi-addon/` — Multi-addon workspace namespace isolation via `addon_root: true` in per-addon `.wowluarc.json`; verifies AddonA sees only its own namespace fields and AddonB sees only its own
- `tests/crossfile/` — Cross-file addon namespace resolution, `@defclass` with parameterized parent classes, `@builds-field` builder chains, `@class`/`@type` field access, `@class` inheritance, `@alias` usage, global functions/variables, access modifier diagnostics, typed self-field inheritance (`self_field_lib.lua`/`self_field_user.lua`), and deep addon-ns chains of 4+ parts with auto-created intermediate sub-tables (`deep_chain_defs.lua`/`deep_chain_user.lua`/`deep_chain_nonroot.lua`)

### Annotation format
Test expectations are embedded as comments below code lines:
```lua
local x = 5
--    ^ hover: x: number  def: local
foo(1, "hello")
--  ^ sig: fun(x: number, y: string): boolean
oldFunc()
-- ^ diag: deprecated
local y = mustUse()
-- ^ diag: none
```
Fields are separated by double-space. Supported fields: `hover:`, `def:`, `sig:`, `diag:`, `refs:`, `comp:`, `tok:`, `hint:`, `lens:`.

The `tok:` field value is the semantic-token classification at the caret: the token type followed by zero or more modifiers in any order (e.g. `tok: function defaultLibrary`, `tok: method deprecated`). Use `tok: none` to assert no token is emitted at the caret.

The `lens:` field value is the expected code lens function name on the code line (e.g. `lens: greet`, `lens: sayHello`). Use `lens: none` to assert no code lens target exists at that line.

The test harness applies `ProjectConfigs::disabled_diagnostics_for()` to filter diagnostics — the same path the LSP server uses in `publish_with_config`. Tests that rely on default-off codes (`need-check-nil`, `implicit-nil-return`, `unused-vararg`, `incomplete-signature-doc`, `unknown-*-type`) must live in a subdirectory with an adjacent `.wowluarc.json` that opts in via `diagnostics.enable`. Existing examples: `tests/need-check-nil/`, `tests/incomplete-signature-doc/`, `tests/unused-vararg/`, `tests/unknown-types/`.

## Stubs
WoW API stubs live in `stubs/`. Scanned at startup by `scan_workspace()` / `scan_stubs_for_test()`. Stubs are precomputed and checked in; they are regenerated by `cargo run -- regenerate-stubs`, which clones [Ketho/vscode-wow-api](https://github.com/Ketho/vscode-wow-api) to a temp directory. Local overrides live in `stubs/overrides/`.

Stub generation (including Classic-only globals from the wiki and BlizzardInterfaceResources) is handled by `src/stub_gen.rs`. Run `cargo run -- regenerate-stubs` to regenerate precomputed stubs. **Any change to `src/stub_gen.rs` or `stubs/overrides/` requires regenerating stubs and committing the updated `stubs/precomputed.bin.zst` and `stubs/precomputed-files.bin.zst`.**

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
