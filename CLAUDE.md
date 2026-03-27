# wowlua_ls — WoW Lua Language Server

A Language Server Protocol implementation for Lua (World of Warcraft API dialect). Provides hover, go-to-definition, completion, signature help, find references, rename, and diagnostics.

## Architecture

### Source files
- `src/main.rs` — CLI entry point: `evaluate` subcommand, `test-query` subcommand (hover/def/sig/completions/diagnostics), otherwise starts LSP
- `src/types.rs` — IR type definitions: `ValueType`, `Expr`, `Symbol`, `Scope`, `Function`, `TableInfo`, `FieldInfo`, deferred check structs, index aliases, `EXT_BASE`
- `src/analysis/` — Core per-file analysis engine (`Analysis` struct):
  - `mod.rs` — Struct definition, constructor, two-tier lookups, core helpers
  - `prescan.rs` — Phase 0: class/alias pre-scan, annotation type resolution, generic inference
  - `build_ir.rs` — Phase 1: AST walk, scope/symbol/function/table creation, expression lowering
  - `resolve.rs` — Phase 2: fixpoint type resolution loop, expression resolver
  - `checks.rs` — Deferred diagnostic checks (run after type resolution), class hierarchy helpers
  - `queries.rs` — LSP query methods: hover, definition, completion, signature help, references, rename
