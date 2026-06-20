### New

- **Constrained type parameters in `@alias` generics** — parameterized aliases can now declare and enforce constraints on their type parameters, e.g. `@alias Box<T: Frame> { value: T }`. Each type argument is checked against its constraint at every use site. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/generics.html))
- **`undefined-field` now flags module-private record tables** — accessing a nonexistent field on a `local X = {}` whose complete field set is statically known is reported, extending the diagnostic beyond `@class` tables. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#undefined-field))
- **`type-mismatch` now flags string-literal-union arguments** — passing a string that isn't one of the allowed literals in a string-literal-union parameter type is reported. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#type-mismatch))

### Improvements

- Nilable values are now narrowed through `assert(not x or y)` implications inside and-guards.
- Inherited protected methods now appear in completion on defclass instances.

### Bug Fixes

- Fixed an incorrect "unknown type" report on a forward-declared local with a trailing `@type` annotation.
- Suppressed unknown-type warnings for throwaway `_` names.
- Recovered cross-file generic type arguments in `expression<C, R>` string contexts.
- Fixed false positives on constrained generics and builder-built types.
- Fixed class-typed field types being lost when assigned to a local.
- Fixed a nested bracket-write polluting the outer table's element type.
