### Bug Fixes

- Untyped varargs (`...`) now render as a bare `...` in hovers, instead of a dangling `...: ` with a trailing colon and no type.
- Fixed go-to-definition on symbols within stub files in IntelliJ.
- Member completion now works on a bracket-indexed receiver — e.g. `x[i]:method` / `x[i].field` — which previously offered no completions.
