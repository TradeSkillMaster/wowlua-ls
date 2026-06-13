### New

- The `unused-function` hint now works **across files** — it flags functions and methods that are never referenced anywhere in the workspace, not just within a single file. It is now **opt-in** (disabled by default). ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#unused-function))
- Support for the `keyof self` generic constraint, so a method can constrain a type parameter to its receiver's field names without declaring a separate generic. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/generics.html#keyof-self))
- `need-check-nil` now also flags bracket access (`t[k]`) on possibly-nil tables, not just dot/method access. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#need-check-nil))
- Automatic detection of dynamically-created globals via `_G["PREFIX" .. key]` (and `_G[name .. "SUFFIX"]`) patterns — reads of those globals no longer report `undefined-global`. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/configuration.html))
- `BINDING_HEADER_*` and `BINDING_NAME_*` keybinding globals are now auto-allowed, matching the existing `SLASH_*` handling. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/configuration.html))
- Completions for annotation content — `@cast` types, `@diagnostic` codes, `@correlated`, and `@class` parent names.

### Improvements

- Better cross-file type inference: method return types now propagate across files, function signatures survive cross-file without losing parameter detail, and `type-mismatch` now validates arguments against cross-file `fun()` parameter annotations.
- Backward parameter-type inference from typed call arguments — an unannotated parameter can now take its type from the typed values that flow into the function.
- Expanded WoW API stub coverage: `AnchorUtil` and FrameXML utilities, additional previously-missing API methods, runtime UI fields discovered from the WoW UI source, `CreateFrame`/`CreateFontFamily` named globals, and flavor masks for XML frame globals.
- The `redundant-condition` hint now also flags provably-constant conditions, alongside a substantial reduction in false positives across the `redundant-condition` and `redundant-and` hints.
- Faster `check` runs on large workspaces.
- Smoother editing on large projects — background re-analysis is now deferred and cancelled while you type to avoid input stalls.
- Inlay hints are hidden for `_`-named (discard) variables.

### Bug Fixes

- Fixed several type-narrowing issues: equality against a non-nil value now narrows to non-nil, and narrowing is no longer lost across for-/while-loop body reassignments, hoisted `and`/`or` sentinel patterns, or field assignments with a nilable right-hand side.
- Fixed find-references mixing up methods with same-named globals, and missing methods reached through chained calls or an out-of-scope accessor base.
- Fixed local-variable completion suggestions and exact-match tab-completion.
- Fixed hover: duplicate signatures, `@return self` return-type display, and function-value disambiguation in multi-return hover.
- Fixed `@cast` not applying when separated from its target by a blank line or a plain comment.
- Fixed type-assignability and inference bugs: function types with fewer parameters, function-parameter contravariance in `type-mismatch`, enum-member assignability to nominal enum types, bracket access on intersection types, `setmetatable` on union-typed arguments, `select()` overload matching, `boolean?` inference, and constructor fields incorrectly pinned to `nil`.
- Fixed stub signatures: `SetPoint` 3-arg form, `tonumber` with a base argument, and `TexturePool`/`FontStringPool` constructors.
- Fixed XML-scanned `FontString`/`Texture` losing their base type when using `inherits`.
- Fixed addon-namespace fields leaking onto parent tables, and `select(2, ...)` inside functions resolving to the addon namespace.
- Fixed `autoInsertEnd` not triggering inside nested blocks, and annotation-comment syntax coloring.
