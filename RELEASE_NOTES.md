**New**

- Lua diagnostic plugin system — write custom file-level diagnostics in Lua via `.wowluarc.json` plugins, with access to local variables, field reads/writes, method calls, and initializer analysis ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/plugins.html))
- `doc` subcommand — generate VitePress-compatible markdown API documentation from `@class` definitions in your addon ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/cli.html))

**Bug Fixes**

- Fix recursive self-type expansion causing infinite loops in hover

**Improvements**

- Simplify anonymous table inlay hints to show `table` instead of expanding inline fields
