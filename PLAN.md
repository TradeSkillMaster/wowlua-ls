# wowlua_ls — Future Work

Running document of deferred work items and future improvements.

---

## Annotations

- **@see** — Cross-reference links (37 uses in WoW stubs). Documentation-only, no type system impact.

---

## Diagnostics

### Low value

- **global-in-nil-env** — Lua 5.1 `setfenv` patterns.
- **doc-field-no-class** — `@field` without preceding `@class`. Simple but rare mistake.
- **undefined-doc-name** — References to undefined types in annotations. Moderate value.
- **unknown-cast-variable** — Casting undefined variables. Not applicable (we don't support `@cast`).
- **cast-type-mismatch** — Incompatible `@cast` types. Not applicable.
- **cast-local-type** — Local cast to different type. Not applicable.
- **empty-block** — Empty `if` / `while` blocks. Stylistic, low signal.
- **trailing-space** — Whitespace lint. Better handled by formatters.
- **unused-vararg** — Unused `...` in function body. Low value.
- **redundant-return** — `return` at end of function with no value. Stylistic.
- **newfield-call** / **newline-call** — Ambiguous multi-line table/call patterns. Rare.
- **ambiguity-1** — Operator precedence ambiguity. Very rare.
- **count-down-loop** — Decrementing for loop with wrong step sign. Rare.
- **no-unknown** — Strict mode: flag all untyped variables. Too noisy for addon dev.
- **codestyle-check** / **name-style-check** / **spell-check** — Formatting/style. Out of scope.
- **global-element** — Convention warning for undeclared globals. Overlaps with `undefined-global`.
- **incomplete-signature-doc** / **missing-global-doc** / **missing-local-export-doc** — Doc completeness. Out of scope.

---

## Known Limitations

- **Reassignment overwrites hover type for earlier references** — Symbol versions lack positional awareness: if a variable is reassigned later in a block (e.g. `node = node.next` in a while loop), hover on earlier references shows the reassigned type rather than the version at that point. The nil-check diagnostic is correctly suppressed by narrowing, but hover displays the wrong (nullable) type.

- **Cross-file addon chains deeper than 3 parts** — The scanner handles `ns.X.Y = expr` (3-part chains) for addon namespace fields, but deeper chains like `ns.A.B.C = expr` are silently ignored. In practice WoW addon code doesn't use deeper chains at the top level.

- **`@return any` hover shows `?`** — The `any` annotation type resolves to `None` internally, so the direct result of calling functions with `@return any ...` (like `string.match`, `string.gmatch`) hovers as `?`. However, expressions built on top of these calls (e.g. `strmatch(...) and true or false`) still resolve correctly via `and`/`or` propagation with unknown operands. Fully fixing `any` would require adding an explicit `ValueType::Any` variant to the type system.

---

## WoW API Stubs

- **Flavor filtering** — Retail vs Classic API differentiation (bitmask data available in Ketho's repo).
