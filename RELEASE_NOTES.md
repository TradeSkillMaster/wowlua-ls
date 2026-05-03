# v0.6.0

## New Features

### Editor features
- **Inlay hints** — inline annotations for parameter names, variable types, function return types, for-loop variable types, parameter types (opt-in), and chained method return types (opt-in)
- **Code lens** — "N usages" counts on function definitions, "N implementations" on `@class` declarations, and "overrides Parent" on methods
- **Call hierarchy** — navigate incoming/outgoing call relationships, with cross-file support
- **Document symbols** — outline view and breadcrumb navigation
- **Workspace symbols** — cross-file symbol search
- **Document highlight** — highlight all references to the symbol under the cursor
- **Code folding** — collapsible regions for functions, blocks, and comments
- **Linked editing ranges** — live rename for local variables without a full rename operation
- **Unused variable dimming** — `unused-local` / `unused-function` diagnostics use `DiagnosticTag::Unnecessary` so editors dim them automatically

### Type system & inference
- **Multi-addon workspaces** — `addon_root: true` in per-addon `.wowluarc.json` isolates addon namespaces so addons in the same workspace don't leak globals into each other (#10)
- **Glob patterns in config** — `globals.read`, `globals.write`, and `ignore` paths now accept glob patterns like `SLASH_*` (#1)
- **Inline function param inference** — function parameters are inferred from `@field` type declarations on the containing class (#7)
- **Preserve `table<number, V>`** — numeric-keyed class tables keep their `table<number, V>` type instead of collapsing to `V[]` (#11)
- **SetScript contextual typing** — `SetScript` callbacks get event-specific parameter types based on the event string argument
- **Bracket-access nil narrowing** — `if tbl["key"]` nil checks now narrow the type in guarded branches

### Diagnostics
- **Detect malformed `@class` inheritance** — emits a diagnostic when `@class Foo Parent` is missing the colon before the parent name (#9)
- **Comma-separated `@return` hint** — emits a helpful diagnostic when `@return` uses commas instead of separate `@return` lines

## Bug Fixes

- Fix `WorldFrame` not inheriting from `Frame` (#5)
- Fix `type()` guard not removing `number` from union types (#16)
- Fix incorrect for-loop variable types when iterating with `next` (#8)
- Fix cross-file outgoing calls in call hierarchy (#13)
- Suppress diagnostics on stub files (#12)
- Fix wrong type after string concat on addon namespace fields
- Fix class field getting narrowed to `table` cross-file
- Fix wrong type after and-chaining in addon namespace
- Fix false positive flavor warning for field-guarded API calls
- Fix cross-file class definitions not merging for global assignments
- Fix `type-mismatch` false positive when callback renames `self` parameter
- Fix `malformed-annotation` false positive when `@return` description contains commas
- Fix `documentSymbol` LSP error (selectionRange not contained in fullRange)
- Fix intersection and `class-to-table<K,V>` type assignability false positives
- Fix go-to-definition for stub class fields and event locations

## Improvements

- Fix O(n³) union resolution for large alias types (significant performance improvement for addons with complex type unions)
