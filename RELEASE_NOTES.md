### New

- `@deprecated` symbols now show a deprecation notice (including any custom message) in hover and completion, and are struck through at call sites ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html))

### Bug Fixes

- Generic `@overload` returns of parameterized classes no longer drop their type arguments.
- Removed duplicated hover text for inherited methods on union-typed receivers.
- `@enum` whose member values are variables no longer resolves to `any`.
- Multi-addon workspaces no longer leak namespace sub-table fields (such as `self.db`) between addons.
- Cross-file type inference now preserves local-table field shapes and array/map element types instead of collapsing them to a bare `table`.
