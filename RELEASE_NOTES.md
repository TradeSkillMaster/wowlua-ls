### New

- **Cross-file factory functions** now carry precise per-instance field shapes. When a factory in one file injects fields onto the object it returns, those fields hover and complete with their exact types in other files (no more false `undefined-field`), and go-to-definition on an injected field jumps to its assignment in the factory's file.
- **Annotation-integrity diagnostics now run in `@meta` files.** Malformed, misplaced, or dangling annotations in declaration-only stubs are now caught — undefined type/class references, `@field`/`@param` not attached to a `@class`/function, invalid `@diagnostic` codes, and `nil` table-key types — while runtime/behavior diagnostics stay suppressed. ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html))

### Bug Fixes

- WoW API fields and return values typed as `vector2`/`vector3` now resolve to `Vector2DMixin`/`Vector3DMixin` with typed `x`/`y` fields and their methods, so method calls on them no longer report false `undefined-field`.
- Fixed a false `unbalanced-assignments` warning that fired when a function's `@return` type couldn't be resolved.
