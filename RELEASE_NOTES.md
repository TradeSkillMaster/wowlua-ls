**Bug Fixes**
- Fixed a false `undefined-field` warning on methods defined on a `@class`-typed local inside a function body.
- Fixed cross-file `@class` field types and diagnostics going stale after multi-file edits in IntelliJ (previously needed a close/reopen to refresh).
