# v0.5.0

### New
- **`@event` annotation with payload hover** — A new `@event` annotation lets you declare event payload schemas. Hovering over event names (e.g. in `RegisterEvent` or handler functions) shows the event's payload parameters with syntax coloring, arrow formatting, and links to the wiki.
- **String literal completions for `==` / `~=` comparisons** — When comparing a variable against a string literal union type, the editor now offers completions for the valid string values.
- **TOC-based flavor narrowing** — Flavor restrictions from `.toc` file listings (suffixed TOCs, `AllowLoadGameType` headers, per-line directives) are now used to narrow `wrong-flavor-api` diagnostics per file.
- **Auto-detect `SLASH_*` globals** — Slash command globals (e.g. `SLASH_MYCOMMAND1`) are automatically recognized as allowed read/write globals without explicit configuration.

### Improvements
- **Richer type-mismatch messages for table literals** — `type-mismatch` diagnostics on table literal arguments now show which fields are missing or have wrong types.
- **Cross-file overlay field tracking** — Fields added to type-annotated locals (overlay fields) are now tracked cross-file.
- **Suppress `create-global` for `_G.field` assignments** — Explicit writes to `_G.field` no longer trigger the `create-global` diagnostic.
- **JetBrains plugin uses built-in LSP API and TextMate grammar** — The JetBrains plugin now uses the IDE's native LSP support and shared TextMate grammar instead of a third-party LSP library.
- **Return names shown in hover for external/cross-file functions** — Hover on functions defined in stubs or other files now displays return value names from `@return` annotations.

### Bug Fixes
- Fix `type-mismatch` false positive for hash-map + array mixed tables (e.g. `{ [1] = x, key = y }`).
- Fix `type-mismatch` false positive for enum-like class value types.
- Fix `doc-func-no-function` false positive for `@constructor` on `@class` annotation blocks.
- Fix `doc-func-no-function` false positive for functions inside `and`/`or` expressions.
- Fix `undefined-doc-name` false positive for class generics referenced on nested methods.
- Fix `undefined-field` false positive on fields assigned from function call results.
- Fix `missing-parameter` and `redundant-parameter` false positives for `__call` and `@constructor` callables.
- Fix `wrong-flavor-api` not narrowing inside `and` short-circuit expressions.
- Fix workspace scanners missing definitions inside `do...end` blocks.
- Fix LibStub type inference: backtick generic fallback produces `any` instead of failing; silent-aware nil return handling.
- Fix defclass dotted field assignment overwriting `var_to_result` mapping.
- Fix pipe characters inside string literal types being parsed as union separators.
- Fix `extract_type_prefix` not splitting correctly after string literal union types.
- Fix class property tracking for assignments on class instances.
