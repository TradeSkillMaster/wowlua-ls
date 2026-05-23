### New

- **`nil-table-key` diagnostic** — warns when a table key type annotation includes nil, e.g. `table<string?, V>` ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/diagnostics.html#nil-table-key))
- **`library` config field** — mark directories as libraries: types are imported but diagnostics are suppressed. Supports relative and absolute paths ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/configuration.html#library))
- **`codeLens` config** — granular control to disable "N usages", "N implementations", and "overrides Parent" code lenses ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/configuration.html#codelens))
- **`@defclass` through overload dispatch** — `@defclass` now works on functions with `@overload` annotations, enabling typed class factories like AceAddon's `NewAddon`/`GetAddon` ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html))
- **AceAddon-3.0 stub overrides** — built-in type definitions for `LibStub("AceAddon-3.0")`, including `NewAddon` and `GetAddon` with `@defclass` support

### Bug Fixes

- Fix stale, duplicate, and lagging diagnostics across Neovim (pull-model) and other push-only LSP clients
- Fix inlay hints flickering during typing
- Fix `undefined-field` false positive on multi-assignment `@class` methods
- Fix `pairs`/`ipairs` iteration types: strip nil from key and value types, remove duplicate `string[]|string[]` in `ipairs`
- Fix annotation type highlighting for dashed names (e.g. `@class my-addon`)

### Improvements

- Narrow stub generation dependencies on the Ketho repo for faster and more resilient builds
