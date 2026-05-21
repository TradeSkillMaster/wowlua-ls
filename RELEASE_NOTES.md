### New
- **Enum literal values in hover** — Hovering over an `@enum` field now shows its literal value (e.g. `= 1`, `= "WARRIOR"`)
- **Dashed type names** — Type names containing dashes (e.g. `Auction-Item-Info`) are now recognized in hover and go-to-definition

### Bug Fixes
- Recognize negative number literals in hover/definition ([#54](https://github.com/tradeskillmaster/wowlua-ls/issues/54))
- Fix `undefined-field` false positives for cross-file method definitions ([#53](https://github.com/tradeskillmaster/wowlua-ls/issues/53))
- Fix hover/definition for methods chained on function call return values (e.g. `GetFrame():SetPoint(...)`)
- Fix class field doc syntax highlighting in hover markdown ([#52](https://github.com/tradeskillmaster/wowlua-ls/issues/52))
- Fix missing tooltip for string methods on variables (e.g. `s:upper()`) ([#51](https://github.com/tradeskillmaster/wowlua-ls/issues/51))
- Fix `.toc` files being parsed as Lua when not directly opened in the editor
- Fix stack overflow from recursive types in type formatting
- Fix `@builds-field` losing `fun()` types and array type collapse
- Fix `@defclass` lateinit fields losing parameterized table types
- Fix string literal hover for escaped quotes and long bracket strings
- Fix ternary idiom (`x and y or z`) nil-check suppression on the or-branch
- Fix field tracking for table literals assigned to `self` fields

### Improvements
- Pull GlobalStrings from wago.tools DB2 for more complete coverage
- Parse Blizzard `APIDocumentationGenerated` directly for stub generation
- Add `vanilla`/`tbc` to `AllowLoadGameType` completions
