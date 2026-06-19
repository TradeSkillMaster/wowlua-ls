### Bug Fixes

- Function-typed aliases (e.g. `---@alias Callback fun(link: string, qty: number): boolean`) now keep their full signature on hover everywhere — in parameters, return types, containers (arrays/maps of the alias), cross-file usages, and `@event` payload fields — instead of decaying to a bare `function`.

### Improvements

- Improved syntax highlighting: method definition headers now color their class / accessor / method segments, `@alias` declaration names are colored like class names, and boolean constants (`true`/`false`) are colored consistently inside `expression<…>` strings.
- `check --severity hint` now exits non-zero when hints are found, so hint-level issues can fail CI.
