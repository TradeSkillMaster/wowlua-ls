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
  - `semantic_tokens.rs` — LSP semantic-token classification: walks every `Name`/`Parameter` token, resolves via `resolve_field_chain_at` then `find_symbol_at`, and maps to `(token_type, modifiers)`. Legend is the `SEMANTIC_TOKEN_TYPES` / `SEMANTIC_TOKEN_MODIFIERS` arrays. Encoded into LSP wire format by `main_loop.rs::encode_semantic_tokens`. `defaultLibrary` modifier marks anything with `idx >= EXT_BASE`.
- `src/pre_globals.rs` — `PreResolvedGlobals` struct + 5-phase build from WoW API stubs
- `src/annotations.rs` — Annotation parsing (`@param`, `@return`, `@class`, `@field`, `@type`, `@alias`, `@overload`, `@overload return:`, `@generic`, `@defclass`, `@deprecated`, `@nodiscard`, `@meta`, `@diagnostic`, `@cast`, `@as`, `@builds-field`, `@built-name`, `@built-extends`, `@type-narrows`, `@correlated`), shared `resolve_annotation_type()` function, `scan_defclass_calls()` for cross-file defclass discovery, `scan_built_name_calls()` for cross-file `@built-name` class registration, `scan_method_typed_self_fields()` for cross-file typed `self.field` discovery
- `src/diagnostics/` — Diagnostic types and per-diagnostic modules (see [Diagnostics](#diagnostics) below)
- `src/syntax/parser.rs` — Recursive descent + Pratt parser producing arena-based `SyntaxTree`
- `src/syntax/tree.rs` — Arena-based syntax tree: `SyntaxTree`, `Node`, `Token`, `NodeId`, `TokenId`, `TreeBuilder` with checkpoint support; also high-level API wrappers (`SyntaxNode`, `SyntaxToken`, `TextRange`, `TextSize`, `TokenAtOffset`, `NodeOrToken`)
- `src/syntax/syntax_kind.rs` — `SyntaxKind` enum (unified token + node kinds)
- `src/syntax/lexer.rs` — Tokenization
- `src/ast.rs` — AST node definitions and casts over `SyntaxNode` (uses `define_ast_node!` macro)
- `src/config.rs` — Project configuration: `.wowluarc.json` loading, ignore patterns, diagnostic overrides, allowed globals
- `src/stub_gen.rs` — Stub generation: fetches WoW API stubs, Classic globals from wiki/BlizzardInterfaceResources, and serializes precomputed `PreResolvedGlobals` blob (replaces former Python scripts)
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

### Workspace scanning passes (in `main_loop.rs:scan_paths_with_overrides`)
Run before `PreResolvedGlobals::build()` to collect classes, aliases, and globals from all files:
1. **Pass 1** — `scan_all_annotations()` + `scan_file_globals()` per file: collect `@class`/`@alias` declarations and top-level function/variable globals
2. **Pass 2** — `scan_defclass_calls()` per file: discover classes from `@defclass` factory calls, extract constructor self-fields
3. **Pass 3** — `scan_built_name_calls()` per file: discover `@built-name` classes, merge with `@class` overlays
4. **Pass 4** — `scan_method_typed_self_fields()` per file: scan colon-method bodies for `self.field = expr ---@type Type` assignments on known classes. Captures both preceding-line and inline `---@type` annotations. Per-field file paths stored in `ClassDecl.field_paths` for cross-file definition locations.

### Per-file analysis phases (in `src/analysis/`)
1. **Phase 0: prescan_classes_and_aliases** — Import external classes/aliases, scan local `@class`/`@alias` declarations
2. **Phase 1: build_ir** — Walk AST, create scopes/symbols/functions/tables, lower expressions to `Expr` IR
3. **Phase 2: resolve_types** — Fixpoint loop resolving expressions until no progress

### Metatable type inference (`setmetatable` + `__index`)
`setmetatable(tbl, mt)` is detected during Phase 2 resolution via `setmetatable_func_idx` stored on `PreResolvedGlobals`. When detected, `resolve_setmetatable()` in `resolve.rs`:
1. Resolves arg[0] (the table) and arg[1] (the metatable)
2. Looks up `__index` on the metatable via `resolve_metatable_index_field()`
3. Mutates the table in-place, setting `metatable_index` to the resolved `__index` target, `metatable` to the raw metatable, and `call_func` from `__call` if present

Field lookups (`get_field` in `mod.rs`) check three levels: direct fields → `parent_classes` → `metatable_index` chain. The `get_field_via_metatable()` helper walks the chain with `HashSet<TableIndex>` cycle detection, supporting chained metatables (e.g. `inst → Child → Base`).

`getmetatable(obj)` is detected via `getmetatable_func_idx` and returns `obj.metatable` (the raw metatable table).

Operator metamethods (`__add`, `__sub`, `__mul`, `__div`, `__mod`, `__pow`, `__concat`, `__unm`, `__len`) are resolved via `resolve_metamethod_return()` in `resolve.rs`. The function checks the table's `metatable` first, then the table itself (for `@class` tables with metamethods as direct fields). The metamethod function's `@return` annotation determines the operator's result type.

Key fields: `TableInfo.metatable_index: Option<TableIndex>`, `TableInfo.metatable: Option<TableIndex>`, `PreResolvedGlobals.setmetatable_func_idx: Option<FunctionIndex>`.

Class name propagation from `setmetatable()` uses three sources (in priority order):
1. `__index` as a direct table reference with `class_name` (e.g. `{ __index = MyClass }`)
2. The metatable itself having `class_name` (e.g. `---@class Foo \n local MT = { __index = function ... }`)
3. `__index` as a function whose return expressions access a class-typed table (e.g. `__index = function(self, key) if METHODS[key] then return METHODS[key] end end` where METHODS has `@class`). Detected by `find_index_function_class_delegate()` in `resolve.rs`, which scans the function's ret symbols for BracketIndex/FieldAccess on class tables.

**Limitations**: `setmetatable` mutates the table in-place — this means field assignments on a `setmetatable`-created table after the call ARE visible, but the metatable won't be set on external tables (idx >= EXT_BASE).

### Expression lowering — split identifier nodes (in `build_ir.rs`)
The parser produces distinct node kinds for identifier access patterns instead of a single `Identifier` catch-all. The `Expression::Identifier` handler dispatches on node kind:
1. **NameRef** → `lower_name_ref()`: simple symbol lookup with type narrowing
2. **DotAccess** → `lower_dot_access()`: lower base expression, create `FieldAccess`
3. **BracketAccess** → `lower_bracket_access()`: lower base and key, create `BracketIndex`
4. **MethodCall** → `lower_method_call_as_callee()`: fully lower the base (including nested calls), then create `FieldAccess` for the method name. This is called when a MethodCall is used as a callee inside `lower_function_call`.

For chained method calls like `obj:A("x"):B("y")`, the parser nests MethodCall nodes. Each level's base is lowered as a complete FunctionCall before the next method name is resolved. Long chains (≥50 links) use `lower_function_call_chain()` for iterative processing to avoid stack overflow.

A legacy 4-way dispatch for old-style flat `Identifier` nodes is retained below the new handlers but is no longer exercised by the current parser.

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
- `duplicate_doc_alias.rs` — `CODE` + `check()` for duplicate `@alias` declarations
- `unknown_diag_code.rs` — `CODE` + `check()` for unknown code in `@diagnostic` directives
- `redundant_return_value.rs` — `CODE` + `check()` for returning more values than `@return` declares
- `redundant_value.rs` — `CODE` + `check()` for extra values in assignments
- `unbalanced_assignments.rs` — `CODE` + `check()` for more variables than values in assignments
- `duplicate_set_field.rs` — `CODE` + `check()` for setting a field already set on `@class` tables
- `unused_function.rs` — `CODE` + `check()` for unused function definitions (HINT severity)
- `undefined_doc_class.rs` — `CODE` + `check()` for references to undefined class names in `@class Foo: Parent` inheritance position
- `undefined_doc_name.rs` — `CODE` + `check()` for references to undefined type names in annotations (`@param`, `@return`, `@type`, `@field`, `@alias`, etc.)
- `missing_fields.rs` — `CODE` + `check()` for missing required fields when constructing `@class` tables (WARNING severity)
- `malformed_annotation.rs` — `CODE` + `check()` for unknown or incomplete `---@` annotations
- `circle_doc_class.rs` — `CODE` + `check()` for circular `@class` inheritance chains
- `grouped_return_mismatch.rs` — `CODE` + `check()` for return values not matching any return-only overload (WARNING severity)
- `builds_field_not_self.rs` — `CODE` + `check()` for `@builds-field` methods that use `@return ClassName` instead of `@return self` (WARNING severity)
- `return_self_class_name.rs` — `CODE` + `check()` for methods that use `@return ClassName` instead of `@return self` (HINT severity)
- `create_global.rs` — `CODE` + `check()` for implicit global creation via assignment or function definition (HINT severity)
- `duplicate_constructor.rs` — `CODE` + `check()` for multiple `@constructor` annotations on a single class (WARNING severity)
- `constructor_return.rs` — `CODE` + `check()` for `@constructor` methods with return annotations other than `@return self` (WARNING severity)
- `count_down_loop.rs` — `CODE` + `check()` for numeric for-loops with step direction not matching start/end values (WARNING severity)
- `unused_vararg.rs` — `CODE` + `check()` for functions declaring `...` but never referencing it in their body (HINT severity, default-disabled)

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

Key fields: `Function.builds_field: Option<(usize, ValueType, bool)>` (param_index, resolved_type, lateinit), `Function.built_name: Option<usize>`, `Function.built_extends: bool`, `Function.returns_built: bool`, `Function.returns_built_parent: Option<String>`, `TableInfo.built_table: Option<TableIndex>`.

The type in `@builds-field` supports `T!` (NonNil/lateinit): `@builds-field 1 T!` creates fields with `FieldInfo.lateinit = true`, allowing nil assignment without `field-type-mismatch`. The `!` is detected at three sites: `build_ir.rs` (per-file), `pre_globals.rs` build function resolution (cross-file `ClassDecl.fields`), and `pre_globals.rs` `build_on_stubs` (workspace overlay).

#### Naming built types (`@built-name`)
`@built-name <param_idx>` on the chain entry point function sets the `built_table`'s `class_name` from the string literal at parameter `param_idx`. This allows the built type to be referenced by name in `@param`/`@type` annotations.

Resolution in `resolve.rs`:
- `clone_table_with_built_name()` creates a built table with the specified class name and registers it in `ir.classes`
- Subsequent `clone_table_with_built_field()` calls preserve the name and re-register the latest built table in `ir.classes`
- A post-fixpoint step re-resolves param annotations that reference newly discovered `@built-name` classes

Cross-file visibility: `scan_built_name_calls()` in `annotations.rs` scans workspace files for calls to `@built-name` functions, extracting class names and registering them as empty `ClassDecl` entries in `PreResolvedGlobals`.

#### `@class` overlays on `@built-name` types
A `@class Foo` declaration that re-uses a name already created via `@built-name` merges its `@field` annotations with the builder-pattern fields. Overlay `@field` types take precedence over built field types for matching names. The overlay must be standalone (not directly preceding a `local` statement, which would be interpreted as typing the variable).

Resolution happens at three levels:
- **Per-file** (`resolve.rs`): `clone_table_with_built_name()` checks `ir.classes` for a local `@class` table with the same name and merges its `@field` annotations (identified by `annotation_type_raw.is_some()`) into the built table. `clone_table_with_built_field()` skips overwriting fields that have `annotation_type_raw` (from overlays).
- **Per-file prescan** (`prescan.rs`): After populating local class fields, external class fields (from `ext.classes`) are imported into local `@class` overlay tables for any matching names.
- **Workspace** (`main_loop.rs`): When merging `ws_file_defclasses` (from `scan_built_name_calls()`) into `ws_classes`, built-name fields are merged into existing `@class` entries instead of being skipped.

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

**Implementation**: `multi_return_siblings` in `Analysis` tracks which symbols came from the same function call. `narrow_siblings()` in `build_ir.rs` hooks into all narrowing points (`analyze_nil_guard`, `analyze_early_exit_guard`, assert narrowing). It checks `check_return_only_overloads_from_siblings()` to only activate for functions with `is_return_only` overloads. Return-only overloads are filtered out of arg-count matching in `resolve.rs`.

**Overload-based narrowing**: When a sibling is narrowed, `narrow_siblings()` creates `Expr::OverloadNarrow` versions for ALL other siblings. The OverloadNarrow stores `(ret_index, func_expr, narrowed)` where `narrowed` is a list of `(sibling_ret_index, NarrowKind)` entries for each directly-guarded sibling. `NarrowKind` has four variants: `StripNil` (e.g. `x ~= nil`), `StripFalsy` (e.g. `if x then`), `StripTruthy` (e.g. `if not x then` or `else` of `if x then`), and `ClassEq(String)` (e.g. `if x == Foo.MEMBER then` where `Foo.MEMBER` is class-typed). During resolve, `resolve_overload_narrow()` filters return-only overloads whose type at each narrowed sibling's position is compatible with the `NarrowKind` (`overload_type_survives_{strip_nil,strip_falsy,strip_truthy}` / `overload_type_matches_class`), then computes the union of types at `ret_index` across compatible overloads. Overload-narrowed siblings are NOT added to `narrowed_symbols` to avoid double-stripping nil in `narrow_type_for_display`. For cross-file calls (deferred case), the narrowed_info is stored in `deferred_sibling_narrowings` and processed during the resolve fixpoint loop. `push_overload_narrow_version()` uses `version_for_scope_ancestors_only` for the base version so that a narrowing created inside a sibling branch scope can't become the base of an outer-scope narrowing.

**Class-equality narrowing (`x == Foo.MEMBER`)**: Detected during `analyze_nil_guard` by `record_class_eq_deferral()` when the non-symbol side is a pure identifier chain (a bare `Identifier` / `DotAccess` — see *Expression lowering — split identifier nodes* above). The RHS is lowered and queued in `deferred_class_eq_narrowings` as `(sym_idx, expr_id, scope_idx)`. At resolve, `resolve_deferred_class_eq_narrowings()` resolves the RHS: if its type is (or contains) a class table with a `class_name`, it sets `class_narrowed_symbols[scope][sym] = class_name`, inserts into `type_filtered_symbols` (for symbol-level display), pushes a `TypeFilter` version, and propagates to multi-return siblings via `push_overload_narrow_version` with `NarrowKind::ClassEq(name)`. To reach references lowered before resolve, `rewrite_sym_refs_in_subtree()` walks `sym_ref_sites` (a reverse index `SymbolIndex → Vec<(ExprId, token_offset)>` populated at build time) and redirects `Expr::SymbolRef` exprs in the affected scope subtree to the new version, updates `symbol_version_at`, invalidates the `resolved_expr_cache`, and prunes stale `need-check-nil` / `type-mismatch` diagnostics emitted pre-narrowing. Restricting detection to pure identifier chains avoids re-lowering embedded name references (e.g. `name` inside `strlower(name)`) which would clobber the original `symbol_version_at` entries.

**Narrowing tracking maps (convention)**: Each map's name describes what the guard *stripped* to produce the narrowing, not what the value is. `narrowed_symbols` = nil stripped; `falsy_narrowed_symbols` = nil AND false stripped (a subset of `narrowed_symbols`); `truthy_narrowed_symbols` = truthy stripped, so the value is `nil | false`; `class_narrowed_symbols` = equated to a class (value IS that class). So "truthy_narrowed" reads as "truthy-ness stripped" → value is falsy.

**Callee enforcement**: The `grouped-return-mismatch` diagnostic (deferred check in `checks.rs`) verifies that each `return` statement matches one of the return-only overloads. The `missing-return-value` diagnostic is suppressed for functions with a nil return-only overload.

### Literal boolean return type union discrimination
When a union type `A | B` has a method where `A:Method()` is annotated `@return false` and `B:Method()` is annotated `@return true`, the LS automatically narrows the union in conditional branches — then-branch keeps the `true`-returning types, else-branch keeps the `false`-returning types.

**Implementation** (`build_ir.rs`):
- `resolve_expr_to_tables()` — like `resolve_expr_to_table()` but returns ALL table indices from a union type
- `extract_bool_discriminator()` — given a method call on a union receiver, checks if all union member tables define the method with complementary literal boolean `@return` annotations. Returns `(sym_idx, true_type, false_type)`.
- Integrated into `analyze_nil_guard` (then + else branches), `analyze_early_exit_guard`, and `narrow_assert_expr`

**Conditions**: all union members must define the method, every return annotation must be literal `true` or `false` (not generic `boolean`), and at least one of each must exist. Works with 3+ member unions.

### Correlated nil fields (`@correlated`)
`@correlated field1, field2, ...` on a `@class` declares that listed optional fields are always nil/non-nil together. Stored as `correlated_groups: Vec<Vec<String>>` on `TableInfo`. Multiple `@correlated` lines per class create independent groups. Groups are inherited by child classes during prescan pass 3.

**Narrowing**: In `try_narrow_field()` and `try_narrow_field_falsy()` (build_ir.rs), after inserting the primary narrowing, `narrow_correlated_fields()` resolves the field's table via `resolve_field_chain_table()`, looks up its `correlated_groups`, and inserts sibling narrowings into `narrowed_fields` (and `falsy_narrowed_fields` if applicable). Works for both `self.field` (chain len 1) and `self.sub.field` (chain len 2+) patterns, and with early-exit narrowing.

### Correlated locals (inferred from if/elseif branches)
When multiple local variables are assigned in every explicit branch of an if/elseif chain (without else), they form a correlation group. Stored as `correlated_locals: Vec<Vec<SymbolIndex>>` on `Analysis`. When one member is narrowed via a nil guard or early-exit guard, all siblings in the group are narrowed too.

**Detection**: In the `PendingBranchMerge` processing (build_ir.rs), after collecting symbols assigned in branch scopes, symbols that are assigned (not just narrowed) in ALL explicit branches of a `has_implicit_else=true` merge are collected into a correlation group.

**Narrowing**: `narrow_correlated_locals()` in build_ir.rs is called from `narrow_symbol_strip_nil()`, `narrow_symbol_strip_falsy()`, and direct narrowing insertion points in `analyze_nil_guard()`, `analyze_early_exit_guard()`, and `narrow_assert_expr()`. It looks up the symbol in `correlated_locals` groups and inserts sibling narrowings into `narrowed_symbols` (and `falsy_narrowed_symbols` if applicable).

### DefNode (source location pointers)
Symbol and function definitions store `DefNode { start: u32, end: u32 }` — a simple byte range with no dependency on the syntax tree. External symbols use `DefNode::DUMMY`. `definition_at()` returns `DefinitionResult::External(loc)` for external symbols instead of trying to look up the node.

### `self` parameter handling (cross-cutting invariant)
A parameter named `self` can be **implicit** (colon syntax: `function Foo:bar(x)` → parser sees `[x]`, self injected by `insert_function_definition`) or **explicit** (dot/global: `function handler(self, index)` → parser sees `[self, index]`). Three code paths must agree on this distinction:
1. **Stub scanning** (`annotations.rs:scan_file_globals`) — Only filter `self` from unannotated param lists when `is_call_to_self()` (colon syntax). Global functions with explicit `self` must keep it.
2. **Function building** (`build_ir.rs:insert_function_definition`) — `inject_self` adds a synthetic self param for colon-defined methods.
3. **Call-site `self_offset`** (`resolve.rs`) — Offset by 1 when `is_method_call` (colon call) AND the function has any first param (whether named `self` or not, including stored function fields). Plain calls pass all args explicitly, so offset must be 0 regardless of the param name.

### Implicit protected for `_`-prefixed names
Runtime-discovered data fields starting with `_` are implicitly `Protected` when no explicit visibility annotation is present. This does **not** apply to explicit `@field` declarations — those default to `Public` since the author had the opportunity to write `@field protected`. This does **not** apply to methods — only data fields. The helper `default_visibility_for_name()` in `annotations.rs` centralizes the implicit protected logic. It is called from:
- Table constructor fields in `build_ir.rs`
- All FieldInfo construction sites in `pre_globals.rs` and `prescan.rs`
- `self._foo` assignments inside class methods (the class is defining its own field)
`@field` annotation parsing does **not** call `default_visibility_for_name()` — explicit declarations always use `Public` as the default, with `@field protected`/`@field private` for explicit restriction.
Runtime field assignments from outside the class (in `build_ir.rs` and `resolve.rs`) use `Visibility::Public` — ad-hoc injected fields should not get implicit protected since there is no `@field` declaration asserting protection.

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
- `T & U` (intersection type) — `AnnotationType::Intersection(Vec<AnnotationType>)` / `ValueType::Intersection(Vec<ValueType>)`. Parsed via `&` with higher precedence than `|` (split `|` first, then `&` inside each union member). An intersection is assignable to X if ANY member is; X is assignable to an intersection if assignable to ALL members. Field access checks all member tables. Used by `CreateFrame` stub to combine frame type with template mixin (`T & Tp`).
- `T!` (non-nil assertion / lateinit) — `AnnotationType::NonNil(Box<inner>)` wraps the inner type. Resolves to the inner type with nil stripped. On `@field` or `---@type`, sets `FieldInfo.lateinit = true`, which suppresses `field-type-mismatch` for nil assignments and ensures the field's resolved type is non-nil (no `need-check-nil` on access). Hover shows `T!`.
- `{field: type, ...}` (anonymous table shape) — `AnnotationType::TableLiteral(Vec<(String, AnnotationType)>)`. Parsed in `parse_type()` when the string starts with `{` and ends with `}`, splitting on `,` at top level and then `field: type` pairs. Resolves via `materialize_table_literal()` in `prescan.rs` which creates a `TableInfo` with the specified fields. Supports optional fields (`field?: type`) which become `Union(type, nil)`. Works in `@param`, `@return`, `@type`, `@alias`, and inside intersections (`T & {field: type}`).
- `...T` (variadic return) — `AnnotationType::VarArgs(Box<AnnotationType>)`. When `@return ...T` is the last return annotation, it fills all remaining return slots with type `T`. Stored as `Function.has_vararg_return: bool`. The vararg portion is optional (no `missing-return-value` for it) and `redundant-return-value` is suppressed. Multiple returns must use separate `@return` lines (comma-separated multi-return on a single `@return` line is not supported).
- `@alias Foo<K,V> V[]` (parameterized alias) — `AliasDecl` has `type_params: Vec<String>`. Stored in `ir.parameterized_aliases` / `ext.parameterized_aliases` as `(Vec<String>, AnnotationType)` (type params + raw body). When `Parameterized("Foo", args)` is encountered during resolution, `substitute_alias_type_params()` replaces type param names in the body with concrete args, then the substituted type is resolved normally. Supports colon syntax (`@alias Foo<K,V>: V[]`) and spaces in type params (`@alias Foo<K, V>`).
- **Never special-case specific functions** (e.g. `tinsert`, `table.insert`) in the LS engine code. Behavior differences should be expressed through stub annotations (`@generic`, `@overload`, etc.) so the general type system handles them.
- **Zero warnings policy**: Always run `cargo build` after completing changes and ensure there are zero warnings before considering work done.
- **No real addon code in source**: Never use code from real addons (e.g. TradeSkillMaster) in source comments, test names, or examples. Always generalize to fictional/generic examples.

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
- `tests/undefined-doc-class.lua` — Undefined class names in `@class Foo: Parent` inheritance position
- `tests/undefined-doc-name.lua` — Undefined type names in annotations (`@param`, `@return`, `@type`, `@field`, `@alias`, fun()/inline table shapes)
- `tests/circle-doc-class.lua` — Circular @class inheritance chain diagnostics
- `tests/generics.lua` — Generic type parameters with `@generic`
- `tests/funcall-access.lua` — Dot/colon access on function call return values
- `tests/builder-pattern.lua` — `@builds-field` and `@return built` builder pattern with edge cases and diagnostics
- `tests/return-overloads.lua` — Return-only overloads (`@overload return:`), sibling narrowing, and variadic return expansion (`@return ...T`)
- `tests/cast.lua` — `@cast` (replace/add/remove) and `@as` inline expression type assertions
- `tests/annotation-completion.lua` — Annotation doc-comment completions: tag names, `@param` names, type suggestions
- `tests/type-narrows.lua` — `@type-narrows` custom type guard narrowing (then-branch, early-exit, else-branch, assert, method-style)
- `tests/type-guard.lua` — `type()` guard narrowing for symbols and field chains (`type(x) == "string"`, `type(obj.field) == "table"`, `type(x) ~= "nil"`)
- `tests/literal-bool-ret.lua` — Literal boolean return type union discrimination (`@return true`/`@return false` on union member methods)
- `tests/correlated-locals.lua` — Correlated local variable narrowing: locals assigned in every branch of if/elseif (no else) are narrowed together
- `tests/lateinit.lua` — `T!` non-nil assertion / lateinit fields: `@field` and `---@type` with `!` suffix
- `tests/count-down-loop.lua` — Numeric for-loop step direction diagnostics (`count-down-loop`)
- `tests/syntax-coverage.lua` — Under-tested syntax constructs: hex/scientific/float literals, long strings, unary operators, repeat/until, for-step, semicolons, no-paren calls, anonymous functions, multi-dot definitions, code-after-break, long bracket comments
- `tests/convergence.lua` — Fixpoint convergence regression: 60 reverse-order function calls testing inner loop optimization
- `tests/metatable-type-i.lua` — Metatable type inference: `setmetatable()` + `__index` field propagation, chained metatables, self-referential `mt.__index = mt`, factory functions, instance field priority (--with-stubs)
- `tests/semantic-tokens.lua` — Semantic-token classification via the `tok:` assertion: function/method/class/namespace/parameter/property/variable tokens with `defaultLibrary`/`deprecated` modifiers (--with-stubs)
- `tests/allowed-globals/` — Allowed globals via `.wowluarc.json` config (`globals.read`/`globals.write`) and `create-global` diagnostic
- `tests/unused-vararg/` — `unused-vararg` diagnostic for functions declaring `...` but never referencing it; uses `.wowluarc.json` to enable the default-disabled code
- `tests/crossfile/` — Cross-file addon namespace resolution, `@defclass` with parameterized parent classes, `@builds-field` builder chains, `@class`/`@type` field access, `@class` inheritance, `@alias` usage, global functions/variables, access modifier diagnostics, and typed self-field inheritance (`self_field_lib.lua`/`self_field_user.lua`)
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
Fields are separated by double-space. Supported fields: `hover:`, `def:`, `sig:`, `diag:`, `refs:`, `comp:`, `tok:`.

The `tok:` field value is the semantic-token classification at the caret: the token type followed by zero or more modifiers in any order (e.g. `tok: function defaultLibrary`, `tok: method deprecated`). Use `tok: none` to assert no token is emitted at the caret.

## Stubs
WoW API stubs live in `stubs/`. Scanned at startup by `scan_workspace()` / `scan_stubs_for_test()`. Stubs are precomputed and checked in; they are regenerated by `cargo run -- regenerate-stubs`, which clones [Ketho/vscode-wow-api](https://github.com/Ketho/vscode-wow-api) to a temp directory. Local overrides live in `stubs/overrides/`.

Stub generation (including Classic-only globals from the wiki and BlizzardInterfaceResources) is handled by `src/stub_gen.rs`. Run `cargo run -- regenerate-stubs` to regenerate precomputed stubs. **Any change to `src/stub_gen.rs` or `stubs/overrides/` requires regenerating stubs and committing the updated `stubs/precomputed.bin.zst` and `stubs/precomputed-files.bin.zst`.**

## Profiling

```bash
# Profile against an addon directory (parses + analyzes all .lua files)
cargo run --release -- profile /path/to/addon
```

## VS Code Extension Development

When using `/vscode`, check whether VS Code already has a window open for the target folder **before** launching. If it does, stop and ask the user to close it — VS Code reuses the existing window and silently ignores the new `--extensionDevelopmentPath`, so the dev build won't load. The `--new-window` flag does not reliably fix this. Warning the user *after* launching is too late; the wrong instance is already foregrounded.
