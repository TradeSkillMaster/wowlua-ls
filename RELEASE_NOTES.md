### New

- `@correlated` annotation now supports local variables, not just class fields ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/nil-safety.html))
- `need-check-nil` now warns on `#` applied to possibly-nil values ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/nil-safety.html))
- `invalid-op` now catches ordered comparisons (`<`, `>`, `<=`, `>=`) on nil and incompatible types ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#invalid-op))
- Warn on `@cast` to an unknown type name ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/basic-annotations.html))
- XML `parentKey` fields now resolve on mixin classes
- Backward param inference now infers `table` from bracket-index usage and `string|table` from `#` operator

### Bug Fixes

- Fix several type narrowing edge cases: `== nil` then-branch for simple variables, string literal `==` now narrows to the exact literal type, `@cast -Type` now respects enum types, compound guard sibling narrowing, correlated narrowing no longer ignores nil reassignment, optional field truthiness no longer narrows too aggressively
- Fix false positives: `cannot-call` on callable class through table field, `type-mismatch` from `#` backward inference conflicting with array usage, `field-type-mismatch` on table constructors with per-field `@type`, bracket-access type inference
- Fix cross-file `@enum` field not visible on class
- Fix generic function return types dropped from hover
- Fix cross-file self-field visibility in class hover
- Fix `modifierOffset`-style type inference showing `?` instead of the resolved type
- Fix inlay hints jumping on line deletion
- Fix find-references missing RHS shadow-before-use matches
- Fix `count-down-loop` not warning on zero-step equal-bounds loops
- Fix `invalid-op` not flagging boolean concatenation
- Fix XML files parsed as Lua producing false diagnostics

### Improvements

- Pre-build defclass/built-name lookup tables for faster workspace scans
