# Architecture Deep Reference

Deep internals of the wowlua_ls type system, inference engine, narrowing, and cross-file resolution. For the source file map, diagnostics catalog, conventions, and testing guide, see [CLAUDE.md](CLAUDE.md).

## Code lens (in `queries.rs` + `main_loop.rs`)
`code_lens_targets(tree)` collects one entry per function definition in the file: top-level named functions (scope-0 symbols with `ValueType::Function`), class methods (from `ir.classes`), and non-class table functions (scope-0 table-typed symbols). Each `CodeLensTarget` carries the function name, definition byte range (`def_start`/`def_end`), and a `name_offset` pointing inside the function's name token (suitable for `reference_target_at`).

The "N usages" LSP flow is two-stage:
1. **`textDocument/codeLens`** returns unresolved lenses (range + `data` containing URI and name_offset, no command).
2. **`codeLens/resolve`** counts references via `find_references_across_workspace` (same `text.contains(name)` prefilter + rayon parallel disk scan as find-references) and returns a `Command` with title "N usages" / "1 usage" / "0 usages" and command `editor.action.showReferences`.

`code_lens()` returns `Vec<CodeLensData>` with two additional kinds of annotations:
1. **"N implementations"** — shown on each local `@class` declaration. Counts direct subclasses by inverting the parent→child map from all classes (local + external). Uses `ir.class_def_ranges` for positioning and `table.parent_classes` for the child count.
2. **"overrides Parent"** — shown on methods that override a parent class method. Walks all local tables' function-typed fields, uses `function_owner_class` to find the owning class, then checks the class's `parent_classes` chain for a parent defining the same method name. Parent method detection uses a `class_methods: HashMap<class_name, HashSet<method_name>>` built from both local tables (via `function_owner_class`) and external class tables (via `field.annotation` or `Expr::FunctionDef`).

LSP handler in `main_loop.rs` registers `codeLensProvider` and converts byte ranges to LSP positions. "N implementations" uses two-stage resolve (like "N usages"): initial lens has `command: None` with `className` in `data`; on `codeLens/resolve`, `find_implementations_across_workspace()` searches `ws_file_classes` for child classes and returns their `def_range`/`def_path` as locations via `wowlua-ls.showReferences`. "overrides Parent" uses `wowlua-ls.showSuperDefinition` (navigates to parent definition).

## Cross-file find-references / rename
`references_at(offset)` runs against a single tree. For workspace-wide search, the LSP handler (`lsp/main_loop.rs::find_references_across_workspace`) composes three queries:
1. `AnalysisResult::reference_target_at(offset)` returns a `ReferenceTarget` (either `Symbol { idx, name }` or `Field { table_idx, field_name }`). An index `>= EXT_BASE` is stable across every `AnalysisResult` built from the same `PreResolvedGlobals`.
2. `AnalysisResult::promote_to_cross_file(&target)` lifts a file-local symbol or table to its workspace-wide counterpart when one exists (the defining file keeps a shadowing scope-0 local for its own global functions and a local `@class` table for its own `@class` declarations — both are swapped out for the `EXT_BASE+` idx when searching elsewhere).
3. `AnalysisResult::references_for_target(tree, &target, include_declaration, strict_shadow)` runs the search over an arbitrary tree against an externally-resolved target, enabling the LSP handler to iterate every open document and every scanned workspace file (rayon-parallel, gated by a `text.contains(target.name())` prefilter).

Consumer → defining-file matching works because the `Symbol` arm of `references_for_target` also accepts a scope-0 local whose name is in `ext.scope0_symbols` when the target is external; the `Field` arm accepts a local `@class` table whose `class_name` maps to the external `table_idx`.

The shadow-acceptance rule permissively matches any scope-0 local with the same name — including a truly-local `local X = 5` in a file that also has a workspace-wide `X` — which is desirable for find-references (the user wants to see the collision) but destructive for rename. The `strict_shadow` flag on `references_for_target` filters shadows whose first-version def-node sits inside a `local` statement (detected via `is_local_declaration_site`, which walks up to a `LocalAssignStatement` or a `FunctionDefinition` with a `LocalKeyword` child). The rename handler passes `strict_shadow=true`; find-refs passes `false`.

`include_declaration=false` drops the name-token range inside the first-version def-node for both the local target and any accepted shadow local. `def_name_token_range` translates the statement-level `DefNode` to the name-token range first, since `DefNode` ranges cover whole statements (e.g. the entire `function X() end`).

`textDocument/rename` is built on top of the same helper (prepare_rename + aggregated references with `strict_shadow=true`), so rename is workspace-wide but safer than find-refs against same-named file-locals.

## PreResolvedGlobals::build() phases (in `pre_globals/mod.rs`)
Built once at startup, shared via `Arc` across all files:
1. **Register class names** — Create empty `TableInfo` for each `@class`
2. **Populate @field entries** — Resolve annotation types, add to table fields
3. **Build method functions** — Create `Function` entries for methods, add to tables
4. **Resolve inheritance** — Fixpoint loop copying parent fields to children (handles 5+ levels), then substitutes parameterized class type params (e.g. `@class C<S>` with `@field __super S` → `S` replaced with concrete parent)
5. **Build global functions** — Create `Function` + `Symbol` entries, add to `scope0_symbols`
6. **Register non-class tables** — `math`, `string`, `table`, etc.

