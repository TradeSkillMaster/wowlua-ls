### Improvements

- Cross-file `@class` fields now resolve to their real types. A runtime `self.x = …` field on a `@class` that previously showed as `any` when read from another file now hovers and completes with its definition-site type — class instances, primitives, and optionals (with nilability preserved) — so methods resolve on it and spurious diagnostics no longer fire.
- Factory functions now carry their return shape across files: a function returning a frame with injected per-instance fields, or an anonymous record, resolves those fields precisely in other files instead of decaying to `any` or a bare `table`.

### Bug Fixes

- `need-check-nil` no longer reports a false positive on deep access into a section inherited as optional from a parent `@class` (such as an AceDB schema-backed defaults table).
- Hover, completion, and go-to-definition now resolve a method called directly on the result of a generic function call (e.g. `pass(pw):Render()`), which previously resolved to nothing.
