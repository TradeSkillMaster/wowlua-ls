# wowlua_ls — Future Work

Running document of deferred work items and future improvements.

---

## Annotations

- **@see** — Cross-reference links (37 uses in WoW stubs). Documentation-only, no type system impact.
- **Recursive generic substitution** — `substitute_generics` currently only handles `TypeVariable` and `Union` variants. Generic type parameters nested inside `Function` or `Table` types (e.g. a generic function returning `fun(T): T`) are not substituted, because these are opaque index references into the IR. Supporting this would require storing generic type structure alongside `FunctionIndex`/`TableIndex` so substitution can reach into referenced types.
- **Malformed annotation diagnostics** — Annotations with missing or invalid names (e.g. `---@class` with no class name, `---@param` with no parameter name) are silently ignored. Emitting diagnostics for malformed annotations would help users catch typos and incomplete annotations. Scope: audit all annotation parsing paths in `annotations.rs` for silent drops and add appropriate warnings.
- **Generic constraint validation** — When inferring generic type bindings at a call site, inferred types are accepted unconditionally. Constraints (e.g. `@generic T: number`) are only used as fallbacks when inference fails, not validated against the inferred type. `foo("string")` with `@generic T: number` silently infers `T = string`. Should emit a type-mismatch diagnostic when the inferred type violates its constraint.

---

## Parser

- **Right-associative `^` operator** — The `^` (power) operator is parsed as left-associative like all other binary operators, but Lua specifies it as right-associative. `2^3^4` should parse as `2^(3^4)`, not `(2^3)^4`. Fixing this requires special-casing `^` in the precedence-climbing algorithm to use a different checkpoint strategy for right-associativity.

---

## Type Resolution

- **Call expression fixpoint resolution** — After the symbol fixpoint loop, remaining call expressions are resolved in a single linear pass. If resolution of one call depends on side effects (parameter type propagation) of another call appearing later in the list, it fails to resolve. A fixpoint loop over call expressions would improve coverage.
- **Rich array/generic type representation** — `table<K,V>` loses key type info after resolution (`resolve_annotation_type_mut` preserves value type but not key type). Diagnostics and type checking don't benefit from the key type. `annotation_text` is a display-only workaround for hover.
- **Cross-file function call return types on addon table fields** — `ns.Foo = ns.Bar.NewComponent("Foo")` where `NewComponent` returns a `@class` type can't be resolved at scan time. The field type remains `?`. Would require full type resolution during the workspace scan phase.

---

## Diagnostics

### Moderate value, worth considering

- **missing-fields** — Class instance missing required fields from `@class`/`@field` declarations. Deferred: complex to implement and high false-positive risk (Lua tables are dynamic, fields often set lazily/conditionally).

### Low value or not applicable to WoW

- **lowercase-global** — Lua convention, not WoW convention (WoW has many lowercase globals like `print`, `next`).
- **global-in-nil-env** — Lua 5.1 `setfenv` patterns. Not relevant to WoW Lua.
- **undefined-env-child** — Related to `_ENV` manipulation. Not relevant to WoW Lua.
- **circle-doc-class** — Circular `@class` inheritance. Edge case, low occurrence.
- **doc-field-no-class** — `@field` without preceding `@class`. Simple but rare mistake.
- **undefined-doc-class** — References to undefined class names in annotations. Moderate value.
- **undefined-doc-name** — References to undefined types in annotations. Moderate value.
- **unknown-cast-variable** — Casting undefined variables. Not applicable (we don't support `@cast`).
- **cast-type-mismatch** — Incompatible `@cast` types. Not applicable.
- **cast-local-type** — Local cast to different type. Not applicable.
- **close-non-object** — Lua 5.4 `<close>` variables. Not applicable to WoW Lua (5.1).
- **empty-block** — Empty `if` / `while` blocks. Stylistic, low signal.
- **trailing-space** — Whitespace lint. Better handled by formatters.
- **unused-label** — `::label::` never jumped to. Rare in WoW addons.
- **unused-vararg** — Unused `...` in function body. Low value.
- **redundant-return** — `return` at end of function with no value. Stylistic.
- **newfield-call** / **newline-call** — Ambiguous multi-line table/call patterns. Rare.
- **ambiguity-1** — Operator precedence ambiguity. Very rare.
- **count-down-loop** — Decrementing for loop with wrong step sign. Rare.
- **different-requires** — Same file required under different paths. Not applicable (WoW doesn't use `require`).
- **no-unknown** — Strict mode: flag all untyped variables. Too noisy for addon dev.
- **await-in-sync** / **not-yieldable** — Coroutine-related. Niche.
- **codestyle-check** / **name-style-check** / **spell-check** — Formatting/style. Out of scope.
- **global-element** — Convention warning for undeclared globals. Overlaps with `undefined-global`.
- **incomplete-signature-doc** / **missing-global-doc** / **missing-local-export-doc** — Doc completeness. Out of scope.
- **unknown-operator** — Unsupported operators like `**`. Already a parse error for us.

---

## Performance

- **Local overlay tables for external table mutation** — When user code assigns fields/methods to an external (stub-defined) table (e.g. `C_Timer.customField = 123`, `function C_Timer:Hook()`), the assignments are currently silently dropped because external tables are immutable (`Arc`-shared). To support hover/completion/go-to-def on these user-added fields, we'd need to create a local shadow table that inherits from the external one. Approach: when `find_table_for_symbol` returns an external table index and the code wants to mutate it, create a local `TableInfo` with `parent_classes` pointing to the external table, update the symbol's type to reference the local table, then perform the field insertion on the local copy. Queries would then see both external fields (via parent) and local additions.

---

## Known Limitations

- **`@return any` hover shows `?`** — The `any` annotation type resolves to `None` internally, so the direct result of calling functions with `@return any ...` (like `string.match`, `string.gmatch`) hovers as `?`. However, expressions built on top of these calls (e.g. `strmatch(...) and true or false`) still resolve correctly via `and`/`or` propagation with unknown operands. Fully fixing `any` would require adding an explicit `ValueType::Any` variant to the type system.

---

## WoW API Stubs

- **Flavor filtering** — Retail vs Classic API differentiation (bitmask data available in Ketho's repo).