## Workspace scanning passes (in `main_loop.rs:scan_paths_with_overrides`)
Run before `PreResolvedGlobals::build()` to collect classes, aliases, and globals from all files:
1. **Pass 1** — `scan_all_annotations()` + `scan_file_globals()` per file: collect `@class`/`@alias` declarations and top-level function/variable globals
2. **Pass 2** — `scan_defclass_calls()` per file: discover classes from `@defclass` factory calls, extract constructor self-fields
3. **Pass 3** — `scan_built_name_calls()` per file: discover `@built-name` classes, merge with `@class` overlays
4. **Pass 4** — `scan_method_typed_self_fields()` per file: scan colon-method bodies for `self.field = expr ---@type Type` assignments on known classes. Captures both preceding-line and inline `---@type` annotations. Per-field file paths stored in `ClassDecl.field_paths` for cross-file definition locations.

## Metatable type inference (`setmetatable` + `__index`)
`setmetatable(tbl, mt)` is detected during Phase 2 resolution via `setmetatable_func_idx` stored on `PreResolvedGlobals`. When detected, `resolve_setmetatable()` in `resolve_call.rs`:
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
3. `__index` as a function whose return expressions access a class-typed table (e.g. `__index = function(self, key) if METHODS[key] then return METHODS[key] end end` where METHODS has `@class`).

**Limitations**: `setmetatable` mutates the table in-place — this means field assignments on a `setmetatable`-created table after the call ARE visible, but the metatable won't be set on external tables (idx >= EXT_BASE).

## Expression lowering — split identifier nodes (in `lower_expression.rs`)
The parser produces distinct node kinds for identifier access patterns instead of a single `Identifier` catch-all. The `Expression::Identifier` handler dispatches on node kind:
1. **NameRef** → `lower_name_ref()`: simple symbol lookup with type narrowing
2. **DotAccess** → `lower_dot_access()`: lower base expression, create `FieldAccess`
3. **BracketAccess** → `lower_bracket_access()`: lower base and key, create `BracketIndex`
4. **MethodCall** → `lower_method_call_as_callee()`: fully lower the base (including nested calls), then create `FieldAccess` for the method name. This is called when a MethodCall is used as a callee inside `lower_function_call`.

For chained method calls like `obj:A("x"):B("y")`, the parser nests MethodCall nodes. Each level's base is lowered as a complete FunctionCall before the next method name is resolved. Long chains (≥50 links) use `lower_function_call_chain()` for iterative processing to avoid stack overflow.

A legacy 4-way dispatch for old-style flat `Identifier` nodes is retained below the new handlers but is no longer exercised by the current parser.

## Parameterized classes (`@class Name<S>`)
Classes can declare type parameters: `@class BaseClass<S>`. Fields referencing type params (e.g. `@field __super S`) are stored with `annotation_type_raw` and re-resolved during substitution. The substitution chain:
1. A `@defclass T : P` factory declares `@generic T: BaseClass<P>` — binding class type param `S` to function generic `P`
2. At each call site, `P` resolves to the concrete parent class (e.g. `Animal`)
3. Fields with `annotation_type_raw` are re-resolved with `{S → Animal}`, so `__super` becomes `Animal`

Substitution happens in two places:
- **Per-file**: `prescan.rs:substitute_class_type_params()` for local defclass calls
- **Workspace-wide**: `pre_globals.rs` pass 3b for `scan_defclass_calls()`-discovered classes, using `ClassDecl.constraint_type_arg_subs`

## Generic argument inference (call-site `@generic T` binding)
Binding `@generic T` from call-site arguments happens in three layers in `resolve_call.rs` around `resolve_function_call`:

1. **Direct param types** (lines ~1459–1520): if the param's `resolved_type` is `TypeVariable(T)`, bind T to the arg type. If it's `Union(..., TypeVariable(T), ...)` (optional params, or explicit unions), extract the TypeVariable alternative and bind. Strip nil first so optional args don't pollute T.
2. **Structural inference** via `prescan.rs:infer_generics_from_annotation` (called at line ~1524): walks the raw `AnnotationType` to handle:
   - `T[]` — mine T from the arg's array element type
   - `table<K,V>` — mine V from table values, K = string
   - `` `T` `` (backtick) — resolve a string literal arg as a class name
   - `Fun(_, returns, _)` — if a return annotation is `Simple(T)`, extract T from the arg. The arg can be a function (use its first `@return`; fall back to `FunctionRet.resolved_type`, then `type_source`) or a named `@class` table (callable as constructor — T is the class itself). Plain non-class tables are excluded so `{}` literals don't silently bind T.
   - `Union(members)` — recurse into every member (no short-circuit), so multi-generic params like `(fun(): T) | U` can bind T from the Fun member AND U from the Simple member in one pass. Bare `Simple(T)` members bind T directly to the arg type.
   - `Simple(T)` when T is a generic — bind directly.
   - `NonNil(inner)` — recurse.
3. **Receiver `type_args`** (runs BEFORE the per-arg loop): for method calls whose `@param self Class<T>` is `Parameterized`, look up the receiver's `type_args` via `get_expr_type_args` and bind T from there. Runs first so class-generic `T` is bound from the explicit `---@type Class<X>` annotation before direct-arg binding can clobber it with the (rarely useful) arg's runtime type. Receiver-bound generics also join `substitutable_generic_names` so the type-mismatch loop substitutes them.

4. **Variadic generics** (after the per-arg loop): if a generic name starts with `"..."` (e.g. `"...M"` from `@generic T, ...M`), collect the types of all arguments past the positional param count. If any exist, bind the variadic generic to `Intersection(types)` (or the single type if only one). This powers `Mixin(obj, M1, M2, ...)` returning `T & M1 & M2 & ...` with no limit.

