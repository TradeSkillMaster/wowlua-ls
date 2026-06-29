### Bug Fixes

- Fixed classic-only WoW constants being falsely flagged as `undefined-global`.
- Fixed legacy spell APIs with multiple signatures (e.g. `GetSpellInfo`, `IsSpellInRange`) generating incorrect overloads from the wiki.
- Fixed an `inject-field` false positive on union receivers.
- Fixed a false `type-mismatch` on the `_G.API or function() ... end` polyfill idiom.
- Fixed an `unknown-local-type` false positive on forward-declared locals.
- Fixed a spurious `create-global` diagnostic on table fields following a missing comma.
- Fixed inline `---@diagnostic disable-line` not working on annotation comment lines.
- Fixed code folding ranges drifting after adding or removing lines.
- Fixed table-constructor keys being colored as global functions.
- Fixed a Lua plugin comparison range that spanned the whole file instead of the matched region.
- Fixed an internal-API usage in the JetBrains plugin flagged by the plugin verifier.

### Improvements

- Mixin/userdata-object parameters now automatically accept a plain data table (e.g. passing `{ r = 1, g = 0, b = 0 }` where a `ColorMixin` is expected) while still rejecting unrelated tables. **The `@shape` annotation has been removed** — this behavior is now automatic, so `@shape` declarations are no longer needed.
- Heterogeneous array literals now infer a common supertype for element checks, reducing false positives on mixed-type arrays.
- Added stubs for FrameXML symbols missing from the published source, improving API coverage.
- Unknown scanned fields are now typed as `any` instead of `table`, reducing false positives.
- Improved code folding closers per editor.
