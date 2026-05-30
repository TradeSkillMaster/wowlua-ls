### Bug Fixes

- Fix event annotations missing from some globals whose class was defined through method definitions rather than an explicit `@class` stub
- Propagate nested `parentKey` fields from unnamed XML frames — child fields on unnamed intermediate frames now correctly appear on the parent
- Fix generic substitution in callbacks and parameterized parent diagnostics — resolves false positives when generic types flow through callback parameters or class inheritance with type params
- Fix `wrong-flavor-api` false positives for `GameTooltip` methods
- Fix language server process not exiting when the editor closes

### Docs

- Update editor plugin descriptions to match README