- `src/pre_globals.rs` — `PreResolvedGlobals` struct + 5-phase build from WoW API stubs
- `src/annotations.rs` — Annotation parsing (`@param`, `@return`, `@class`, `@field`, `@type`, `@alias`, `@overload`, `@overload return:`, `@generic`, `@defclass`, `@deprecated`, `@nodiscard`, `@meta`, `@diagnostic`, `@cast`, `@as`, `@builds-field`, `@built-name`, `@built-extends`, `@type-narrows`), shared `resolve_annotation_type()` function, `scan_defclass_calls()` for cross-file defclass discovery, `scan_built_name_calls()` for cross-file `@built-name` class registration
- `src/diagnostics/` — Diagnostic types and per-diagnostic modules (see [Diagnostics](#diagnostics) below)
- `src/syntax/syntax.rs` — Lexer/parser using rowan (green tree)
- `src/syntax/lexer.rs` — Tokenization
- `src/syntax/debug.rs` — Debug output utilities for syntax tree
- `src/ast.rs` — AST node definitions and casts (uses `define_ast_node!` macro)
- `src/config.rs` — Project configuration: `.wowluarc.json` loading, ignore patterns, diagnostic overrides, allowed globals
- `src/lsp/main_loop.rs` — LSP server loop, request handlers, `scan_stubs_for_test()`
- `src/lsp/diagnostics.rs` — Diagnostic publishing with `@diagnostic` suppression and project-wide config overrides

### Two-tier index space (EXT_BASE)
External globals (WoW API stubs) use indices >= `EXT_BASE` (1,000,000). Per-file locals use indices < `EXT_BASE`. All lookup functions (`sym()`, `func()`, `table()`, `expr()`) route via `idx >= EXT_BASE` check. This avoids cloning ~9000 external symbols per file.

### Key query functions (in `queries.rs`)
- `find_symbol_at(offset)` — Resolves direct names: gets token at offset → scope lookup → returns `(SymbolIndex, name)`
- `find_field_at(offset)` — Resolves dot/colon chains (`x.y.z`): walks table fields to find the target field's `ExprId`
- `scope_at_offset(offset)` — Finds innermost scope containing offset via `block_scopes` ranges
- `get_symbol(id, scope_idx)` — Walks scope hierarchy upward; at scope 0 also checks `ext.scope0_symbols` (in `analysis/mod.rs`)

### PreResolvedGlobals::build() phases (in `pre_globals.rs`)
Built once at startup, shared via `Arc` across all files:
1. **Register class names** — Create empty `TableInfo` for each `@class`
2. **Populate @field entries** — Resolve annotation types, add to table fields
3. **Build method functions** — Create `Function` entries for methods, add to tables
4. **Resolve inheritance** — Fixpoint loop copying parent fields to children (handles 5+ levels), then substitutes parameterized class type params (e.g. `@class C<S>` with `@field __super S` → `S` replaced with concrete parent)
5. **Build global functions** — Create `Function` + `Symbol` entries, add to `scope0_symbols`
6. **Register non-class tables** — `math`, `string`, `table`, etc.

### Per-file analysis phases (in `src/analysis/`)
1. **Phase 0: prescan_classes_and_aliases** — Import external classes/aliases, scan local `@class`/`@alias` declarations
2. **Phase 1: build_ir** — Walk AST, create scopes/symbols/functions/tables, lower expressions to `Expr` IR
3. **Phase 2: resolve_types** — Fixpoint loop resolving expressions until no progress

### Identifier prefix dispatch (in `build_ir.rs`)
The `Expression::Identifier` handler has a multi-branch dispatch based on what child node appears as the prefix of a dotted/chained identifier. The cases are checked in order:
1. **GroupedExpression** — `(expr).field`: lower grouped expr as base, chain fields
2. **FunctionCall** — `func().field`: lower call as base, chain fields (special-cases `select(2, ...)` for addon namespace)
3. **child Identifier** — `t[expr].field`: recursive lower, handle bracket indexing, chain fields
4. **Name token** — `x.y.z`: symbol lookup on first name, chain remaining as field accesses

When new expression forms can appear as Identifier prefixes, a new branch must be added here or field access tokens will fall through to the Name path and be misidentified as globals.

### Diagnostics
Each diagnostic lives in its own module under `src/diagnostics/`:
- `mod.rs` — `WowDiagnostic` struct + submodule declarations
- `deprecated.rs` — `CODE` + `check()` for deprecated symbol usage
- `discard_returns.rs` — `CODE` + `check()` for ignored `@nodiscard` return values
- `access.rs` — `CODE_PRIVATE`/`CODE_PROTECTED` + `check()` for visibility violations
- `type_mismatch.rs` — `CODE` + `check()` for argument type mismatches against `@param`
- `return_mismatch.rs` — `CODE` + `check()` for return type mismatches against `@return`
- `field_type_mismatch.rs` — `CODE` + `check()` for field assignment type mismatches against `@field`
- `duplicate_index.rs` — `CODE` + `check()` for duplicate keys in table constructors
- `redundant_param.rs` — `CODE` + `check()` for extra arguments in function calls
- `missing_param.rs` — `CODE` + `check()` for missing required arguments in function calls
- `undefined_global.rs` — `CODE` + `check()` for references to unresolved global names
- `undefined_field.rs` — `CODE` + `check()` for accessing nonexistent fields on `@class` tables
- `unused_local.rs` — `CODE` + `check()` for unreferenced local variables (HINT severity)
- `redefined_local.rs` — `CODE` + `check()` for same-scope local variable redefinition
- `assign_type_mismatch.rs` — `CODE` + `check()` for reassignment type mismatches against `@type`
- `missing_return_value.rs` — `CODE` + `check()` for return statements with fewer values than `@return`
- `implicit_nil_return.rs` — `CODE` + `check()` for bare `return` in functions with all-optional `@return` types (HINT severity)
- `missing_return.rs` — `CODE` + `check()` for functions missing return statements
- `unreachable_code.rs` — `CODE` + `check()` for code after return (HINT severity)
- `code_after_break.rs` — `CODE` + `check()` for code after break (HINT severity)
- `inject_field.rs` — `CODE` + `check()` for setting undeclared fields on `@class` tables (HINT severity)
- `need_check_nil.rs` — `CODE` + `check()` for field/method access on possibly-nil values (WARNING severity)
- `undefined_doc_param.rs` — `CODE` + `check()` for `@param` name not matching function parameters
- `duplicate_doc_param.rs` — `CODE` + `check()` for duplicate `@param` annotations
- `duplicate_doc_field.rs` — `CODE` + `check()` for duplicate `@field` annotations
- `unknown_diag_code.rs` — `CODE` + `check()` for unknown code in `@diagnostic` directives
- `redundant_return_value.rs` — `CODE` + `check()` for returning more values than `@return` declares
- `redundant_value.rs` — `CODE` + `check()` for extra values in assignments
- `unbalanced_assignments.rs` — `CODE` + `check()` for more variables than values in assignments
- `duplicate_set_field.rs` — `CODE` + `check()` for setting a field already set on `@class` tables
- `unused_function.rs` — `CODE` + `check()` for unused function definitions (HINT severity)
- `undefined_doc_class.rs` — `CODE` + `check()` for references to undefined class names in annotations
- `missing_fields.rs` — `CODE` + `check()` for missing required fields when constructing `@class` tables (WARNING severity)
- `malformed_annotation.rs` — `CODE` + `check()` for unknown or incomplete `---@` annotations
- `circle_doc_class.rs` — `CODE` + `check()` for circular `@class` inheritance chains
- `grouped_return_mismatch.rs` — `CODE` + `check()` for return values not matching any return-only overload (WARNING severity)
- `builds_field_not_self.rs` — `CODE` + `check()` for `@builds-field` methods that use `@return ClassName` instead of `@return self` (WARNING severity)
- `return_self_class_name.rs` — `CODE` + `check()` for methods that use `@return ClassName` instead of `@return self` (HINT severity)
- `create_global.rs` — `CODE` + `check()` for implicit global creation via assignment or function definition (HINT severity)

To add a new diagnostic: create `src/diagnostics/new_thing.rs` with a `CODE` constant and `check()` function, add `pub mod new_thing;` to `mod.rs`, and call `check()` from the appropriate place in `src/analysis/` (typically `build_ir.rs` for Phase 1 checks or `checks.rs` for deferred checks). Suppression via `@diagnostic disable:new-thing` works automatically by matching the `CODE` string. **Also add the diagnostic to the table in `README.md`.**

### Parameterized classes (`@class Name<S>`)
Classes can declare type parameters: `@class BaseClass<S>`. Fields referencing type params (e.g. `@field __super S`) are stored with `annotation_type_raw` and re-resolved during substitution. The substitution chain:
1. A `@defclass T : P` factory declares `@generic T: BaseClass<P>` — binding class type param `S` to function generic `P`
2. At each call site, `P` resolves to the concrete parent class (e.g. `Animal`)
3. Fields with `annotation_type_raw` are re-resolved with `{S → Animal}`, so `__super` becomes `Animal`

Substitution happens in two places:
- **Per-file**: `prescan.rs:substitute_class_type_params()` for local defclass calls
- **Workspace-wide**: `pre_globals.rs` pass 3b for `scan_defclass_calls()`-discovered classes, using `ClassDecl.constraint_type_arg_subs`

### Builder pattern (`@builds-field` + `@return built`)
Builder methods use `@builds-field <param_idx> <type>` with `@return self` to progressively add typed fields to a shadow `built_table` on `TableInfo`. `@return built [: Parent]` returns the accumulated type.

Resolution in `resolve.rs`:
- **`@builds-field` + `@return self`**: `clone_table_with_built_field()` clones the receiver table with an updated `built_table` containing the new field. Each chained call produces a new table clone.
- **`@return built`**: Returns the `built_table` from the receiver. If `@return built : Parent` is specified, the parent class is added to the built table's `parent_classes`.

Key fields: `Function.builds_field: Option<(usize, ValueType)>`, `Function.built_name: Option<usize>`, `Function.built_extends: bool`, `Function.returns_built: bool`, `Function.returns_built_parent: Option<String>`, `TableInfo.built_table: Option<TableIndex>`.

#### Naming built types (`@built-name`)
`@built-name <param_idx>` on the chain entry point function sets the `built_table`'s `class_name` from the string literal at parameter `param_idx`. This allows the built type to be referenced by name in `@param`/`@type` annotations.

Resolution in `resolve.rs`:
- `clone_table_with_built_name()` creates a built table with the specified class name and registers it in `ir.classes`
- Subsequent `clone_table_with_built_field()` calls preserve the name and re-register the latest built table in `ir.classes`
- A post-fixpoint step re-resolves param annotations that reference newly discovered `@built-name` classes

Cross-file visibility: `scan_built_name_calls()` in `annotations.rs` scans workspace files for calls to `@built-name` functions, extracting class names and registering them as empty `ClassDecl` entries in `PreResolvedGlobals`.

#### Extending builder schemas (`@built-extends`)
`@built-extends` on a method (combined with `@built-name`) makes the new built type inherit from the receiver's current built type instead of copying its fields. This supports schema extension patterns where a base schema is defined and subclasses extend it:

```lua
---@param name string
---@built-name 1
---@built-extends
---@return self
function Schema:Extend(name)
    return self
end

local BASE = Schema:AddString("baseName"):Commit()
local CHILD = BASE:Extend("ChildState"):AddString("childField"):Commit()
-- ChildState inherits baseName from BASE's built type
```

Resolution in `resolve.rs`:
- `clone_table_with_built_name()` with `extends=true` creates a new built table whose `parent_classes` include the receiver's existing built table plus all its ancestors (flattened for single-level FieldAccess resolution)
- Subsequent `clone_table_with_built_field()` calls preserve the parent chain, so fields added after `Extend` still inherit from the base
- Multi-level extension works: grandchild → child → base, with all ancestor fields accessible

### Return-only overloads (`@overload return:`)
`@overload return:` on `OverloadSig`/`ResolvedOverload` (distinguished by `is_return_only: true`) enables multi-return sibling narrowing at call sites.

**Implementation**: `multi_return_siblings` in `Analysis` tracks which symbols came from the same function call. `narrow_siblings()` in `build_ir.rs` hooks into all narrowing points (`analyze_nil_guard`, `analyze_early_exit_guard`, assert narrowing). It checks `has_return_only_overloads_from_siblings()` to only activate for functions with `is_return_only` overloads. Return-only overloads are filtered out of arg-count matching in `resolve.rs`.

**Callee enforcement**: The `grouped-return-mismatch` diagnostic (deferred check in `checks.rs`) verifies that each `return` statement matches one of the return-only overloads. The `missing-return-value` diagnostic is suppressed for functions with a nil return-only overload.

### Dummy SyntaxNodePtr
External symbols don't have real source locations. A minimal `"--"` parse creates a shared dummy node pointer. `definition_at()` returns `DefinitionResult::External(loc)` for these instead of trying to use the dummy node.

### `self` parameter handling (cross-cutting invariant)
A parameter named `self` can be **implicit** (colon syntax: `function Foo:bar(x)` → parser sees `[x]`, self injected by `insert_function_definition`) or **explicit** (dot/global: `function handler(self, index)` → parser sees `[self, index]`). Three code paths must agree on this distinction:
1. **Stub scanning** (`annotations.rs:scan_file_globals`) — Only filter `self` from unannotated param lists when `is_call_to_self()` (colon syntax). Global functions with explicit `self` must keep it.
2. **Function building** (`build_ir.rs:insert_function_definition`) — `inject_self` adds a synthetic self param; `dot_defined = !inject_self` records which style was used.
3. **Call-site `self_offset`** (`resolve.rs`) — Only offset when `is_method_call` (colon call) AND the function has a self-like first param. Plain calls pass all args explicitly, so offset must be 0 regardless of the param name.

### Implicit protected for `_`-prefixed names
Data fields starting with `_` are implicitly `Protected` when no explicit visibility annotation is present. This does **not** apply to methods — only data fields. The helper `default_visibility_for_name()` in `annotations.rs` centralizes this logic. It is called from:
- `@field` annotation parsing (when no explicit `private`/`protected`/`public` keyword)
- Runtime field assignment fallback in `build_ir.rs`
- Table constructor fields in `build_ir.rs`
- Deep field injection and builder fields in `resolve.rs`
- All FieldInfo construction sites in `pre_globals.rs` and `prescan.rs`
Explicit annotations always take precedence: `@field public _foo type` stays public, `@field private _foo type` stays private.

## PLAN.md

`PLAN.md` tracks **unimplemented** future work items only. When an item is completed, remove it entirely rather than crossing it out or marking it done.

## README.md

`README.md` is the user-facing documentation. Keep it in sync when adding new features, annotations, or diagnostics. CLAUDE.md is for developer/AI-facing architecture notes only — do not put user-facing documentation here.

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
- `T!` (non-nil assertion / lateinit) — `AnnotationType::NonNil(Box<inner>)` wraps the inner type. Resolves to the inner type with nil stripped. On `@field` or `---@type`, sets `FieldInfo.lateinit = true`, which suppresses `field-type-mismatch` for nil assignments and ensures the field's resolved type is non-nil (no `need-check-nil` on access). Hover shows `T!`.
- **Never special-case specific functions** (e.g. `tinsert`, `table.insert`) in the LS engine code. Behavior differences should be expressed through stub annotations (`@generic`, `@overload`, etc.) so the general type system handles them.
- **Zero warnings policy**: Always run `cargo build` after completing changes and ensure there are zero warnings before considering work done.

## Testing

```bash
# Run all tests (15 integration tests + parse_samples)
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
- `tests/diagnostics.lua` — Semantic diagnostics with `diag:` assertions and @diagnostic suppression
- `tests/need-check-nil.lua` — Nil-checking diagnostics with nil-guard narrowing
- `tests/access-modifiers.lua` — Private/protected field access diagnostics (--with-stubs)
- `tests/references.lua` — Find references and rename
- `tests/undefined-global.lua` — Undefined global diagnostics (--with-stubs)
- `tests/undefined-field.lua` — Undefined field on @class tables diagnostics
- `tests/undefined-doc-class.lua` — Undefined class names in annotations
- `tests/circle-doc-class.lua` — Circular @class inheritance chain diagnostics
- `tests/generics.lua` — Generic type parameters with `@generic`
- `tests/funcall-access.lua` — Dot/colon access on function call return values
- `tests/builder-pattern.lua` — `@builds-field` and `@return built` builder pattern with edge cases and diagnostics
- `tests/return-overloads.lua` — Return-only overloads (`@overload return:`) and sibling narrowing
- `tests/cast.lua` — `@cast` (replace/add/remove) and `@as` inline expression type assertions
- `tests/annotation-completion.lua` — Annotation doc-comment completions: tag names, `@param` names, type suggestions
- `tests/type-narrows.lua` — `@type-narrows` custom type guard narrowing (then-branch, early-exit, else-branch)
- `tests/lateinit.lua` — `T!` non-nil assertion / lateinit fields: `@field` and `---@type` with `!` suffix
- `tests/allowed-globals/` — Allowed globals via `.wowluarc.json` config (`globals.read`/`globals.write`) and `create-global` diagnostic
- `tests/crossfile/` — Cross-file addon namespace resolution, `@defclass` with parameterized parent classes, and `@builds-field` builder chains
- `tests/samples/` — Parse stress tests (real-world Lua files, third-party libraries, syntax errors)

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
Fields are separated by double-space. Supported fields: `hover:`, `def:`, `sig:`, `diag:`.

## Stubs
WoW API stubs live in `stubs/vscode-wow-api/Annotations/Core/`. Scanned at startup by `scan_workspace()` / `scan_stubs_for_test()`. **The `stubs/vscode-wow-api` directory is a git submodule — never modify files in it directly.** If stub changes are needed, they must be made upstream in the submodule's own repository.

**Do not modify the `stubs/vscode-wow-api/` submodule.** Classic-only globals that are missing from the upstream stubs live in `stubs/classic/ClassicGlobals.lua`. This file is **auto-generated** — do not edit it by hand. Instead, run `python3 generate_classic_stubs.py --include-undocumented` from the repo root to regenerate it from the wiki and BlizzardInterfaceResources.

## Profiling

```bash
# Profile against an addon directory (parses + analyzes all .lua files)
cargo run --release -- profile /path/to/addon
```