**`substitutable_generic_names`** (previously `structural_generic_names`) is the set of generics whose binding is trusted enough to substitute into sibling param types for the type-mismatch check. Populated from structural inference (`T[]`, `table<K,V>`, `fun(): T`), direct-TypeVariable-param inference, receiver-binding, and variadic generics. Explicitly NOT populated from promotional patterns (`` `T` `` backtick, `@defclass T`) where the bound value intentionally differs from the arg.

**`(fun(): T) | T` pre-emption** (lines ~1493–1510): when the raw annotation is a union containing a `Fun(..)` member, run structural inference *before* the eager Union-direct-bind. Otherwise the direct-bind would pick the `TypeVariable(T)` alternative and bind T to the arg itself (e.g. `T = Function(_)` when the user passes a callable), never giving the `Fun` member a chance.

## Function-type projections (`params<F>` / `returns<F>`)
Utility-type projections referencing the shape of a generic `F` bound to a `fun(...)` type. Declared in source as `AnnotationType::Parameterized("params" | "returns", [Simple(name)])` and stored on `Function` as per-slot overlays (`return_projections: HashMap<usize, ProjectionKind>` + `vararg_projection: Option<ProjectionKind>`), NOT as new `ValueType` variants. The `ProjectionKind` enum (`src/types.rs`) has `Params(String)` and `Return(String)` variants naming the referenced generic.

**Validation** at `prescan.rs::check_annotation_type_names` in the `Parameterized(base, args)` arm: `base == "params" || base == "returns"` requires exactly 1 arg of `Simple(name)` where name is a declared `@generic`. Violations emit `malformed-annotation`. `params<F>` outside the vararg slot (positional `@param x params<F>`, or `@return params<F>`) emits `malformed-annotation` during `insert_function_definition`. Nested projections (`returns<returns<F>>`) fail the `Simple` shape check.

**Population** (`build_ir.rs::insert_function_definition`):
- In the `@param ...` vararg branch, `match_projection` detects `params<F>` / `returns<F>` and sets `func.vararg_projection`.
- In the `@return` loop (legacy multi-line branch), each return slot that matches `returns<F>` gets `func.return_projections.insert(ret_index, Return(name))`.

**Resolver-level placeholder** (`prescan.rs::resolve_annotation_type_mut_gen`): when resolving a projection annotation with F still bound as an unresolved generic, returns `ValueType::Any` so the return/vararg slot exists in the IR. Call-site resolution replaces it with F's concrete type.

**Call-site resolution** (`resolve_call.rs::resolve_function_call`):
- `projected_f_idx` is computed early (before the per-arg loop) by looking up F from the receiver's type_args. Used by the arity check AND the per-arg type-mismatch loop.
- Arity check: when `projected_arity` is non-None, `expected_count = non_vararg_count + F.args.len()`; `effective_is_vararg = false`. Missing-param name uses F's arg name at the out-of-range position.
- Type-mismatch loop: for vararg positions (`i >= non_vararg_count`), pull expected type from `F.args[i - non_vararg_count].resolved_type`.
- Return resolution: when `return_projections[ret_index]` is `Return(name)` and `generic_subs[name]` is `Function(Some(f_idx))`, return `f.return_annotations[0]`. If F has multiple return annotations OR the function has tuple-union overloads, emit `multi-return-projection` warning (column 0 is still picked).

**Diagnostics**:
- `malformed-annotation` — shape errors (wrong arity, wrong arg kind, wrong position, nested projection, unknown generic).
- `multi-return-projection` (WARNING, `src/diagnostics/multi_return_projection.rs`) — `returns<F>` truncates when F has >1 return annotation. Suppressible via `@diagnostic disable:multi-return-projection`.

**Hover** (`queries.rs::format_function_decl`): class-declaration hover shows the raw `params<F>` / `returns<F>` via the existing `param_annotation_text` path (no special expansion). Call-site hover on the receiver's call expression already reflects the bound F's concrete return type via the normal resolve path. Signature help at call sites shows `func: F` unsubstituted — further expansion is a v2 enhancement.

## Carrying `type_args` from parameterized return types (`@return Pool<T>`)
When a generic function's return annotation is `Parameterized("Pool", [Simple("T")])`, the call's inferred T has to survive through the assignment so that subsequent method calls on the receiver (e.g. `pool:Get()`) can bind T from the receiver's type_args.

`ValueType::Table(Option<TableIndex>)` doesn't carry type_args, so we keep them outside the value:
- `Function.return_annotations_raw: Vec<AnnotationType>` — preserves the raw `Parameterized(..)` structure alongside the resolved `return_annotations: Vec<ValueType>` (populated in `build_ir.rs`, `prescan.rs`, and `pre_globals.rs`; `#[serde(default)]` for backward compatibility).
- `Analysis.call_type_args: HashMap<ExprId, Vec<ValueType>>` — per-call cache of substituted type_args. Populated in `resolve_function_call` whenever `generic_subs` is non-empty and the raw first-return annotation is `Parameterized`. The type_args are resolved using the function's own `generic_constraints_raw` so that `Simple("T")` becomes `TypeVariable("T")`, then `substitute_generics_deep` substitutes to concrete types.

`get_expr_type_args` (in `resolve_call.rs`) checks this cache:
1. Direct cache hit for the ExprId (covers `FunctionCall` receivers)
2. `SymbolRef(sym, ver)` — first check the version's `type_args` (set by `---@type Pool<Concrete>` in build_ir), then follow `type_source` ExprId into the cache
3. `FieldAccess { table, field }` — check the field's `annotation_type_raw`, then the field's stored `expr` in the cache (covers `private = { pool = New(...) }` table-field patterns)

