# v0.3.0

### New
- **JetBrains IDE plugin** — wowlua_ls now works in IntelliJ IDEA, CLion, and other JetBrains IDEs via a bundled LSP plugin
- **`doc-func-no-function` diagnostic** — warns when function-level annotations (`@param`, `@return`, `@overload`, etc.) aren't attached to a function definition
- **SavedVariables auto-discovery** — `.toc` file `SavedVariables` and `SavedVariablesPerCharacter` entries are automatically treated as allowed globals, suppressing false `undefined-global` diagnostics
- **AceGUI widget type definitions** — `AceGUI:Create()` now returns properly typed widget objects based on the widget type string

### Bug Fixes
- Fix overload optional-param arity checks — calls with fewer args than a non-optional overload param no longer incorrectly match; also fix `debugstack()` typing
- Fix `.wowluarc.json` config changes not taking effect without a full VS Code reload
- Fix phantom class creation in workspace scanner — locals with `@type ClassName` no longer create spurious empty class declarations that shadow real ones
- Fix `wrong-flavor-api` false positives on nil-guarded (`if API then API()`) and locally-shadowed calls
- Fix implicit globals inside nested blocks (if/for/while) being scoped to the block instead of file scope
- Fix `create-global` diagnostic being suppressed when the global was discovered by workspace scanning
- Fix false positive `undefined-field` on `_G.fieldname` for well-known globals
- Fix parameter type-checking for intersection types (`T & fun(...)`) — callable intersections now match function-typed params
- Fix false positive `type-mismatch` when passing one plain table where another plain table is expected
- Fix `__call` metamethod return type inference not picking up return types from the metamethod body

### Improvements
- Skip files with shebang lines (`#!/...`) from scanning and analysis
