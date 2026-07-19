### New

- `@meta` files can now **override a built-in stub method**: an annotated declaration in a `@meta` file that reuses a built-in class + method name replaces the stub's signature in hover, signature help, and completion (go-to-definition still offers both sites). This is how you correct or refine a bundled library annotation. ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html#meta-and-overriding-built-in-stubs))
- `undefined-field` now flags reads of nonexistent fields on `string`-typed values (e.g. `("x").foo`), checked against the `string` library. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#undefined-field))

### Improvements

- Faster diagnostics on large workspaces.
- Better type inference for Ace3 addons: `AceAddon:NewAddon` / `NewModule` now embed the methods of mixin libraries passed as varargs (e.g. `"AceEvent-3.0"`) into the returned addon object.
- `AceDB:New` now types its `defaults` into the returned database object, so profile/global field access resolves correctly.

### Bug Fixes

- Fixed an IntelliJ freeze on project load caused by not draining stdin during the initial workspace scan.
- Fixed a "missing command" error when clicking a code lens ("N usages", "overrides X") in IntelliJ.
- Fixed a `@class` self-field assigned from a chained call being typed as `any` in hover and completion.
- Fixed a chained-receiver method call being mis-typed as a same-named global during cross-file scanning.
- Restored string-method completion on non-literal receivers.
