# v0.4.0

### New
- **`@flavor-narrows` on boolean variables and fields** — Boolean guards like `local isRetail = (WOW_PROJECT_ID == WOW_PROJECT_MAINLINE)` can now be annotated with `@flavor-narrows` to narrow flavor-specific API access in `if isRetail then` branches, including cross-file support for fields on addon namespace tables.

### Improvements
- **`create-global` promoted from hint to warning** — Accidental global creation is now surfaced more prominently.
- **`@class (partial)` and `@class (exact)` modifiers ignored** — These modifiers no longer trigger parse errors.

### Bug Fixes
- Fix `doc-func-no-function` false positives on `@class` annotation blocks and function fields inside table constructors.
- Fix `undefined-field` false positive on colon methods defined on locals typed via function return values.
- Fix `wrong-flavor-api` false positive on `and`-guarded field-access calls (e.g. `lib and lib.SomeClassicApi()`).
- Fix `create-global` false positive on field assignments to call return values (e.g. `GetFrame():field = val`).
- Fix `ipairs` over optional `@class` array fields resolving element type as unknown.
- Fix field narrowing not applying to bracket-access with string literal keys (e.g. `tbl["key"]` after a nil check).
- Fix narrowed/cast expressions not being resolved when accessed read-only (e.g. hover on a StripNil'd or `@as`-cast value).
- Fix hover showing file-scope local variables as `(global)`.
- Fix FrameXML addon namespace globals leaking into user addon namespace tables.
- Fix `self` type and `owner_class` resolution for methods defined with dotted base paths (e.g. `function a.b:Method()`).