Bump `pre_globals.rs::BLOB_VERSION` when changing any field on a serialized type (`Function`, `ClassDecl`, etc.).

## Builder pattern (`@builds-field` + `@return built`)
Builder methods use `@builds-field <param_idx> <type>` with `@return self` to progressively add typed fields to a shadow `built_table` on `TableInfo`. `@return built [: Parent]` returns the accumulated type.

Resolution in `resolve_call.rs`:
- **`@builds-field` + `@return self`**: `clone_table_with_built_field()` clones the receiver table with an updated `built_table` containing the new field. Each chained call produces a new table clone.
- **`@return built`**: Returns the `built_table` from the receiver. If `@return built : Parent` is specified, the parent class is added to the built table's `parent_classes`.

Key fields: `Function.builds_field: Option<(usize, ValueType, bool)>` (param_index, resolved_type, lateinit), `Function.built_name: Option<usize>`, `Function.built_extends: bool`, `Function.returns_built: bool`, `Function.returns_built_parent: Option<String>`, `TableInfo.built_table: Option<TableIndex>`.

The type in `@builds-field` supports `T!` (NonNil/lateinit): `@builds-field 1 T!` creates fields with `FieldInfo.lateinit = true`, allowing nil assignment without `field-type-mismatch`. The `!` is detected at three sites: `build_ir.rs` (per-file), `pre_globals.rs` build function resolution (cross-file `ClassDecl.fields`), and `pre_globals.rs` `build_on_stubs` (workspace overlay).

### Naming built types (`@built-name`)
`@built-name <param_idx>` on the chain entry point function sets the `built_table`'s `class_name` from the string literal at parameter `param_idx`. This allows the built type to be referenced by name in `@param`/`@type` annotations.

Resolution in `resolve_call.rs`:
- `clone_table_with_built_name()` creates a built table with the specified class name and registers it in `ir.classes`
- Subsequent `clone_table_with_built_field()` calls preserve the name and re-register the latest built table in `ir.classes`
- A post-fixpoint step re-resolves param annotations that reference newly discovered `@built-name` classes

Cross-file visibility: `scan_built_name_calls()` in `annotations.rs` scans workspace files for calls to `@built-name` functions, extracting class names and registering them as empty `ClassDecl` entries in `PreResolvedGlobals`.

### `@class` overlays on `@built-name` types
A `@class Foo` declaration that re-uses a name already created via `@built-name` merges its `@field` annotations with the builder-pattern fields. Overlay `@field` types take precedence over built field types for matching names. The overlay must be standalone (not directly preceding a `local` statement, which would be interpreted as typing the variable).

Resolution happens at three levels:
- **Per-file** (`resolve_call.rs`): `clone_table_with_built_name()` checks `ir.classes` for a local `@class` table with the same name and merges its `@field` annotations (identified by `annotation_type_raw.is_some()`) into the built table. `clone_table_with_built_field()` skips overwriting fields that have `annotation_type_raw` (from overlays).
- **Per-file prescan** (`prescan.rs`): After populating local class fields, external class fields (from `ext.classes`) are imported into local `@class` overlay tables for any matching names.
- **Workspace** (`main_loop.rs`): When merging `ws_file_defclasses` (from `scan_built_name_calls()`) into `ws_classes`, built-name fields are merged into existing `@class` entries instead of being skipped.

### Extending builder schemas (`@built-extends`)
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

Resolution in `resolve_call.rs`:
- `clone_table_with_built_name()` with `extends=true` creates a new built table whose `parent_classes` include the receiver's existing built table plus all its ancestors (flattened for single-level FieldAccess resolution)
- Subsequent `clone_table_with_built_field()` calls preserve the parent chain, so fields added after `Extend` still inherit from the base
- Multi-level extension works: grandchild → child → base, with all ancestor fields accessible

## Variadic generics (`@generic T, ...M`)
A variadic generic parameter (prefixed with `...`) collects excess positional arguments into an intersection type. Stored as `("...M", None)` in the existing `generics: Vec<(String, Option<String>)>` — no new data structures.

**Parsing**: `...M` in type positions parses as `VarArgs(Simple("M"))` via existing `parse_type()`. In `resolve_annotation_type` (annotation_scanning.rs), the `VarArgs` arm checks if `"...{name}"` is in the generics list → returns `TypeVariable("...M")`.

**Binding** (resolve_call.rs `infer_generic_subs`): after the per-arg binding loop, finds any generic name starting with `"..."`. Collects types of all arguments beyond the positional param count. Binds to `Intersection(types)` if multiple, or the single type if one.

**Substitution** (resolve_call.rs `substitute_generics_deep`): the `Intersection` arm flattens nested intersections after substitution (when `...M` substitutes to `Intersection(A, B)` inside `T & ...M`, the result is `T & A & B` not `T & Intersection(A, B)`). Unbound variadic generics (`TypeVariable("...M")`) are filtered out by the existing TypeVariable filter.

## `@narrows-arg N` (in-place argument type mutation)
`@narrows-arg N` on a function means: when the function is called as a bare statement (not assigned), narrow the Nth argument's type to the function's return type. Mirrors the `@cast` mechanism.

**Data**: `narrows_arg: Option<usize>` on both `AnnotationBlock` (annotations/mod.rs) and `Function` (types.rs). 1-based index. Stored on `ExternalGlobal` for workspace scanning.

