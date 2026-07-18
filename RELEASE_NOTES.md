### Bug Fixes

- Namespace fields with the same name but different `@class` types no longer leak across separate addon roots, clearing spurious `field-type-mismatch` and `undefined-field` diagnostics.
- In JetBrains IDEs, double-clicking a name inside an annotation comment (e.g. `---@class Foo`) now selects just the word instead of the whole comment line.

### Improvements

- `.wowluarc.json` multi-word config keys are now camelCase (`addonRoot`, `backwardParamTypes`, `correlatedReturnOverloads`, `implicitProtectedPrefix`, `allowSlashCommands`, `allowBindingGlobals`). The old snake_case spellings still work but are deprecated and will be removed in a future version.
