### Bug Fixes
- Fix `@deprecated` not detected on `self:Method()` calls — the deprecated diagnostic now fires correctly for colon-syntax method invocations on `self`.
- Fix stale `undefined-type` warnings persisting after adding an `@event` annotation until the file was re-saved.
- Fix doc generation missing methods on class-typed globals (e.g. globals annotated as `@type ClassName` now include the class's methods in generated docs).

### Improvements
- Add `Param.nilable` to the plugin API, allowing plugins to check whether a function parameter is optional. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/plugins.html))