**Phase 1** (build_ir.rs `try_narrows_arg`): after `Statement::FunctionCall`, looks up the callee in external globals, checks `narrows_arg`. Finds the argument symbol via AST, creates a new `SymbolVersion` with `type_source = call_expr_id`. During Phase 2, the call expression resolves to the return type (with generics substituted), which becomes the argument's new type.

**Limitation**: `try_narrows_arg` only handles single-name external globals (e.g. `Mixin`). Namespace-qualified calls (`ns.Mixin`) and user-defined local functions are not supported. This suffices for WoW's built-in `Mixin()` function.

**Validation** (malformed_annotation.rs): `@narrows-arg` requires a numeric argument >= 1.

## Tuple-union `@return` syntax
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

**Narrowing implementation** (unchanged from the old `@overload return:`): `multi_return_siblings` in `Analysis` tracks which symbols came from the same function call. `narrow_siblings()` in `narrowing.rs` hooks into all narrowing points (`analyze_nil_guard`, `analyze_early_exit_guard`, assert narrowing). It checks `check_return_only_overloads_from_siblings()` to only activate for functions with `is_return_only` overloads. Return-only overloads are filtered out of arg-count matching in `resolve_call.rs`.

**Overload-based narrowing**: When a sibling is narrowed, `narrow_siblings()` creates `Expr::OverloadNarrow` versions for ALL other siblings. The OverloadNarrow stores `(ret_index, func_expr, narrowed)` where `narrowed` is a list of `(sibling_ret_index, NarrowKind)` entries for each directly-guarded sibling. `NarrowKind` has four variants: `StripNil` (e.g. `x ~= nil`), `StripFalsy` (e.g. `if x then`), `StripTruthy` (e.g. `if not x then` or `else` of `if x then`), and `ClassEq(String)` (e.g. `if x == Foo.MEMBER then` where `Foo.MEMBER` is class-typed). During resolve, `resolve_overload_narrow()` filters return-only overloads whose type at each narrowed sibling's position is compatible with the `NarrowKind` (`overload_type_survives_{strip_nil,strip_falsy,strip_truthy}` / `overload_type_matches_class`), then computes the union of types at `ret_index` across compatible overloads. Overload-narrowed siblings are NOT added to `narrowed` to avoid double-stripping nil in `narrow_type_for_display`. For cross-file calls (deferred case), the narrowed_info is stored in `deferred_sibling_narrowings` and processed during the resolve fixpoint loop. `push_overload_narrow_version()` uses `version_for_scope_ancestors_only` for the base version so that a narrowing created inside a sibling branch scope can't become the base of an outer-scope narrowing.

**Class-equality narrowing (`x == Foo.MEMBER`)**: Detected during `analyze_nil_guard` by `record_class_eq_deferral()` when the non-symbol side is a pure identifier chain (a bare `Identifier` / `DotAccess` — see *Expression lowering — split identifier nodes* above). The RHS is lowered and queued in `deferred_class_eq_narrowings` as `(sym_idx, expr_id, scope_idx)`. At resolve, `resolve_deferred_class_eq_narrowings()` resolves the RHS: if its type is (or contains) a class table with a `class_name`, it sets `class_narrowed_symbols[scope][sym] = class_name`, inserts into `type_filtered_symbols` (for symbol-level display), pushes a `TypeFilter` version, and propagates to multi-return siblings via `push_overload_narrow_version` with `NarrowKind::ClassEq(name)`. To reach references lowered before resolve, `rewrite_sym_refs_in_subtree()` walks `sym_ref_sites` (a reverse index `SymbolIndex → Vec<(ExprId, token_offset)>` populated at build time) and redirects `Expr::SymbolRef` exprs in the affected scope subtree to the new version, updates `symbol_version_at`, invalidates the `resolved_expr_cache`, and prunes stale `need-check-nil` / `type-mismatch` diagnostics emitted pre-narrowing. Restricting detection to pure identifier chains avoids re-lowering embedded name references (e.g. `name` inside `strlower(name)`) which would clobber the original `symbol_version_at` entries.

**Narrowing tracking maps (convention)**: The narrowing maps live in the `NarrowingState` struct (`analysis/mod.rs`), embedded in both `Analysis` (mutable phase) and `AnalysisResult` (query/diagnostic phase) and accessed through thin forwarder methods. Each map's name describes what the guard *stripped* to produce the narrowing, not what the value is: `narrowed` = nil stripped; `falsy_narrowed` = nil AND false stripped (a subset of `narrowed`); `truthy_narrowed_symbols` = truthy stripped, so the value is `nil | false`; `class_narrowed_symbols` = equated to a class (value IS that class). So "truthy_narrowed" reads as "truthy-ness stripped" → value is falsy. The `narrowed`, `falsy_narrowed`, `type_narrowed`, and `type_stripped` maps are keyed by `NarrowTarget` (an enum with `Symbol(SymbolIndex)` and `Field(SymbolIndex, Vec<String>)` variants), so a single map covers both bare symbols and field chains; `NarrowTarget`'s `Eq`/`Hash` gives exact-match semantics for free (narrowing `a.b` never matches `a.b.c`). The remaining maps (`truthy_narrowed_symbols`, `class_narrowed_symbols`, `num_compare_narrowed_symbols`, `type_filtered_symbols`) stay symbol-only.

