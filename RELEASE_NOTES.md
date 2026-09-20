**New**

- `ExcludeLoadGameType` TOC support — both the `## ExcludeLoadGameType:` header and the per-line `[ExcludeLoadGameType ...]` directive now restrict which flavors a file loads on, with hover docs and value validation in the TOC editor. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/toc-files.html))
- Comparing against a boolean literal narrows a union: after `if id == false then return end`, a `false|number` value is just `number`. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/nil-safety.html#literal-equality))

**Bug Fixes**

- Space-separated game-type lists (`[AllowLoadGameType tbc wrath]`) were ignored and flagged as an unknown value.
- Literal-equality guards no longer narrow reads before the guard (false `type-mismatch` above an early-exit check).
- Type-guard narrowing inside an `if` branch no longer leaks past the branch.
- JetBrains: double-clicking inside a Lua code fence in a Markdown file selected the whole block (IntelliJ 2026.2).
- Double-click and expand-selection used pre-edit text when a file had unsaved changes.
