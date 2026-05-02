# v0.5.1

### New
- **Typed dictionary classes** — `@class Foo : table<K, V>` now inherits key/value types, enabling typed `pairs()` iteration over class-typed tables.
- **Go-to-definition for event strings** — Ctrl+click on event name strings (e.g. `"CHAT_MSG_ADDON"`) jumps to the event's `@event` declaration.

### Bug Fixes
- Fix `for k, v in next, tbl` not correctly typing loop variables when the iterator is a multi-expression list.
- Fix `__call` metamethod always treating its first parameter as an implicit `self` receiver, even for non-method calls.
- Fix backtick generic inference (`` `T` ``) failing when the argument was a variable or a cross-file sub-field assignment.
- Fix `@field` annotations using `table<K, V>` resolving to an untyped table in cross-file classes.
- Fix `_G.field` assignments not being recognized as global variable definitions.
- Fix missing wiki documentation links in event hover tooltips.