**Temporary insert/restore protocol for `and`/`or` RHS**: `analyze_nil_guard` inserts into the tracking maps at a branch scope (then/else), so entries disappear naturally once the branch scope is out of view. The short-circuit `and`/`or` lowering in `lower_expression.rs::BinaryExpression` operates in the *same* scope as the containing expression, so it uses a temporary-insert-then-remove protocol: (1) record what was inserted (`Vec<(SymbolIndex, bool, bool)>` flags whether each map actually took a new entry); (2) call whatever consumes the maps (`narrow_siblings`, etc.); (3) after RHS lowering, remove *only* the entries you added. Sibling `OverloadNarrow` versions pushed during this window are also scope-persistent — pair each narrow call with a pre-narrow version snapshot, then call `ir.push_alias_version(sym, pre_ver, scope)` at teardown to revert the symbol's current version to the pre-`and` state. Any future code that inserts into these maps mid-RHS must follow the same pattern or the cleanup will under- or over-remove.

**Callee enforcement**: The `grouped-return-mismatch` diagnostic (deferred check in `checks.rs`) verifies that each `return` statement matches one of the return-only overloads. The `missing-return-value` diagnostic is suppressed for functions with a nil return-only overload.

## Literal boolean return type union discrimination
When a union type `A | B` has a method where `A:Method()` is annotated `@return false` and `B:Method()` is annotated `@return true`, the LS automatically narrows the union in conditional branches — then-branch keeps the `true`-returning types, else-branch keeps the `false`-returning types.

**Implementation** (`narrowing.rs`):
- `resolve_expr_to_tables()` — like `resolve_expr_to_table()` but returns ALL table indices from a union type
- `extract_bool_discriminator()` — given a method call on a union receiver, checks if all union member tables define the method with complementary literal boolean `@return` annotations. Returns `(sym_idx, true_type, false_type)`.
- Integrated into `analyze_nil_guard` (then + else branches), `analyze_early_exit_guard`, and `narrow_assert_expr`

**Conditions**: all union members must define the method, every return annotation must be literal `true` or `false` (not generic `boolean`), and at least one of each must exist. Works with 3+ member unions.

## Correlated nil fields (`@correlated`)
`@correlated field1, field2, ...` on a `@class` declares that listed optional fields are always nil/non-nil together. Stored as `correlated_groups: Vec<Vec<String>>` on `TableInfo`. Multiple `@correlated` lines per class create independent groups. Groups are inherited by child classes during prescan pass 3.

**Narrowing**: In `try_narrow_field()` and `try_narrow_field_falsy()` (narrowing.rs), after inserting the primary narrowing, `narrow_correlated_fields()` resolves the field's table via `resolve_field_chain_table()`, looks up its `correlated_groups`, and inserts sibling narrowings into `narrowed` (and `falsy_narrowed` if applicable) as `NarrowTarget::Field` entries. Works for both `self.field` (chain len 1) and `self.sub.field` (chain len 2+) patterns, and with early-exit narrowing.

## Correlated locals (inferred from if/elseif branches)
When multiple local variables are assigned in every explicit branch of an if/elseif chain (without else), they form a correlation group. Stored as `correlated_locals: Vec<Vec<SymbolIndex>>` on `Analysis`. When one member is narrowed via a nil guard or early-exit guard, all siblings in the group are narrowed too.

**Detection**: In the `PendingBranchMerge` processing (build_ir.rs), after collecting symbols assigned in branch scopes, symbols that are assigned (not just narrowed) in ALL explicit branches of a `has_implicit_else=true` merge are collected into a correlation group.

**Narrowing**: `narrow_correlated_locals()` in narrowing.rs is called from `narrow_symbol_strip_nil()`, `narrow_symbol_strip_falsy()`, and direct narrowing insertion points in `analyze_nil_guard()`, `analyze_early_exit_guard()`, and `narrow_assert_expr()`. It looks up the symbol in `correlated_locals` groups and inserts sibling narrowings into `narrowed` (and `falsy_narrowed` if applicable) as `NarrowTarget::Symbol` entries.

## Guard implications (`if A and not B then return`)
An early-return guard `if A1 and ... and An and not B then return end` establishes, for code reached past it, the implication `(A1 and ... and An) ⟹ B is truthy` (the negation of the guard condition is `not A1 or ... or B`; `not B` being false means B is truthy). Stored as `guard_implications: Vec<(Vec<SymbolIndex>, SymbolIndex, ScopeIndex)>` on `Analysis` (antecedents, single consequent, the scope the guard lives in).

**Detection**: `detect_guard_implications()` in narrowing.rs runs from build_ir.rs alongside `detect_complementary_exit_guards`, for every exiting branch of the early-exit prefix. It reuses `extract_and_truthiness_shape()` and records an entry only when there is ≥1 truthy antecedent and exactly one negated term (multiple negated terms give `B1 or B2 or ...`, which pins down no single consequent). Duplicate entries are avoided by checking existing implications before pushing.

**Application**: `apply_guard_implications()` fires from `narrow_symbol_strip_falsy()`, the bare-identifier then-branch insert in `analyze_nil_guard_inner()`, and `narrow_or_coalesce_derived()` (falsy branch). When a symbol that is an antecedent gets narrowed truthy in a scope, and (a) that scope is within the guard's scope subtree (`is_ancestor_scope`, guarding against guards nested in conditional branches leaking out) and (b) every antecedent is now `falsy_narrowed` in that scope, the consequent is `narrow_symbol_strip_falsy`-ed. The consequent is claimed truthy (falsy-stripped), not merely non-nil: the guard `if A and not B then return` means `not B` was false past the guard, i.e. B is truthy.

**Invalidation**: `invalidate_guard_implications()` runs at both reassignment sites in build_ir.rs (alongside `invalidate_correlated_locals`), dropping any implication that mentions the reassigned symbol as antecedent or consequent.

