**New**

- `---@type T1, T2` types each target of a multi-target assignment (`local a, b = ...`) in order ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/basic-annotations.html#type-variable-types))
- `self` in a `@field` type refers to the declaring class ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/classes.html#referring-to-the-class-with-self))

**Improvements**

- The opt-in `unknown-*-type` diagnostics now flag every value that `check`'s type coverage counts as untyped, including `any`-annotated values, `_`/`self` params and globals.

**Bug Fixes**

- Each repeated `_` parameter in a callback now gets the type for its own position.
- Fixed narrowed types going stale or leaking into early-exit branches, reassignments and `or` guard chains (false `need-check-nil`/`redundant-condition`).
- Rename and find-references on table-constructor keys (`{ key = ... }`) now reach the field's dotted uses instead of a same-named local.
- FrameXML functions annotated in the vendor stubs lost their parameter types and docs (e.g. `MenuUtil.CreateContextMenu`).
- `#` description markers in `@param`/`@return` appeared in hover text.
- Fields resolved to `any` across files (fields assigned in a base class constructor, call-valued table-constructor entries, `t.f = t.f or Create()`).
- Generic binding through intersection-typed params (`V[] & {[K]: V}`) widened `K`.
- `undefined-doc-name` missed undefined types in inline `@type` on table-constructor entries.
