### Bug Fixes

- Fix while-loop exit narrowing for AND conditions — `while a and b` now correctly narrows types on loop exit
- Fix nil type for globals assigned from a method call inside a callback
- Suppress stub file diagnostics in the VS Code Problems panel ([#50](https://github.com/tradeskillmaster/wowlua-ls/issues/50))
- Fix cross-file multi-path return type dropping non-nil values
- Fix cross-file self-field visibility on class hover

### Improvements

- Pre-warm workspace diagnostic cache on startup to prevent UI freezes when opening files
