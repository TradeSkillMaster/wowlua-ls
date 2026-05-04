# v0.7.0

## New

- **`cannot-call` diagnostic** — Warns when calling a value whose type is not callable (e.g. number, string, boolean)
- **`nil-index` diagnostic** — Warns when using a possibly-nil value as a table key (#19)
- **`field-type-mismatch` for table constructors** — Type-checks fields in table constructor literals against `@field` annotations
- **`__call` signature in hover** — Tables with a `__call` metamethod now show the call signature on hover
- **`dump-types` subcommand** — Dumps hover types for every identifier in a workspace, useful for regression baselines
- **HookScript support** — `HookScript` calls are now treated like `SetScript` for event payload resolution

## Bug Fixes

- Fix `_G.X` hover and completion for global aliases
- Fix `@builds-field` text filter missing wrapper functions on `didOpen`
- Fix `malformed-annotation` diagnostic incorrectly spanning entire function body
- Fix `grouped-return-mismatch` false positive after nil-guard early exit
- Fix `inject-field` false positive when field name matches a WoW class name (#20)
- Fix cross-file flavor narrowing inside `if` blocks (#17)
- Fix repeated/bogus parameter name inlay hints
- Fix list type inference ignoring nil assignments (#18)
- Fix types not resolving for or-chained function calls (#23)
- Fix undefined type warning for aliases defined in `@meta` files (#22)
- Fix chained defclass method call type resolution
- Fix false `stringlib` warnings on real addons
- Fix false `type-mismatch` for or-function field assignments
- Fix false positive `type-mismatch` on variadic overload
- Fix code lens "N usages" click error in VS Code
- Fix AceAddon-3.0 class methods not resolving via LibStub
- Resolve cross-file function call types through addon namespace aliases
- Merge addon namespace sub-table methods into class tables

## Improvements

- `params<F>`/`returns<F>` projections now resolve inside inline `fun()` type expressions
- Suppress inlay type hints for discard variable `_`
- Improve `grouped-return-mismatch` diagnostic message clarity
- Build universal VSIX instead of per-platform packages
- Support external stubs via `embedded-stubs` feature flag (for shared plugin packages)
- Improve JetBrains LSP completion performance and compatibility
- Publish JetBrains plugin to marketplace with universal binary ZIP
