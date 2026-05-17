**Bug Fixes**

- Fix `undefined-field` false positive on chained overlay assignments (e.g. `self.x = self.x or default` across files)
- Fix spurious `inject-field` on nested `@class` declarations inside table constructors
- Fix false positive `unused-local` when a variable is only used inside an or-expression call argument
- Fix false positive `type-mismatch` for parameters named `_` (underscore discard convention)
- Fix stale diagnostics persisting after edits until the next file save
- Skip packager placeholder tokens (e.g. `@project-version@`) in TOC `Interface` field validation

**Improvements**

- Propagate untyped self-field assignments cross-file — fields set via `self.x = value` in one file are now visible in other files even without explicit `@field` annotations
- Push diagnostics on every change for editors that don't support `workspace/diagnostics/refresh` (improves Neovim compatibility)
- `check` command now prints summary statistics (file count, diagnostic totals by severity) at the end of output ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/cli.html))

**Docs**

- Add Neovim setup guide covering native LSP client, LazyVim, and nvim-lspconfig ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/getting-started.html))
