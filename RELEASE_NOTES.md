**New**

- Support intersection types in `@class` parent position — e.g. `@class MyFrame : Frame & MyMixin` ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html))
- Smarter annotation auto-complete that suggests relevant annotations based on context — e.g. `@field` only appears after `@class`, `@return` only inside functions
- Show event type name in `OnEvent` handler parameter hover ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/events.html))
- Support string-valued `@enum` declarations and `mixed-enum-values` diagnostic for enums mixing number/string values ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/classes.html))
- Add hover and inlay hints for vararg parameters and event name comparisons
- Add `returns<F, index>` offset-aware projection for modeling functions like `select()` ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/generics.html))

**Bug Fixes**

- Fix bracket-access writes overriding addon namespace field type
- Fix code lens "N implementations" click action
- Fix sticky scroll for Lua files
- Fix `@type` annotation on field showing `table` instead of typed array
- Fix `@class` annotation hiding cross-file namespace fields
- Fix multiline string folding hiding last line

**Improvements**

- Warn when array type syntax is used in `@class` annotation position
- Reduce noisy info-level LSP logging to debug
