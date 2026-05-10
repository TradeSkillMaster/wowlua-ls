### Bug Fixes

- Fix false `undefined-field` diagnostics on fields accessed through cross-file class return types (e.g. a factory function in one file returning a class whose fields are accessed in another)
- Fix unresolved generic type parameters displaying as raw `T` in `CreateFrame` hover tooltips instead of the concrete frame type
- Fix FrameXML font globals (`GameFontNormal`, etc.) not resolving to their proper `Font` type

### New

- Add JSON Schema for `.wowluarc.json` — editors that support JSON Schema (e.g. VS Code) now provide autocompletion, validation, and inline documentation for project configuration ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/configuration.html))
