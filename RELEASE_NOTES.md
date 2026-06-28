### New

- **CallbackRegistryMixin support** — `:GenerateCallbackEvents(...)` now synthesizes the `.Event` enum table on the receiver class, with event-name completion and a new (off-by-default) `unknown-callback-event` diagnostic for events that were never registered ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html)).
- **`@shape` annotation** — declare the plain-table forms a `@class` accepts (userdata/mixin escapes like `ItemLocation` or `ColorMixin`), so a matching literal is assignable even without the class's methods ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html#shape-accept-plain-table-forms)).
- **`@returns-class-name` narrowing** — comparing a method's string result against a class-name literal (e.g. `region:GetObjectType() == "FontString"`) now narrows the receiver to that class ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/type-guards.html#returns-class-name)).
- **`missing-param-annotation` / `missing-return-annotation` diagnostics** (off by default) — flag non-file-local functions missing `@param`/`@return`, scoped to functions that actually escape their file ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/diagnostics.html)).
- **`class-shadows-builtin` diagnostic** — warn when a workspace `@class` that declares its own `@field`s reuses a built-in WoW API class name ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/diagnostics.html)).
- **LuaLS-compatible annotation forms** — comma-separated `@return T1, T2`, `[T1, T2]` tuple syntax, the `?optional` prefix shorthand, and additional LuaLS `@diagnostic` codes are now accepted ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html)).
- **Bundled Ace3 types** — `AceAddon-3.0` instance methods, the `AceModule` type for `:NewModule`/`:GetModule`, and a non-nil return for default-locale `AceLocale` `NewLocale`.
- **Multi-result go-to-definition** — go-to-definition and go-to-type-definition now list every site when a global or type is defined across multiple files.
- **`completion.callSnippets` option** — disable auto-filling a function's parameters on completion ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/configuration.html#completion-callsnippets)).

### Improvements

- **Mixin typing** — `Mixin()`'d fields now resolve the mixin's methods, the original frame type is combined with the applied mixin, and derived mixins (`CreateFromMixins`, XML `parentKey`) are typed as their own class.
- **Cross-file field tracking** — self-fields assigned in method bodies, fields assigned a bare local, and injected frame fields now carry across files; overlay fields on `param = param or CreateFrame()` frames are tracked too.
- **XML widget handling** — recognize `ItemButton`/`EventFrame` intrinsics, auto-allow XML-bound mixin and handler globals, type XML mixin `self` as a frame, and refresh XML-bound names when `.xml` files change.
- **Flavor-aware `deprecated`** — a retail-only deprecation is no longer flagged on an addon that still targets a flavor where the API remains live.
- **Broader built-in stub coverage** — discover missing FrameXML globals/fields from real sources and restore bare globals dropped by stub discovery, reducing false `undefined-global`.
- **External library directories** — `library` paths that escape the workspace root (e.g. `../shared`) are now scanned for types.
- **`.github/` skipped by default** during scanning (CI/build Lua, not in-game addon code).
- **Faster `check` + responsive code lens** — per-file analysis is parallelized, and cross-file analyses are cached so code-lens "N usages" resolves quickly.
- **Progress reporting** for background diagnostic re-checks.

### Bug Fixes

- Fixed an LSP crash on multibyte (non-ASCII) characters in code actions.
- Eliminated several `redundant-parameter`/`missing-parameter` false positives — on undocumented widget methods, union-typed receivers, flavor-split functions with differing arities, and wrong-arity built-in stubs.
- Suppressed `undefined-field` false positives on defensive membership-test reads (`if t.x then ...`) and on namespace fields assigned inside functions.
- Fixed `undefined-global` false positive for globals created inside function bodies.
- Fixed `cannot-call` false positive on forwarded namespace fields.
- Fixed `create-global` false positive on field writes through a parenthesized/prefix expression.
- Fixed `unbalanced-assignments` false positive when destructuring dynamic multi-returns.
- Fixed `field-type-mismatch` false positive on scan-placeholder self-fields.
- Fixed `return-mismatch` false positive for arrays compared against `{[1]: T}` literals.
- Fixed `undefined-doc-name` false positive on `@alias` with a trailing `#` comment.
- Fixed a `LibDBIcon:Register` false positive on partial DB tables, plus a `pcall` sibling-narrow type leak (and widened LibSharedMedia typing).
- Fixed mixin self-field misattribution on nested colon methods.
- Stopped a placeholder `function() end` from masking callback arity checks.
- Fixed a spurious table-shape union in namespace field hover.
- Narrow a field after `field = field or X` for indexed writes.
- **JetBrains/IntelliJ:** dropped the plugin's until-build cap so it survives IDE upgrades, and fixed go-to-definition inside opened stub files.

### Docs

- Aligned the configuration guide with the reference (`library` field) and fixed outdated JetBrains install instructions in the README.