## `x = x or y` coalesce narrowing
The idiom `x = x or y` makes `x` non-nil whenever `y` is non-nil: either the old `x` was truthy (kept) or `y` is used (and `y` non-nil means the result is non-nil). Narrowing is one-directional — narrowing `y` narrows `x`, but narrowing `x` tells you nothing about `y`. Stored as `or_coalesce_derivations: HashMap<SymbolIndex, Vec<SymbolIndex>>` (source `y` → derived `x`s).

**Detection**: `maybe_register_or_coalesce()` runs at every simple-name assignment `x = expr` in `narrowing.rs`. When `expr` is `BinaryOp(Or, NameRef(x), NameRef(y))` and both sides resolve to symbols (with the LHS matching the target), it registers `(y, x)`. Any other assignment to `x` invalidates prior `(*, x)` entries — the coalesce relationship is tied to this specific assignment.

**Narrowing**: Propagated from `narrow_symbol_strip_nil()` / `narrow_symbol_strip_falsy()` via `narrow_or_coalesce_derived()`, from the direct-insert narrowing sites in `analyze_nil_guard_inner` (then-branch `if x then` / `if x ~= nil then` / `if type(x) ~= "nil" then` / `if type(x) == "T" then`) and `narrow_assert_expr` (`assert(x ~= nil)`, `assert(type(x) ...)`), and from the temporary `and`/`or`-guard narrowings in `lower_expression.rs`'s `BinaryExpression` arm (lines near `coalesce_pre_narrow`). Guard-path propagation pushes a transient StripNil/StripFalsy version on each derived symbol alongside the primary/extra guard narrowings, then restores them in the same reverse-order pass that restores the primary guard.

## Flavor filtering (`flavors` config + TOC detection + `@flavor-narrows` + `wrong-flavor-api`)
Projects declare target WoW flavors in `.wowluarc.json` via `flavors: [...]` (accepting `retail`, `classic`, `classic_era` — the three WoW install-folder names), or flavors are auto-detected from TOC file listings. Each `Function` carries a `flavors: u8` (the 3-bit mask `crate::flavor`) and a `flavor_guard: u8` (from the `@flavor-narrows` annotation). `Symbol` and `FieldInfo` also carry `flavor_guard: u8` for boolean variable/field guards.

Stub gen: `src/stub_gen/` reads Ketho's `flavor.ts` (4-bit mainline/mists/bcc/classic_era mask) and passes each entry through `crate::flavor::from_ketho_mask` to collapse mists+bcc into our `classic` bit. `stub_gen::classic::apply_flavor_data` writes the translated mask into each matching `ExternalGlobal.flavors`, and `PreResolvedGlobals::build_function` pipes it through to `Function.flavors`.

Narrowing: `Analysis` carries `project_flavors: u8` and `scope_flavors: HashMap<ScopeIndex, u8>`. `try_flavor_narrow()` in narrowing.rs detects three patterns: `WOW_PROJECT_ID == WOW_PROJECT_*` comparisons, `@flavor-narrows` guard function calls (`flavor_guard_mask_for_call`), and `@flavor-narrows` boolean variables/fields (`flavor_guard_mask_for_ident`). All three call `narrow_scope_flavors()` or `exclude_scope_flavors()` on the target scope. `active_flavors_at(scope)` walks ancestor scopes to the first explicit override; if none, returns `project_flavors`.

Because annotation guards on local functions aren't typed at build-ir time, `flavor_guard_mask_for_call` uses `find_function_def(type_source)` to walk the symbol's `type_source` to a `FunctionDef` directly (bypassing `resolved_type`, which is only populated in Phase 2). Boolean guards (`flavor_guard_mask_for_ident`) check `Symbol.flavor_guard` for single-name identifiers and `FieldInfo.flavor_guard` for dotted field access.

Short-circuit `and` narrowing: `collect_and_chain_flavor_guards()` and `detect_and_lhs_flavor_guard_leaf()` in narrowing.rs detect `@flavor-narrows` guards in the LHS of `and` expressions. During expression lowering (`lower_expression.rs`), all `FunctionCall` exprs in the RHS range are recorded in `Ir.and_guarded_flavor_exprs: HashMap<ExprId, u8>` with the effective narrowed flavor mask. The `wrong-flavor-api` diagnostic checks this map before falling back to `active_flavors_at(scope_idx)`.

TOC-based detection: `parse_toc_files()` in `config.rs` reads all `.toc` files in a directory and extracts per-file flavor masks. Three mechanisms are supported: (1) TOC filename suffixes (`_Mainline`, `_Classic`, `_Vanilla`, `_Cata`, `_Wrath`, `_TBC`, `_Mists`) mapped via `flavor::parse_toc_suffix()`; (2) `## AllowLoadGameType:` header restricting the entire TOC; (3) `[AllowLoadGameType ...]` per-line directives restricting individual files. The unsuffixed (base) TOC covers flavors not claimed by any suffixed TOC. Per-file flavor masks are stored in `ProjectConfigs.toc_file_flavors` and intersected with project-level `flavors` in `flavors_for()`. `FLAVOR_ALL` entries are pruned (no restriction needed). Game type names (`mainline`, `vanilla`, `cata`, etc.) are parsed via `flavor::parse_game_type_name()`, which is more permissive than `parse_flavor_name()`.

Path variables `[Family]` and `[Game]` in TOC file listings are expanded via `expand_toc_path_variables()`. Each expansion value maps to a flavor (e.g. `[Game]` → `Standard`=retail, `Vanilla`=classic_era, `Cata`=classic). Only expansions whose file exists on disk are included. The expansion tables are `flavor::FAMILY_EXPANSIONS` and `flavor::GAME_EXPANSIONS`.

