### New

- **Inline TOC directives.** `.toc` file lines now accept load conditions (`[AllowLoadGameType]`, `[AllowLoadTextLocale]`, `[AllowLoad]`) and path variables (`[Family]`, `[Game]`, `[TextLocale]`) in either prefix **or** suffix position — matching the WoW client's documented suffix form (`Retail/File.lua [AllowLoadGameType mainline]`). Only `[AllowLoadGameType]` affects flavor filtering; the other conditions are recognized and stripped. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/toc-files.html#per-line-directives))
- **String-enum alias value completion.** For an open string-enum alias (`---@alias Unit string` with `---|"player"` value lines), completion now offers the enumerated values inside a string argument typed with the alias, while still accepting any string.

### Improvements

- Go-to-definition on a method that a `library` redefines now lists **every** definition site (the built-in stub plus each workspace redefinition), matching the multi-site behavior already used for globals, classes, and aliases.

### Bug Fixes

- Fixed a bare forward-declared local (`local x`) incorrectly absorbing a `---@type` annotation on the **next** line, which mis-typed the variable. A same-line trailing `---@type` still applies correctly.
- The VS Code extension now activates as soon as you open a project containing Lua or `.toc` files, so workspace-wide diagnostics appear in the Problems panel without needing to open a file first.
