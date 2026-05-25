### New

- **Go to Type Definition** — navigate from a variable to its `@class` or `@alias` declaration
- **Go to definition for `@as` and `@cast` types** — jump to the class/alias referenced in type assertions
- **Extract Function / Extract Variable refactorings** — code actions to extract selected code into a new function or variable
- **Fill missing fields quickfix** — code action to insert required fields when constructing a `@class` table
- **Generate annotations source action** — auto-generate `@param`/`@return` annotations for a function
- **Fix all in file** — batch-apply quickfixes for the same diagnostic code across an entire file
- **Highlight related** — highlights matching `return`/`break`/`end` control flow points when cursor is on one
- **Bidirectional rename** — renaming a function parameter also renames the corresponding `@param` annotation (and vice versa)
- **Batch `@event` declaration with `---|` syntax** — declare multiple event payloads in a single annotation block ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/events.html))
- **Plugin API: event declarations and dot-syntax** — plugins can query `@event` declarations and find dot-syntax definitions/calls ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/plugins.html))
- **`missing-fields` for nested `table<K,V>` constructors** — the diagnostic now fires inside nested table constructors, not just top-level ones ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#missing-fields))
- **Correlated variable narrowing through number literals** — `if x == 1 then` now narrows correlated locals that were assigned in the same branch

### Bug Fixes

- Fix `missing-fields` false positive on inherited class fields
- Fix `undefined-field` false positive on builder-chain methods
- Fix `type-mismatch` false positive on `@class` table passed to `fun(): T`
- Fix `duplicate-doc-alias` false positive on opaque aliases
- Fix `cannot-call` false positive on `table.insert`
- Fix `undefined-field` false positive for function-call constructor fields
- Fix `type-mismatch` false positive for string literals assigned to string enums
- Fix `ScriptRegion|Frame` not assignable to `Frame`
- Fix false-positive when local function leaked as cross-file global
- Fix LS stuck loading after go-to-definition on a stub symbol
- Fix for-in union table value inference
- Fix event name string literal completions (extra quote)
- Fix keyword completions appearing in non-keyword positions
- Fix hover syntax highlighting for second table field type
- Harden on-type `end`/`until` auto-insertion edge cases
- Harden table literal-key class matching edge cases

### Improvements

- Skip workspace rebuild when only byte offsets changed (faster incremental edits)
- Exhaustive diagnostic assertion checking in test harness
- Overhaul README and docs site home/why pages

### Stubs

- Fix Classic API stubs returning `any` instead of typed returns
- Parse Blizzard ScriptObject API docs to fix missing widget methods
- Fix missing `FramePool`/`ObjectPool`/`FramePoolCollection` methods
- Fix missing `Enum.ItemQuality` members
- Fix `SetOwner` stub to accept `Region`
- Fix missing FrameXML globals, fonts, and types (e.g. `FramePool`)
- Fix missing classic tooltip API methods
