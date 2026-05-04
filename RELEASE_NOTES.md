# v0.8.0

## New

- **`shadowed-local` diagnostic** — warns (as a hint) when a local variable shadows a variable from an outer scope ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#shadowed-local))
- **`expression<C, R>` built-in type** — inline Lua expression strings with hover, completions, go-to-definition, and type-checking support ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/expressions.html))
- **Nested document symbols** — enables sticky scroll and a hierarchical outline view in VS Code (functions nested inside tables/classes appear under their parent)
- **Multi-line string folding** — long `[[ ... ]]` strings are now foldable in the editor

## Improvements

- Member-access completions now filter by the prefix you've already typed, reducing noise
- Hover for anonymous subtables now shows inline field types instead of just `table`

## Bug Fixes

- Fix several **completion** issues: event string completions no longer show non-event globals; string literal param completions no longer show all globals
- Fix **diagnostic false positives**: `@event` annotations no longer trigger `malformed-annotation`; `nil-index` no longer fires on and-expression RHS values; `redefined-local` no longer fires on `local function` declarations or inherits the old definition's type
- Fix **type inference**: cross-file `params<F>` projection, backward param inference through function aliases, trailing `?` on `fun()` param annotations
- Fix **navigation**: go-to-definition for a local that shadows a global with the same name; duplicate reference entries in local assignment RHS
- Fix **inlay hints** for method calls on class fields
- Fix **folding ranges** not working correctly in some cases
