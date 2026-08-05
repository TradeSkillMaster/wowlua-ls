### New

- Quick fixes (code actions) are now offered for many diagnostics: declare an undefined/create-global as a `local` or add it to `.wowluarc.json`, add a missing `---@field`, fill required fields, add/remove arguments to match arity, wrap a flavor-specific API in a `WOW_PROJECT_ID` guard, insert an `--[[@as T]]` cast, and more. Every diagnostic also gains disable/suppress actions and a "Fix all in this file" batch action. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#quick-fixes))

### Bug Fixes

- IntelliJ go-to-definition no longer jumps to the top of the file for `_G.X` globals or on unresolvable tokens (operators, literals, `{}`).
- Double-clicking a name in IntelliJ no longer selects the entire `.toc` line.
- LSP refresh requests are now coalesced, fixing IntelliJ's "too many non-blocking read actions" overflow in large workspaces.
- Doc comments written in the spaced `--- @tag` style are now honored by every annotation scanner instead of being silently dropped.
- Reassigned table-constructor data fields no longer show a spurious `function & table` type.
- Files with a byte-order mark (BOM) no longer produce a spurious parse error or a line-0 position skew.
- Hover doc comments now preserve their line breaks.
