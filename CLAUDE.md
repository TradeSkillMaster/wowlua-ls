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
- `src/annotations.rs` — Annotation parsing (`@param`, `@return`, `@class`, `@field`, `@type`, `@alias`, `@overload`, `@generic`, `@defclass`, `@deprecated`, `@nodiscard`, `@meta`, `@diagnostic`), shared `resolve_annotation_type()` function, `scan_defclass_calls()` for cross-file defclass discovery
- `src/diagnostics/` — Diagnostic types and per-diagnostic modules (see [Diagnostics](#diagnostics) below)
- `src/syntax/syntax.rs` — Lexer/parser using rowan (green tree)
- `src/syntax/lexer.rs` — Tokenization
- `src/syntax/debug.rs` — Debug output utilities for syntax tree
- `src/ast.rs` — AST node definitions and casts (uses `define_ast_node!` macro)
- `src/config.rs` — Project configuration: `.wowluarc.json` loading, ignore patterns, diagnostic overrides
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

To add a new diagnostic: create `src/diagnostics/new_thing.rs` with a `CODE` constant and `check()` function, add `pub mod new_thing;` to `mod.rs`, and call `check()` from the appropriate place in `src/analysis/` (typically `build_ir.rs` for Phase 1 checks or `checks.rs` for deferred checks). Suppression via `@diagnostic disable:new-thing` works automatically by matching the `CODE` string.

### Parameterized classes (`@class Name<S>`)
Classes can declare type parameters: `@class BaseClass<S>`. Fields referencing type params (e.g. `@field __super S`) are stored with `annotation_type_raw` and re-resolved during substitution. The substitution chain:
1. A `@defclass T : P` factory declares `@generic T: BaseClass<P>` — binding class type param `S` to function generic `P`
2. At each call site, `P` resolves to the concrete parent class (e.g. `Animal`)
3. Fields with `annotation_type_raw` are re-resolved with `{S → Animal}`, so `__super` becomes `Animal`

Substitution happens in two places:
- **Per-file**: `prescan.rs:substitute_class_type_params()` for local defclass calls
- **Workspace-wide**: `pre_globals.rs` pass 3b for `scan_defclass_calls()`-discovered classes, using `ClassDecl.constraint_type_arg_subs`

### Dummy SyntaxNodePtr
External symbols don't have real source locations. A minimal `"--"` parse creates a shared dummy node pointer. `definition_at()` returns `DefinitionResult::External(loc)` for these instead of trying to use the dummy node.

## PLAN.md

`PLAN.md` tracks **unimplemented** future work items only. When an item is completed, remove it entirely rather than crossing it out or marking it done.

## Bug fixes

When fixing a bug, always add a regression test covering the fix. Add test assertions to the appropriate existing test file (see test file layout below) using the annotation format (`hover:`, `def:`, `sig:`, `diag:`, etc.). Run `cargo test` to confirm the new test passes.

## Conventions

- Byte offsets are `u32` throughout the IR (not `usize`)
- `SymbolIndex`, `FunctionIndex`, `TableIndex`, `ExprId` are all `usize` type aliases
- Symbol versions track reassignments: `local x = 1; x = "hi"` creates two versions
- External data is immutable after `PreResolvedGlobals::build()`
- `@meta` files suppress all diagnostics (they're declaration-only stubs)

## Testing

```bash
# Run all tests (15 integration tests + parse_samples)
cargo test

# Evaluate a file with type info
cargo run -- evaluate tests/annotations.lua

# Test hover/definition/signature/diagnostics at line:col
cargo run -- test-query tests/integration_stubs.lua:4:10 --with-stubs

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
- `tests/crossfile/` — Cross-file addon namespace resolution and `@defclass` with parameterized parent classes
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
WoW API stubs live in `stubs/vscode-wow-api/Annotations/Core/`. Scanned at startup by `scan_workspace()` / `scan_stubs_for_test()`.

## Profiling

```bash
# Profile against an addon directory (parses + analyzes all .lua files)
cargo run --release -- profile /path/to/addon
```
