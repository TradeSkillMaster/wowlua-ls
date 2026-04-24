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
  - `semantic_tokens.rs` — LSP semantic-token classification. Narrow by design: walks only bare `Name` tokens (skips field/method access and parameters) and emits a `function` token when the symbol resolves to a function value. Everything else is left to the editor's built-in Lua grammar, so coloring matches pre-feature behavior. Modifiers: `defaultLibrary` for stub symbols (via `is_stub_symbol()` — `idx - EXT_BASE < stub_symbols_end`, boundary captured at `load_precomputed_stubs()` time), `deprecated` when the resolved function is `@deprecated`. Legend is the `SEMANTIC_TOKEN_TYPES` / `SEMANTIC_TOKEN_MODIFIERS` arrays; encoded into LSP wire format by `main_loop.rs::encode_semantic_tokens`.
- `src/pre_globals.rs` — `PreResolvedGlobals` struct + 5-phase build from WoW API stubs
- `src/annotations.rs` — Annotation parsing (`@param`, `@return`, `@class`, `@field`, `@type`, `@alias`, `@overload`, `@generic`, `@defclass`, `@deprecated`, `@nodiscard`, `@meta`, `@diagnostic`, `@cast`, `@as`, `@builds-field`, `@built-name`, `@built-extends`, `@type-narrows`, `@correlated`, `@see`, `@flavor-narrows`), tuple-union `@return` syntax (`(A name, B) | (C, D)`) producing `AnnotationType::Tuple`, shared `resolve_annotation_type()` function, `scan_defclass_calls()` for cross-file defclass discovery, `scan_built_name_calls()` for cross-file `@built-name` class registration, `scan_method_typed_self_fields()` for cross-file typed `self.field` discovery
- `src/flavor.rs` — 3-flavor bitmask (retail/classic/classic_era matching WoW's install-folder names), `from_ketho_mask()` that collapses Ketho's 4-bit (mainline/mists/bcc/classic_era) into ours (mists and bcc both map to classic), name parsing, and narrowing helpers for `wrong-flavor-api`
- `src/diagnostics/` — Diagnostic types and per-diagnostic modules (see [Diagnostics](#diagnostics) below)
- `src/syntax/parser.rs` — Recursive descent + Pratt parser producing arena-based `SyntaxTree`
- `src/syntax/tree.rs` — Arena-based syntax tree: `SyntaxTree`, `Node`, `Token`, `NodeId`, `TokenId`, `TreeBuilder` with checkpoint support; also high-level API wrappers (`SyntaxNode`, `SyntaxToken`, `TextRange`, `TextSize`, `TokenAtOffset`, `NodeOrToken`)
- `src/syntax/syntax_kind.rs` — `SyntaxKind` enum (unified token + node kinds)
- `src/syntax/lexer.rs` — Tokenization
- `src/ast.rs` — AST node definitions and casts over `SyntaxNode` (uses `define_ast_node!` macro)
- `src/config.rs` — Project configuration: `.wowluarc.json` loading, ignore patterns, diagnostic overrides, allowed globals, `inference.backward_param_types`, `inference.correlated_return_overloads`
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

### Cross-file find-references / rename
`references_at(offset)` runs against a single tree. For workspace-wide search, the LSP handler (`lsp/main_loop.rs::find_references_across_workspace`) composes three queries:
1. `AnalysisResult::reference_target_at(offset)` returns a `ReferenceTarget` (either `Symbol { idx, name }` or `Field { table_idx, field_name }`). An index `>= EXT_BASE` is stable across every `AnalysisResult` built from the same `PreResolvedGlobals`.
2. `AnalysisResult::promote_to_cross_file(&target)` lifts a file-local symbol or table to its workspace-wide counterpart when one exists (the defining file keeps a shadowing scope-0 local for its own global functions and a local `@class` table for its own `@class` declarations — both are swapped out for the `EXT_BASE+` idx when searching elsewhere).
3. `AnalysisResult::references_for_target(tree, &target, include_declaration, strict_shadow)` runs the search over an arbitrary tree against an externally-resolved target, enabling the LSP handler to iterate every open document and every scanned workspace file (rayon-parallel, gated by a `text.contains(target.name())` prefilter).

Consumer → defining-file matching works because the `Symbol` arm of `references_for_target` also accepts a scope-0 local whose name is in `ext.scope0_symbols` when the target is external; the `Field` arm accepts a local `@class` table whose `class_name` maps to the external `table_idx`.

The shadow-acceptance rule permissively matches any scope-0 local with the same name — including a truly-local `local X = 5` in a file that also has a workspace-wide `X` — which is desirable for find-references (the user wants to see the collision) but destructive for rename. The `strict_shadow` flag on `references_for_target` filters shadows whose first-version def-node sits inside a `local` statement (detected via `is_local_declaration_site`, which walks up to a `LocalAssignStatement` or a `FunctionDefinition` with a `LocalKeyword` child). The rename handler passes `strict_shadow=true`; find-refs passes `false`.

`include_declaration=false` drops the name-token range inside the first-version def-node for both the local target and any accepted shadow local. `def_name_token_range` translates the statement-level `DefNode` to the name-token range first, since `DefNode` ranges cover whole statements (e.g. the entire `function X() end`).

`textDocument/rename` is built on top of the same helper (prepare_rename + aggregated references with `strict_shadow=true`), so rename is workspace-wide but safer than find-refs against same-named file-locals.

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
- `grouped_return_mismatch.rs` — `CODE` + `check()` for return values not matching any tuple-union `@return` case (WARNING severity)
- `builds_field_not_self.rs` — `CODE` + `check()` for `@builds-field` methods that use `@return ClassName` instead of `@return self` (WARNING severity)
- `return_self_class_name.rs` — `CODE` + `check()` for methods that use `@return ClassName` instead of `@return self` (HINT severity)
- `create_global.rs` — `CODE` + `check()` for implicit global creation via assignment or function definition (HINT severity)
- `duplicate_constructor.rs` — `CODE` + `check()` for multiple `@constructor` annotations on a single class (WARNING severity)
- `constructor_return.rs` — `CODE` + `check()` for `@constructor` methods with return annotations other than `@return self` (WARNING severity)
- `count_down_loop.rs` — `CODE` + `check()` for numeric for-loops with step direction not matching start/end values (WARNING severity)
- `unused_vararg.rs` — `CODE` + `check()` for functions declaring `...` but never referencing it in their body (HINT severity, default-disabled)
- `incomplete_signature_doc.rs` — `CODE` + `check_missing_param()`/`check_missing_return()` for functions with partial `@param`/`@return` annotations — some params or return undocumented (HINT severity)
- `empty_block.rs` — `CODE` + `check()` for empty `if`/`elseif`/`else`/`while`/`for`/`repeat` bodies (HINT severity)
- `redundant_return.rs` — `CODE` + `check()` for bare `return` as the final statement of a function's top block (HINT severity)
- `trailing_space.rs` — `CODE` + `check()` for lines ending with whitespace; text-level scan invoked from `Analysis::new_with_tree` (HINT severity)
- `not_precedence.rs` — `CODE` + `check()` for `not x <cmp> y` parsing as `(not x) <cmp> y` because `not` binds tighter than comparison operators (HINT severity)
- `wrong_flavor_api.rs` — `CODE` + `check()` for calls to APIs not available in all project-declared flavors (WARNING severity). Only fires when the project declares `flavors` in `.wowluarc.json`.
- `unknown_param_type.rs` / `unknown_return_type.rs` / `unknown_local_type.rs` / `unknown_field_type.rs` — `CODE` + `check()` for sites whose type couldn't be inferred (HINT severity, default-disabled). See [Unknown-type diagnostics (strict typing)](#unknown-type-diagnostics-strict-typing) below.

To add a new diagnostic: create `src/diagnostics/new_thing.rs` with a `CODE` constant and `check()` function, add `pub mod new_thing;` to `mod.rs`, and call `check()` from the appropriate place in `src/analysis/` (typically `build_ir.rs` for Phase 1 checks or `checks.rs` for deferred checks). Suppression via `@diagnostic disable:new-thing` works automatically by matching the `CODE` string. **Also add the diagnostic to the table in `README.md`.**

### Parameterized classes (`@class Name<S>`)
Classes can declare type parameters: `@class BaseClass<S>`. Fields referencing type params (e.g. `@field __super S`) are stored with `annotation_type_raw` and re-resolved during substitution. The substitution chain:
1. A `@defclass T : P` factory declares `@generic T: BaseClass<P>` — binding class type param `S` to function generic `P`
2. At each call site, `P` resolves to the concrete parent class (e.g. `Animal`)
3. Fields with `annotation_type_raw` are re-resolved with `{S → Animal}`, so `__super` becomes `Animal`

Substitution happens in two places:
- **Per-file**: `prescan.rs:substitute_class_type_params()` for local defclass calls
- **Workspace-wide**: `pre_globals.rs` pass 3b for `scan_defclass_calls()`-discovered classes, using `ClassDecl.constraint_type_arg_subs`

### Generic argument inference (call-site `@generic T` binding)
Binding `@generic T` from call-site arguments happens in three layers in `resolve.rs` around `resolve_function_call`:

1. **Direct param types** (lines ~1459–1520): if the param's `resolved_type` is `TypeVariable(T)`, bind T to the arg type. If it's `Union(..., TypeVariable(T), ...)` (optional params, or explicit unions), extract the TypeVariable alternative and bind. Strip nil first so optional args don't pollute T.
2. **Structural inference** via `prescan.rs:infer_generics_from_annotation` (called at line ~1524): walks the raw `AnnotationType` to handle:
   - `T[]` — mine T from the arg's array element type
   - `table<K,V>` — mine V from table values, K = string
   - `` `T` `` (backtick) — resolve a string literal arg as a class name
   - `Fun(_, returns, _)` — if a return annotation is `Simple(T)`, extract T from the arg. The arg can be a function (use its first `@return`; fall back to `FunctionRet.resolved_type`, then `type_source`) or a named `@class` table (callable as constructor — T is the class itself). Plain non-class tables are excluded so `{}` literals don't silently bind T.
   - `Union(members)` — recurse into every member (no short-circuit), so multi-generic params like `(fun(): T) | U` can bind T from the Fun member AND U from the Simple member in one pass. Bare `Simple(T)` members bind T directly to the arg type.
   - `Simple(T)` when T is a generic — bind directly.
   - `NonNil(inner)` — recurse.
3. **Receiver `type_args`** (runs BEFORE the per-arg loop, around `resolve.rs:1534-1556`): for method calls whose `@param self Class<T>` is `Parameterized`, look up the receiver's `type_args` via `get_expr_type_args` and bind T from there. Runs first so class-generic `T` is bound from the explicit `---@type Class<X>` annotation before direct-arg binding can clobber it with the (rarely useful) arg's runtime type. Receiver-bound generics also join `substitutable_generic_names` so the type-mismatch loop at `resolve.rs:~1920` substitutes them.

**`substitutable_generic_names`** (previously `structural_generic_names`) is the set of generics whose binding is trusted enough to substitute into sibling param types for the type-mismatch check. Populated from structural inference (`T[]`, `table<K,V>`, `fun(): T`), direct-TypeVariable-param inference, and receiver-binding. Explicitly NOT populated from promotional patterns (`` `T` `` backtick, `@defclass T`) where the bound value intentionally differs from the arg.

**`(fun(): T) | T` pre-emption** (lines ~1493–1510): when the raw annotation is a union containing a `Fun(..)` member, run structural inference *before* the eager Union-direct-bind. Otherwise the direct-bind would pick the `TypeVariable(T)` alternative and bind T to the arg itself (e.g. `T = Function(_)` when the user passes a callable), never giving the `Fun` member a chance.

### Function-type projections (`params<F>` / `returns<F>`)
Utility-type projections referencing the shape of a generic `F` bound to a `fun(...)` type. Declared in source as `AnnotationType::Parameterized("params" | "returns", [Simple(name)])` and stored on `Function` as per-slot overlays (`return_projections: HashMap<usize, ProjectionKind>` + `vararg_projection: Option<ProjectionKind>`), NOT as new `ValueType` variants. The `ProjectionKind` enum (`src/types.rs`) has `Params(String)` and `Return(String)` variants naming the referenced generic.

**Validation** at `prescan.rs::check_annotation_type_names` in the `Parameterized(base, args)` arm: `base == "params" || base == "returns"` requires exactly 1 arg of `Simple(name)` where name is a declared `@generic`. Violations emit `malformed-annotation`. `params<F>` outside the vararg slot (positional `@param x params<F>`, or `@return params<F>`) emits `malformed-annotation` during `insert_function_definition`. Nested projections (`returns<returns<F>>`) fail the `Simple` shape check.

**Population** (`build_ir.rs::insert_function_definition`):
- In the `@param ...` vararg branch, `match_projection` detects `params<F>` / `returns<F>` and sets `func.vararg_projection`.
- In the `@return` loop (legacy multi-line branch), each return slot that matches `returns<F>` gets `func.return_projections.insert(ret_index, Return(name))`.

**Resolver-level placeholder** (`prescan.rs::resolve_annotation_type_mut_gen`): when resolving a projection annotation with F still bound as an unresolved generic, returns `ValueType::Any` so the return/vararg slot exists in the IR. Call-site resolution replaces it with F's concrete type.

**Call-site resolution** (`resolve.rs::resolve_function_call`):
- `projected_f_idx` is computed early (before the per-arg loop) by looking up F from the receiver's type_args. Used by the arity check AND the per-arg type-mismatch loop.
- Arity check (`resolve.rs:~1340`): when `projected_arity` is non-None, `expected_count = non_vararg_count + F.args.len()`; `effective_is_vararg = false`. Missing-param name uses F's arg name at the out-of-range position.
- Type-mismatch loop (`resolve.rs:~1900`): for vararg positions (`i >= non_vararg_count`), pull expected type from `F.args[i - non_vararg_count].resolved_type`.
- Return resolution (`resolve.rs:~2040`): when `return_projections[ret_index]` is `Return(name)` and `generic_subs[name]` is `Function(Some(f_idx))`, return `f.return_annotations[0]`. If F has multiple return annotations OR the function has tuple-union overloads, emit `multi-return-projection` warning (column 0 is still picked).

**Diagnostics**:
- `malformed-annotation` — shape errors (wrong arity, wrong arg kind, wrong position, nested projection, unknown generic).
- `multi-return-projection` (WARNING, `src/diagnostics/multi_return_projection.rs`) — `returns<F>` truncates when F has >1 return annotation. Suppressible via `@diagnostic disable:multi-return-projection`.

**Hover** (`queries.rs::format_function_decl`): class-declaration hover shows the raw `params<F>` / `returns<F>` via the existing `param_annotation_text` path (no special expansion). Call-site hover on the receiver's call expression already reflects the bound F's concrete return type via the normal resolve path. Signature help at call sites shows `func: F` unsubstituted — further expansion is a v2 enhancement.

### Carrying `type_args` from parameterized return types (`@return Pool<T>`)
When a generic function's return annotation is `Parameterized("Pool", [Simple("T")])`, the call's inferred T has to survive through the assignment so that subsequent method calls on the receiver (e.g. `pool:Get()`) can bind T from the receiver's type_args.

`ValueType::Table(Option<TableIndex>)` doesn't carry type_args, so we keep them outside the value:
- `Function.return_annotations_raw: Vec<AnnotationType>` — preserves the raw `Parameterized(..)` structure alongside the resolved `return_annotations: Vec<ValueType>` (populated in `build_ir.rs`, `prescan.rs`, and `pre_globals.rs`; `#[serde(default)]` for backward compatibility).
- `Analysis.call_type_args: HashMap<ExprId, Vec<ValueType>>` — per-call cache of substituted type_args. Populated in `resolve_function_call` whenever `generic_subs` is non-empty and the raw first-return annotation is `Parameterized`. The type_args are resolved using the function's own `generic_constraints_raw` so that `Simple("T")` becomes `TypeVariable("T")`, then `substitute_generics_deep` substitutes to concrete types.

`get_expr_type_args` (in `resolve.rs`) checks this cache:
1. Direct cache hit for the ExprId (covers `FunctionCall` receivers)
2. `SymbolRef(sym, ver)` — first check the version's `type_args` (set by `---@type Pool<Concrete>` in build_ir), then follow `type_source` ExprId into the cache
3. `FieldAccess { table, field }` — check the field's `annotation_type_raw`, then the field's stored `expr` in the cache (covers `private = { pool = New(...) }` table-field patterns)

Bump `pre_globals.rs::BLOB_VERSION` when changing any field on a serialized type (`Function`, `ClassDecl`, etc.).

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

### Tuple-union `@return` syntax
`@return (A name, B) | (C, D) desc` lowers to a `Function` with:
- `return_annotations` = per-position column union (each position's type = union of that position across all cases)
- `return_annotations_raw` = per-position raw `AnnotationType` column (often a `Union`)
- `return_labels` = names from the first tuple's positions (parallel to column types)
- `overloads` += one `ResolvedOverload { is_return_only: true, description }` per case

Parser: `AnnotationType::Tuple(Vec<TuplePosition>, Option<String>)` represents a single tuple with optional per-case description. A union of tuples is the correlated form. `parse_return_line()` in `annotations.rs` is the line-level entry point — it detects the tuple form by `s.starts_with('(')` + top-level comma inside, then captures any trailing text after `)` as the case description (with optional `@` prefix). `---|` continuation lines merge into the last `@return` line when that line parsed as tuple-form; otherwise they extend an active `@alias`. Tuple-form aliases (`@alias Foo (A) | (B)`) are stored in `IrBuilder.tuple_form_aliases` / `PreResolvedGlobals.tuple_form_aliases` rather than `aliases` (tuples have no single `ValueType`); use-site expansion via `expand_tuple_form_alias()` happens in `build_ir.rs`'s `@return` processing.

The actual lowering work (cases → column-union `ValueType`/raw + labels + return-only `ResolvedOverload`s) lives in the shared `lower_tuple_form_cases()` helper in `annotations.rs`, which takes a resolver closure so every caller can plug in its own type-resolution context. Four call sites invoke it: `build_ir.rs::insert_function_definition` (local functions), `prescan.rs::materialize_fun_type` (inline `fun()` types), `pre_globals.rs::build_function` (external stubs + cross-file workspace scan), and `pre_globals.rs::materialize_fun_type` (stub `fun()` types). Each site is responsible only for the surrounding scope setup — symbol insertion for return slots, overload merging, and IR-specific index adjustments.

Legacy `@return T name` parsing is still accepted; mixing it with a tuple-form `@return` on the same function emits `malformed-annotation`. `@overload return:` parsing was removed.

Single-position tuple cases (`(T)`) are supported in two contexts: (a) `---|` continuation lines, where we know we're extending a tuple union — controlled by the `force_tuple` flag on `parse_return_line`; and (b) base `@return (T)` lines with *no* trailing text (the trailing check preserves the legacy `@return (string|number) name` form, which would otherwise lose its name to case-description parsing). Outside `@return`, `(T)` still parses as plain grouping in `parse_type`.

Arity mismatch across tuple-union cases is allowed. Shorter cases are implicitly nil-padded at missing positions — `lower_tuple_form_cases` uses `max` arity across cases (not first-case arity), computes per-column types by padding with `nil`, and picks labels from the first case that provides a name at each column. Each case's return-only overload keeps its declared arity (no padding); the overload-narrowing lookup (`resolve_overload_narrow`) and `return_overload_may_nil` handle out-of-bounds positions by returning implicit nil, or by falling through to the last declared position's type when `ResolvedOverload.has_vararg_tail` is set (case ended in `...T`). Tuple cases with a `...T` tail also set `Function.has_vararg_return`, same as legacy `@return ...T` — this is detected in all three lowering paths (`build_ir.rs`, `prescan.rs::materialize_fun_type`, `pre_globals.rs::build_function`).

Hover rendering (`queries.rs::format_function_decl`) shows labels inline (`-> name: type, level: type`) and renders return-only overloads as a `cases:` table under the primary signature instead of stacking each as a separate `function name()` block.

**Narrowing implementation** (unchanged from the old `@overload return:`): `multi_return_siblings` in `Analysis` tracks which symbols came from the same function call. `narrow_siblings()` in `build_ir.rs` hooks into all narrowing points (`analyze_nil_guard`, `analyze_early_exit_guard`, assert narrowing). It checks `check_return_only_overloads_from_siblings()` to only activate for functions with `is_return_only` overloads. Return-only overloads are filtered out of arg-count matching in `resolve.rs`.

**Overload-based narrowing**: When a sibling is narrowed, `narrow_siblings()` creates `Expr::OverloadNarrow` versions for ALL other siblings. The OverloadNarrow stores `(ret_index, func_expr, narrowed)` where `narrowed` is a list of `(sibling_ret_index, NarrowKind)` entries for each directly-guarded sibling. `NarrowKind` has four variants: `StripNil` (e.g. `x ~= nil`), `StripFalsy` (e.g. `if x then`), `StripTruthy` (e.g. `if not x then` or `else` of `if x then`), and `ClassEq(String)` (e.g. `if x == Foo.MEMBER then` where `Foo.MEMBER` is class-typed). During resolve, `resolve_overload_narrow()` filters return-only overloads whose type at each narrowed sibling's position is compatible with the `NarrowKind` (`overload_type_survives_{strip_nil,strip_falsy,strip_truthy}` / `overload_type_matches_class`), then computes the union of types at `ret_index` across compatible overloads. Overload-narrowed siblings are NOT added to `narrowed_symbols` to avoid double-stripping nil in `narrow_type_for_display`. For cross-file calls (deferred case), the narrowed_info is stored in `deferred_sibling_narrowings` and processed during the resolve fixpoint loop. `push_overload_narrow_version()` uses `version_for_scope_ancestors_only` for the base version so that a narrowing created inside a sibling branch scope can't become the base of an outer-scope narrowing.

**Class-equality narrowing (`x == Foo.MEMBER`)**: Detected during `analyze_nil_guard` by `record_class_eq_deferral()` when the non-symbol side is a pure identifier chain (a bare `Identifier` / `DotAccess` — see *Expression lowering — split identifier nodes* above). The RHS is lowered and queued in `deferred_class_eq_narrowings` as `(sym_idx, expr_id, scope_idx)`. At resolve, `resolve_deferred_class_eq_narrowings()` resolves the RHS: if its type is (or contains) a class table with a `class_name`, it sets `class_narrowed_symbols[scope][sym] = class_name`, inserts into `type_filtered_symbols` (for symbol-level display), pushes a `TypeFilter` version, and propagates to multi-return siblings via `push_overload_narrow_version` with `NarrowKind::ClassEq(name)`. To reach references lowered before resolve, `rewrite_sym_refs_in_subtree()` walks `sym_ref_sites` (a reverse index `SymbolIndex → Vec<(ExprId, token_offset)>` populated at build time) and redirects `Expr::SymbolRef` exprs in the affected scope subtree to the new version, updates `symbol_version_at`, invalidates the `resolved_expr_cache`, and prunes stale `need-check-nil` / `type-mismatch` diagnostics emitted pre-narrowing. Restricting detection to pure identifier chains avoids re-lowering embedded name references (e.g. `name` inside `strlower(name)`) which would clobber the original `symbol_version_at` entries.

**Narrowing tracking maps (convention)**: Each map's name describes what the guard *stripped* to produce the narrowing, not what the value is. `narrowed_symbols` = nil stripped; `falsy_narrowed_symbols` = nil AND false stripped (a subset of `narrowed_symbols`); `truthy_narrowed_symbols` = truthy stripped, so the value is `nil | false`; `class_narrowed_symbols` = equated to a class (value IS that class). So "truthy_narrowed" reads as "truthy-ness stripped" → value is falsy.

**Temporary insert/restore protocol for `and`/`or` RHS**: `analyze_nil_guard` inserts into the tracking maps at a branch scope (then/else), so entries disappear naturally once the branch scope is out of view. The short-circuit `and`/`or` lowering in `build_ir.rs::BinaryExpression` operates in the *same* scope as the containing expression, so it uses a temporary-insert-then-remove protocol: (1) record what was inserted (`Vec<(SymbolIndex, bool, bool)>` flags whether each map actually took a new entry); (2) call whatever consumes the maps (`narrow_siblings`, etc.); (3) after RHS lowering, remove *only* the entries you added. Sibling `OverloadNarrow` versions pushed during this window are also scope-persistent — pair each narrow call with a pre-narrow version snapshot, then call `ir.push_alias_version(sym, pre_ver, scope)` at teardown to revert the symbol's current version to the pre-`and` state. Any future code that inserts into these maps mid-RHS must follow the same pattern or the cleanup will under- or over-remove.

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

### `x = x or y` coalesce narrowing
The idiom `x = x or y` makes `x` non-nil whenever `y` is non-nil: either the old `x` was truthy (kept) or `y` is used (and `y` non-nil means the result is non-nil). Narrowing is one-directional — narrowing `y` narrows `x`, but narrowing `x` tells you nothing about `y`. Stored as `or_coalesce_derivations: HashMap<SymbolIndex, Vec<SymbolIndex>>` (source `y` → derived `x`s).

**Detection**: `maybe_register_or_coalesce()` runs at every simple-name assignment `x = expr` in `build_ir.rs`. When `expr` is `BinaryOp(Or, NameRef(x), NameRef(y))` and both sides resolve to symbols (with the LHS matching the target), it registers `(y, x)`. Any other assignment to `x` invalidates prior `(*, x)` entries — the coalesce relationship is tied to this specific assignment.

**Narrowing**: Propagated from `narrow_symbol_strip_nil()` / `narrow_symbol_strip_falsy()` via `narrow_or_coalesce_derived()`, from the direct-insert narrowing sites in `analyze_nil_guard_inner` (then-branch `if x then` / `if x ~= nil then` / `if type(x) ~= "nil" then` / `if type(x) == "T" then`) and `narrow_assert_expr` (`assert(x ~= nil)`, `assert(type(x) ...)`), and from the temporary `and`/`or`-guard narrowings in `lower_expression_inner`'s `BinaryExpression` arm (lines near `coalesce_pre_narrow`). Guard-path propagation pushes a transient StripNil/StripFalsy version on each derived symbol alongside the primary/extra guard narrowings, then restores them in the same reverse-order pass that restores the primary guard.

### Flavor filtering (`flavors` config + `@flavor-narrows` + `wrong-flavor-api`)
Projects declare target WoW flavors in `.wowluarc.json` via `flavors: [...]` (accepting `retail`, `classic`, `classic_era` — the three WoW install-folder names). Each `Function` carries a `flavors: u8` (the 3-bit mask `crate::flavor`) and a `flavor_guard: u8` (from the `@flavor-narrows` annotation).

Stub gen: `src/stub_gen.rs::parse_flavor_ts` reads Ketho's `flavor.ts` (4-bit mainline/mists/bcc/classic_era mask) and passes each entry through `crate::flavor::from_ketho_mask` to collapse mists+bcc into our `classic` bit. `apply_flavor_data` writes the translated mask into each matching `ExternalGlobal.flavors`, and `PreResolvedGlobals::build_function` pipes it through to `Function.flavors`.

Narrowing: `Analysis` carries `project_flavors: u8` and `scope_flavors: HashMap<ScopeIndex, u8>`. `try_flavor_narrow()` in build_ir.rs detects `WOW_PROJECT_ID == WOW_PROJECT_*` comparisons and `@flavor-narrows` guard calls, calling `narrow_scope_flavors()` or `exclude_scope_flavors()` on the target scope. `active_flavors_at(scope)` walks ancestor scopes to the first explicit override; if none, returns `project_flavors`.

Because annotation guards on local functions aren't typed at build-ir time, `flavor_guard_mask_for_call` uses `find_function_def(type_source)` to walk the symbol's `type_source` to a `FunctionDef` directly (bypassing `resolved_type`, which is only populated in Phase 2).

Diagnostic: `resolve.rs` emits `wrong-flavor-api` at the call site when `unsupported_flavors(active, call.flavors)` is non-zero. Fires only when `project_flavors != 0` and the function has non-zero `flavors` (a mask of 0 is treated as "available everywhere").

### DefNode (source location pointers)
Symbol and function definitions store `DefNode { start: u32, end: u32 }` — a simple byte range with no dependency on the syntax tree. External symbols use `DefNode::DUMMY`. `definition_at()` returns `DefinitionResult::External(loc)` for external symbols instead of trying to look up the node.

### `self` parameter handling (cross-cutting invariant)
A parameter named `self` can be **implicit** (colon syntax: `function Foo:bar(x)` → parser sees `[x]`, self injected by `insert_function_definition`) or **explicit** (dot/global: `function handler(self, index)` → parser sees `[self, index]`). Three code paths must agree on this distinction:
1. **Stub scanning** (`annotations.rs:scan_file_globals`) — Only filter `self` from unannotated param lists when `is_call_to_self()` (colon syntax). Global functions with explicit `self` must keep it.
2. **Function building** (`build_ir.rs:insert_function_definition`) — `inject_self` adds a synthetic self param for colon-defined methods.
3. **Call-site `self_offset`** (`resolve.rs`) — Offset by 1 when `is_method_call` (colon call) AND the function has any first param (whether named `self` or not, including stored function fields). Plain calls pass all args explicitly, so offset must be 0 regardless of the param name.

### Backward param-type inference
`Analysis::infer_backward_param_types()` in `resolve.rs` sets `resolved_type` on unannotated local function parameters based on how they're used in the body. Runs inside the fixpoint loop's fallback branch (same branch that handles `@built-name` late resolution), gated by the `backward_param_types` flag (Analysis field, populated from `inference.backward_param_types` in `.wowluarc.json`; default `true`).

**Hints are treated as upper bounds and intersected.** Each use site implies a constraint that the param value must be assignable to; the inferred type is the narrowest type satisfying every constraint (`intersect_hints`/`intersect_pair` at the bottom of `resolve.rs`). Empty intersection (genuinely conflicting constraints) leaves the param untyped. A hint of `any` causes the pass to bail for that param — `any` is a no-information constraint that shouldn't combine with specific hints, and loose stub annotations like `tostring(v: any)` would otherwise coerce real hints away.

Hints are split into **baseline** and **narrowing**. Baseline hints alone drive inference; narrowing hints only tighten existing baseline hints. Narrowing never strips nil from a baseline that explicitly allowed it (every baseline hint contained nil) — the `?` on `@param a? T` is user intent, and a conditional use inside the body reflects a user-maintained invariant the LS can't verify. If a narrowing hint contradicts the baseline intersection entirely, the baseline-only intersection is used instead so a weak signal can't block inference.

Baseline hints:
- Arithmetic `param + n` / `param * n` / `-param` when the other side resolves to `number` → `number`
- Concatenation `param .. x` / `x .. param` when the other side `can_concat_to_string()` → `string | number`
- Passed as arg to a function whose corresponding param has a non-vararg annotation → that annotation's type (respects `self_offset` for colon calls). When the primary signature's arity doesn't match the call, a unique-arity overload is used instead; hints containing a `TypeVariable` (generic) are skipped.
- Passed as arg to a function whose corresponding param has no annotation but a `resolved_type` from a prior inner-iteration of this pass → that inferred type. This lets `outer(y) → inner(y)` inherit `inner`'s backward-inferred type across iterations. Only applies when there's no annotation (otherwise `resolved_type` would already reflect the annotation).

Narrowing-only hints (tighten but don't create inference):
- Passed as arg matching a target function's **variadic** annotation (e.g. `Log.Info(...)` with `@param ... string`) — stubs frequently over-specify varargs (`"%s" format accepts any` but is annotated `string`), so these can't alone drive inference.
- Assignment to an annotated field (`field_type_checks`), an annotated local (`assign_type_checks`), or as an annotated return value (`return_type_checks`).
- Any baseline hint whose contributing expression is **conditionally reached** — i.e. inside the RHS of a short-circuit `and`/`or`, or inside an if/elseif/else/while/for body. Such expressions may not execute on a given invocation of the enclosing function, so they can't establish a lower bound — they can only tighten one. Tracked via `conditionally_reached_exprs: HashSet<ExprId>` populated during `build_ir` at two sites: after lowering the RHS of any `And`/`Or` `BinaryExpression` (marking the RHS sub-tree's ExprId range), and after each statement lowered in a Frame whose `is_conditional` is true (if/elseif/else/while/for bodies; do-blocks and repeat bodies inherit their parent frame's flag; function-body frames reset to `false`).

The typed-arg signal is overload-aware: it filters the callee's primary + non-return-only `Function.overloads` by arg-count (`required..=total`, `is_vararg` for the primary), then collects hints at the candidate position from every matching signature. Generic `T` / `T[]` params are substituted via `substitute_generics_deep` using generics inferred from the sibling (non-candidate) args of the same call (`infer_array_element_type` for `T[]`, direct arg type for `T`). Unsubstituted type-variables are dropped. This prevents the 3-arg `tinsert(list, pos, value)` primary from infecting a 2-arg `tinsert(list, x)` with `pos: integer` — only the 2-arg `@overload fun(list: T[], value: T)` matches by arity, and `T` is inferred from the first arg's `T[]` type.

Skipped cases: `self` params, params already annotated (`param_annotations[i]` non-empty), params with an existing `resolved_type`, and external (stub) functions (`sym_idx >= EXT_BASE`).

**Multi-site caller bail-out.** Alongside body hints, `collect_backward_inference_hints` also records the actual arg types passed at each external call site of a candidate function (`caller` map on `BackwardInferenceHints`). These aren't mixed into the body-hint intersection (they're lower bounds, not upper bounds). Instead, `caller_types_mutually_compatible` runs `intersect_pair` on every pair; if any pair is disjoint, inference bails for that param and it stays untyped. Example: `register(GameTooltip)` + `register(ItemRefTooltip)` at top level → two disjoint class tables → body-inferred `GameTooltip` would spuriously reject the `ItemRefTooltip` site, so the param is left as `?`. A single conflicting caller (e.g. `f(nil)` where the body infers `number`) still goes through so the type-mismatch diagnostic fires — only *caller-vs-caller* disagreement bails, not caller-vs-body. `nil` arg types are dropped (signals optionality rather than a type), as are types containing a `TypeVariable`.

Because the pass runs inside the fixpoint fallback, expressions using the param re-resolve naturally on the next iteration via the existing cache-clear + pending-calls repopulation logic.

### Correlated return-only overload inference
`Analysis::synthesize_correlated_return_overloads()` in `build_ir.rs` adds synthetic return-only `ResolvedOverload` entries to a function whose return statements form a clear all-set-or-all-nil pattern. On by default; gated by `correlated_return_overloads` (Analysis field, populated from `inference.correlated_return_overloads` in `.wowluarc.json`; default `true`).

Trigger point: invoked from the `stack.pop()` handler in `build_ir()` when the popped frame's `func_id` differs from the new top-of-stack's `func_id` (i.e. the function body completed, not just a nested if/do block within it). Doing this BEFORE later statements that call the function is critical — `narrow_siblings` checks `is_return_only` at call sites, so the synthesized overloads must be in place before any later narrowing-triggering reference fires.

Detection groups `func.rets` versions by `(def_node.start, def_node.end)` (each group = one return statement). Requires:
- No `@return` annotations, no existing return-only overloads, not `has_vararg_return`, not `explicit_void_return`.
- ≥ 2 distinct return statements with matching arity ≥ 2.
- Every tuple is either entirely `Expr::Literal(Nil)` or has no `Nil` literals — mixed tuples (`return "x", nil`) are skipped to avoid false correlations where the "set" branch's nil position would survive narrowing on a sibling.
- ≥ 1 all-nil tuple AND ≥ 1 non-all-nil tuple (otherwise nothing to discriminate).

For each unique tuple a `ResolvedOverload { is_return_only: true }` is emitted. Position types are derived from each lowered return expression via `synthesized_return_type()`: `Nil` → `Nil`, string/number/boolean literals normalize to their generic types (avoiding ugly literal unions across branches), everything else → `Any`. Duplicate tuples are deduped by `returns` vector equality.

Two downstream consumers pick these up:
1. `narrow_siblings` — finds them via the existing `is_return_only` check; creates `OverloadNarrow` versions for the call's other return values exactly as it does for a hand-written tuple-union `@return`.
2. `resolve_function_call` — the FunctionRet base-type lookup at `func_scope` is replaced by an overload union when `func.return_annotations.is_empty() && any(is_return_only)`. This is required because the existing `get_symbol(FunctionRet, func_scope)` only finds returns at the function-body scope, not nested-if returns; for unannotated functions whose every return is in a nested branch, the lookup would otherwise produce no type. The synthesized overloads encode types for ALL return statements, so the union gives a useful base type. Use `self.func(func_idx).return_annotations` directly here — the local `return_annotations` variable in `resolve_function_call` is only cloned for generic functions.

### Unknown-type diagnostics (strict typing)
Four HINT-severity, default-disabled diagnostics fire at sites whose `resolved_type` ended up as `None` after all inference passes. `resolved_type = Some(Any)` is treated as an explicit author-written `@type any` / `@type unknown` / `@param x any` and skipped — there's no engine-level distinction between user-written `any` and resolver-produced `Any`, so `None` vs `Some(Any)` is the discriminator.

- `unknown-param-type` — unannotated, non-`self`, local function parameter whose type couldn't be inferred from the body (arithmetic/concat hints, typed-arg calls, etc.) or reconciled with caller types.
- `unknown-return-type` — a return expression with no resolvable type, **and** the function has no `@return` annotation at that ret_index. When `@return Foo` is declared, the annotation is authoritative — body mismatches are `return-type-mismatch` territory.
- `unknown-local-type` — `local x = expr` where `expr` resolves to `None`. Explicit `---@type Foo` produces `Some(_)` and is skipped.
- `unknown-field-type` — field assignment on a `@class` table (local or overlay) where the RHS resolves to `None` **and** the field has no `annotation_type_raw` (no `@field` declaration).

All four live in `checks.rs::check_unknown_*_type_diagnostics`, called from `resolve_types()` **before** the deferred drains (`check_return_type_diagnostics`, `check_unused_local_diagnostics`) so they can read `deferred.return_type_checks` and `deferred.local_defs` non-destructively. Param emission walks AST Parameter tokens (mirrors `incomplete_signature_doc`) since the param symbol's `def_node` points at the whole function, not the param name.

### Implicit protected for `_`-prefixed names
Runtime-discovered data fields starting with `_` are implicitly `Protected` when no explicit visibility annotation is present. This does **not** apply to explicit `@field` declarations — those default to `Public` since the author had the opportunity to write `@field protected`. This does **not** apply to methods — only data fields. The helper `default_visibility_for_name()` in `annotations.rs` centralizes the implicit protected logic. It is called from:
- Table constructor fields in `build_ir.rs`
- All FieldInfo construction sites in `pre_globals.rs` and `prescan.rs`
- `self._foo` assignments inside class methods (the class is defining its own field)
`@field` annotation parsing does **not** call `default_visibility_for_name()` — explicit declarations always use `Public` as the default, with `@field protected`/`@field private` for explicit restriction.
Runtime field assignments from outside the class (in `build_ir.rs` and `resolve.rs`) use `Visibility::Public` — ad-hoc injected fields should not get implicit protected since there is no `@field` declaration asserting protection.

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
- **Never `git stash` in a worktree**: All worktrees of a repo share a single stash stack (it lives on the common git dir, not per-worktree). Concurrent workspaces running `git stash push` / `pop` will clobber each other's entries. To shelve changes, use a per-worktree WIP commit (`git commit -m WIP`, reset later) or write to a uniquely-named ref (`git stash create` + `git update-ref refs/wip/<name>`).

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
- `tests/diagnostics/` — Semantic diagnostics with `diag:` assertions and @diagnostic suppression; `.wowluarc.json` enables `need-check-nil` + `implicit-nil-return`
- `tests/need-check-nil/` — Nil-checking diagnostics with nil-guard narrowing; `.wowluarc.json` enables the default-off `need-check-nil` code
- `tests/access-modifiers.lua` — Private/protected field access diagnostics (--with-stubs)
- `tests/references.lua` — Find references and rename
- `tests/undefined-global.lua` — Undefined global diagnostics (--with-stubs)
- `tests/undefined-field.lua` — Undefined field on @class tables diagnostics
- `tests/undefined-doc-class.lua` — Undefined class names in `@class Foo: Parent` inheritance position
- `tests/undefined-doc-name.lua` — Undefined type names in annotations (`@param`, `@return`, `@type`, `@field`, `@alias`, fun()/inline table shapes)
- `tests/circle-doc-class.lua` — Circular @class inheritance chain diagnostics
- `tests/generics.lua` — Generic type parameters with `@generic`
- `tests/funcall-access.lua` — Dot/colon access on function call return values
- `tests/builder-pattern/` — `@builds-field` and `@return built` builder pattern with edge cases and diagnostics; `.wowluarc.json` enables `need-check-nil`
- `tests/return-overloads.lua` — Tuple-union `@return` (`(A, B) | (C, D)`) sibling narrowing and variadic return expansion (`@return ...T`)
- `tests/tuple-union-returns.lua` — Focused tuple-union coverage: single-tuple shorthand, labels, per-case descriptions, `fun()` and `@alias` propagation, mixing/arity diagnostics
- `tests/cast.lua` — `@cast` (replace/add/remove) and `@as` inline expression type assertions
- `tests/annotation-completion.lua` — Annotation doc-comment completions: tag names, `@param` names, type suggestions
- `tests/type-narrows.lua` — `@type-narrows` custom type guard narrowing (then-branch, early-exit, else-branch, assert, method-style)
- `tests/type-guard.lua` — `type()` guard narrowing for symbols and field chains (`type(x) == "string"`, `type(obj.field) == "table"`, `type(x) ~= "nil"`)
- `tests/literal-bool-ret.lua` — Literal boolean return type union discrimination (`@return true`/`@return false` on union member methods)
- `tests/correlated-locals.lua` — Correlated local variable narrowing: locals assigned in every branch of if/elseif (no else) are narrowed together
- `tests/lateinit/` — `T!` non-nil assertion / lateinit fields: `@field` and `---@type` with `!` suffix; `.wowluarc.json` enables `need-check-nil`
- `tests/count-down-loop.lua` — Numeric for-loop step direction diagnostics (`count-down-loop`)
- `tests/incomplete-signature-doc/` / `tests/incomplete-signature-doc-meta/` — `incomplete-signature-doc` HINT for functions with partial `@param`/`@return` annotations; `-meta` asserts `@meta` files suppress the diagnostic. Each dir has a `.wowluarc.json` enabling the default-off code.
- `tests/stylistic.lua` — Stylistic HINT diagnostics: `empty-block`, `redundant-return`, `trailing-space`
- `tests/not-precedence.lua` — Operator precedence: `not x <cmp> y` parses as `(not x) <cmp> y` (`not-precedence`)
- `tests/syntax-coverage.lua` — Under-tested syntax constructs: hex/scientific/float literals, long strings, unary operators, repeat/until, for-step, semicolons, no-paren calls, anonymous functions, multi-dot definitions, code-after-break, long bracket comments
- `tests/convergence.lua` — Fixpoint convergence regression: 60 reverse-order function calls testing inner loop optimization
- `tests/metatable-type-i.lua` — Metatable type inference: `setmetatable()` + `__index` field propagation, chained metatables, self-referential `mt.__index = mt`, factory functions, instance field priority (--with-stubs)
- `tests/semantic-tokens.lua` — Semantic-token classification via the `tok:` assertion: function/method/class/namespace/parameter/property/variable tokens with `defaultLibrary`/`deprecated` modifiers (--with-stubs)
- `tests/backward-inference.lua` — Backward param-type inference signals: arithmetic/unary/concat, typed-argument propagation, annotated-param precedence, conflict fallback, overload-aware arity selection (2-arg call must pick the 2-arg `@overload`, not the 3-arg primary)
- `tests/backward-inference-disabled/` — Verifies `inference.backward_param_types: false` in `.wowluarc.json` disables the inference pass
- `tests/correlated-return-inference/` — Synthesized correlated return-only overloads (default-on; explicit `inference.correlated_return_overloads: true`): basic 2-tuple narrowing, 3-tuple, early-exit, skip cases (existing `@return`, single return, mismatched arity, mixed tuples, all-nil only, arity 1)
- `tests/correlated-return-inference-disabled/` — Verifies `inference.correlated_return_overloads: false` disables synthesis: nested-scope returns leave callers with `?`
- `tests/allowed-globals/` — Allowed globals via `.wowluarc.json` config (`globals.read`/`globals.write`) and `create-global` diagnostic
- `tests/unused-vararg/` — `unused-vararg` diagnostic for functions declaring `...` but never referencing it; uses `.wowluarc.json` to enable the default-disabled code
- `tests/unknown-types/` — Strict-typing `unknown-param-type` / `unknown-return-type` / `unknown-local-type` / `unknown-field-type` diagnostics; uses `.wowluarc.json` to enable the four default-disabled codes
- `tests/flavor-filter/` — Flavor filtering via `.wowluarc.json` (`flavors`), `@flavor-narrows` annotation, `WOW_PROJECT_ID` narrowing, and the `wrong-flavor-api` diagnostic. One subdirectory per scenario (classic-only, multi-flavor, wow-project-guard, annotation-guard, no-config, suppression).
- `tests/crossfile/` — Cross-file addon namespace resolution, `@defclass` with parameterized parent classes, `@builds-field` builder chains, `@class`/`@type` field access, `@class` inheritance, `@alias` usage, global functions/variables, access modifier diagnostics, typed self-field inheritance (`self_field_lib.lua`/`self_field_user.lua`), and deep addon-ns chains of 4+ parts with auto-created intermediate sub-tables (`deep_chain_defs.lua`/`deep_chain_user.lua`/`deep_chain_nonroot.lua`)
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

The test harness applies `ProjectConfigs::disabled_diagnostics_for()` to filter diagnostics — the same path the LSP server uses in `publish_with_config`. Tests that rely on default-off codes (`need-check-nil`, `implicit-nil-return`, `unused-vararg`, `incomplete-signature-doc`, `unknown-*-type`) must live in a subdirectory with an adjacent `.wowluarc.json` that opts in via `diagnostics.enable`. Existing examples: `tests/need-check-nil/`, `tests/incomplete-signature-doc/`, `tests/unused-vararg/`, `tests/unknown-types/`.

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
