**Bug Fixes**
- Fixed class hover showing `any` for fields on a constructor annotated with `@class`.
- Fixed a false-positive `undefined-field` on global-namespace `@class` addon fields accessed from another file.
- Event handler parameters typed `table<K,V>` are no longer downgraded to bare `table` (#60).
- Fixed a regression that produced spurious return-type mismatches on boolean-literal fields.

**Improvements**
- Table fields assigned the wrong literal value are now flagged against literal-typed targets (e.g. a `false` where `---@field ok true` is expected) — previously the mismatch was masked by literal widening.