Diagnostic: `wrong_flavor_api.rs` emits `wrong-flavor-api` at the call site when `unsupported_flavors(active, call.flavors)` is non-zero. Fires only when `project_flavors != 0` and the function has non-zero `flavors` (a mask of 0 is treated as "available everywhere").

## DefNode (source location pointers)
Symbol and function definitions store `DefNode { start: u32, end: u32 }` — a simple byte range with no dependency on the syntax tree. External symbols use `DefNode::DUMMY`. `definition_at()` returns `DefinitionResult::External(loc)` for external symbols instead of trying to look up the node.

## `self` parameter handling (cross-cutting invariant)
A parameter named `self` can be **implicit** (colon syntax: `function Foo:bar(x)` → parser sees `[x]`, self injected by `insert_function_definition`) or **explicit** (dot/global: `function handler(self, index)` → parser sees `[self, index]`). Three code paths must agree on this distinction:
1. **Stub scanning** (`annotations.rs:scan_file_globals`) — Only filter `self` from unannotated param lists when `is_call_to_self()` (colon syntax). Global functions with explicit `self` must keep it.
2. **Function building** (`build_ir.rs:insert_function_definition`) — `inject_self` adds a synthetic self param for colon-defined methods.
3. **Call-site `self_offset`** (`resolve_call.rs`) — Offset by 1 when `is_method_call` (colon call) AND the function has any first param (whether named `self` or not, including stored function fields). Plain calls pass all args explicitly, so offset must be 0 regardless of the param name.

## Backward param-type inference
`Analysis::infer_backward_param_types()` in `resolve_call.rs` sets `resolved_type` on unannotated local function parameters based on how they're used in the body. Runs inside the fixpoint loop's fallback branch (same branch that handles `@built-name` late resolution), gated by the `backward_param_types` flag (Analysis field, populated from `inference.backward_param_types` in `.wowluarc.json`; default `true`).

**Hints are treated as upper bounds and intersected.** Each use site implies a constraint that the param value must be assignable to; the inferred type is the narrowest type satisfying every constraint (`intersect_hints`/`intersect_pair` in `resolve_call.rs`). Empty intersection (genuinely conflicting constraints) leaves the param untyped. A hint of `any` causes the pass to bail for that param — `any` is a no-information constraint that shouldn't combine with specific hints, and loose stub annotations like `tostring(v: any)` would otherwise coerce real hints away.

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

## Correlated return-only overload inference
`Analysis::synthesize_correlated_return_overloads()` in `narrowing.rs` adds synthetic return-only `ResolvedOverload` entries to a function whose return statements form a clear all-set-or-all-nil pattern. On by default; gated by `correlated_return_overloads` (Analysis field, populated from `inference.correlated_return_overloads` in `.wowluarc.json`; default `true`).

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

## Unknown-type diagnostics (strict typing)
Four HINT-severity, default-disabled diagnostics fire at sites whose `resolved_type` ended up as `None` after all inference passes. `resolved_type = Some(Any)` is treated as an explicit author-written `@type any` / `@type unknown` / `@param x any` and skipped — there's no engine-level distinction between user-written `any` and resolver-produced `Any`, so `None` vs `Some(Any)` is the discriminator.

- `unknown-param-type` — unannotated, non-`self`, local function parameter whose type couldn't be inferred from the body (arithmetic/concat hints, typed-arg calls, etc.) or reconciled with caller types.
- `unknown-return-type` — a return expression with no resolvable type, **and** the function has no `@return` annotation at that ret_index. When `@return Foo` is declared, the annotation is authoritative — body mismatches are `return-type-mismatch` territory.
- `unknown-local-type` — `local x = expr` where `expr` resolves to `None`. Explicit `---@type Foo` produces `Some(_)` and is skipped.
- `unknown-field-type` — field assignment on a `@class` table (local or overlay) where the RHS resolves to `None` **and** the field has no `annotation_type_raw` (no `@field` declaration).

All four are implemented as `DiagnosticPass` trait implementations in their respective modules under `src/diagnostics/`, running as post-analysis passes via `run_all()`. Param emission walks AST Parameter tokens (mirrors `incomplete_signature_doc`) since the param symbol's `def_node` points at the whole function, not the param name.

## Implicit protected for `_`-prefixed names
Runtime-discovered data fields starting with `_` are implicitly `Protected` when no explicit visibility annotation is present. **This behavior is configurable and disabled by default.** Set `inference.implicit_protected_prefix: true` in `.wowluarc.json` to enable it. This does **not** apply to explicit `@field` declarations — those default to `Public` since the author had the opportunity to write `@field protected`. This does **not** apply to methods — only data fields. The helper `default_visibility_for_name()` in `annotations.rs` centralizes the implicit protected logic and takes an `implicit_protected_prefix: bool` parameter. It is called from:
- Table constructor fields in `build_ir.rs`
- All FieldInfo construction sites in `pre_globals.rs` and `prescan.rs`
- `self._foo` assignments inside class methods (the class is defining its own field)
`@field` annotation parsing does **not** call `default_visibility_for_name()` — explicit declarations always use `Public` as the default, with `@field protected`/`@field private` for explicit restriction.
Runtime field assignments from outside the class (in `build_ir.rs` and `resolve.rs`) use `Visibility::Public` — ad-hoc injected fields should not get implicit protected since there is no `@field` declaration asserting protection.
