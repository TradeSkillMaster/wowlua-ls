### New

- **IsObjectType() narrowing** — calling `frame:IsObjectType("Button")` in an `if` guard now narrows the frame variable to the `Button` subclass in the then-branch ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/type-guards.html))
- **hooksecurefunc callback inference** — `hooksecurefunc` callbacks automatically receive parameter types from the hooked function's signature
- **`@param` on call statement callbacks** — `@param` annotations placed above a call statement apply to the callback argument, giving you named typed parameters without a separate function definition
- **Hover for string literal method calls** — hovering `("hello"):upper()` or `str:format(...)` on a string literal now shows the resolved method signature
- **Addon folder name inference** — the first file-level vararg (`addonName`) is inferred as the string-literal addon folder name (e.g. `"MyAddon"`)
- **Intersection type hover expansion** — fields from intersection types (`A & B`) are now displayed vertically in hover, matching the class field layout

### Bug Fixes

- Fix duplicate diagnostics and stale parse errors in Neovim
- Fix `undefined-global` not catching typos in bracket access (e.g. `_G["typo"]`)
- Fix `type-mismatch` false positive when `@return` annotation exists
- Fix `field-type-mismatch` false positive for class assigned to a table field
- Fix `@type` on `self.field` not resolving during workspace scan
- Fix addon namespace class name being lost during generic binding
- Fix cross-file scan dropping non-literal table fields
- Fix hover display for variadic generic return types
- Fix backtick generic resolving to string when the argument is a variable
- Fix deep dot chains on global non-class tables producing wrong hover
- Fix `@event` / `@param` diagnostics and standalone `params<EventType>` resolution
- Suppress diagnostics in stub files opened via go-to-definition

### Improvements

- Replace Ketho Wiki/Enum/CVar stubs with primary Blizzard sources for more accurate and complete data
- Add missing FrameXML color constants to stubs

### Docs

- Move stub generation gist to a docs page
