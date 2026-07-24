### Improvements

- The "overrides" code lens now navigates via the editor's built-in go-to-locations action (VS Code and JetBrains) instead of internal LSP4IJ API — more robust across IDE versions.

### Bug Fixes

- Improved cross-file type inference for Ace3 addons: `self.db` (AceDB) from a chained `LibStub(...):New()` call is now typed correctly across files (including member completion), `LibStub("Lib"):Method()` field types resolve during cross-file scanning, and `NewAddon` / `NewModule` mixin embedding works with chained `LibStub` calls.
- Reading a name before a same-name local declared later in the file now resolves to the intended global/external — fixing hover, go-to-definition, and signature help, and no longer suppressing `undefined-global` for a genuinely missing global.
- Fixed generated stubs leaking an undeclared type variable that made functions like `ipairs_reverse` report `never` / `any?` at their use sites.
