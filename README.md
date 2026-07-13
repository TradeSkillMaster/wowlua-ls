# wowlua-ls

A language server for World of Warcraft addon development. Built specifically for WoW Lua, not a general-purpose Lua LS with WoW bolted on.

> [!NOTE]
> **wowlua-ls is in beta.** It's under active development and improving fast. If you run into issues, have feature requests, or want to contribute, join us on [Discord](https://discord.gg/XgqevqEqJK). Your feedback directly shapes the project.

## Why wowlua-ls

LuaLS is an excellent general-purpose Lua language server. But WoW addons aren't general-purpose Lua: the API is enormous, and the patterns every addon is built from (event handlers, mixins, XML-defined frames, multi-flavor code) are exactly the ones generic tooling can't see. wowlua-ls is built for them.

|  | LuaLS | wowlua-ls |
|---|---|---|
| WoW API (retail · classic · classic era) | Install a third-party stub addon | **9,000+ stubs built in**, zero setup |
| Event handler payloads | Untyped `...` | **1,000+ events** with fully typed `...` |
| XML frames & templates | Invisible to the language server | Scanned into typed classes and globals |
| `.toc` files | Unsupported | Hover, completion, go-to-def, diagnostics |
| Wrong-flavor API calls | Not detected | Flagged with `wrong-flavor-api` |
| Mixins & templates | Annotate by hand | `CreateFrame` / `Mixin` infer `A & B` automatically |

And it goes well beyond stubs. The type engine understands the patterns addons are actually written in:

- **Metatable inference**: understands `setmetatable` + `__index`, chained metatables, `__call`, operator metamethods. Your OOP patterns just work without annotations.
- **Correlated narrowing**: check one return value, and the LS narrows the rest. Eliminates false positives from multi-return functions. Works automatically - no annotations needed in most cases.
- **Powerful generics**: parameterized classes, constrained type parameters, backtick factory annotations, function-type projections (`params<F>`, `returns<F>`). Class-level generics propagate through method calls automatically.
- **Builder pattern**: `@builds-field` tracks progressive type construction across chained method calls.
- **75+ diagnostics**: type safety, nil checking, annotation correctness, code quality, and WoW-specific checks. Each one individually configurable per-line or per-project.
- **Diagnostic plugins**: write custom Lua scripts to enforce project-specific conventions. Query local variables, field accesses, and method calls to emit your own diagnostics.
- **CI-ready CLI**: `wowlua_ls check path/to/addon` lints your addon and exits non-zero on diagnostics. Drop it into your CI pipeline.

Full feature list and a complete LuaLS comparison in the [documentation](https://tradeskillmaster.github.io/wowlua-ls/guide/why-wowlua-ls).

## Install

### VS Code

Install **wowlua-ls** from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=TradeSkillMaster.wowlua-ls). The extension bundles the language server binary - no separate install needed.

### JetBrains IDEs

Install **WoW Lua Language Server** from the [JetBrains Marketplace](https://plugins.jetbrains.com/plugin/31581-wow-lua-language-server) (or **Settings → Plugins → Marketplace**, search for "WoW Lua"). Works in any JetBrains IDE 2025.2 or newer; it uses the [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) plugin as its LSP client, which the Marketplace installs automatically as a dependency. The plugin bundles the language server binary - no separate install needed.

### Neovim

Neovim has built-in LSP support - no plugin required. Get the binary (download from [GitHub Releases](https://github.com/TradeSkillMaster/wowlua-ls/releases) or `cargo build --release`), then add to your config:

```lua
vim.lsp.config('wowlua_ls', {
  cmd = { '/path/to/wowlua_ls' },
  filetypes = { 'lua' },
  root_markers = { '.wowluarc.json', '.toc', '.git' },
  workspace_required = false,
})
vim.lsp.enable('wowlua_ls')
```

See the [Getting Started guide](https://tradeskillmaster.github.io/wowlua-ls/guide/getting-started) for LazyVim and nvim-lspconfig setup.

### Other editors

```bash
git clone https://github.com/TradeSkillMaster/wowlua-ls.git
cd wowlua-ls
cargo build --release
```

The binary is at `target/release/wowlua_ls`. Run it as an LSP server over stdio for Lua files.

## Quick start

Open a WoW addon folder. wowlua-ls automatically scans `.lua` files, loads WoW API stubs, and starts reporting diagnostics. No configuration required.

For project-specific settings, add a `.wowluarc.json`:

```json
{
  "ignore": ["Libs/"],
  "flavors": ["retail", "classic"],
  "diagnostics": {
    "enable": ["need-check-nil"]
  }
}
```

See the [Configuration guide](https://tradeskillmaster.github.io/wowlua-ls/guide/configuration) for all options.

## Features

Beyond the WoW-specific intelligence above, wowlua-ls is a complete language server:

### IDE features

Hover, go-to-definition (lists every site when a global or type is defined across multiple files), find references, rename, completions, signature help, semantic tokens, inlay hints (6 categories), code lens (usages, implementations, overrides), and call hierarchy.

### Type system

LuaLS-compatible annotations (`@param`, `@return`, `@class`, `@field`, `@generic`, `@overload`, etc.) plus WoW-specific extensions (`@event`, `@defclass`, `@builds-field`, `@type-narrows`, `@flavor-narrows`, and more). Full type system with unions, intersections, generics, parameterized classes, anonymous table shapes, optionals, lateinit, tuple-union returns, and opaque aliases.

See the [annotation reference](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations) and [diagnostic reference](https://tradeskillmaster.github.io/wowlua-ls/reference/diagnostics).

## CLI

Lint an addon from the command line:

```bash
wowlua_ls check path/to/addon
wowlua_ls check path/to/addon --severity hint
```

Exit code is `1` if any diagnostics are found - suitable for CI.

Generate API documentation (compatible with [sphinx-lua-ls](https://github.com/taminomara/sphinx-lua-ls)):

```bash
wowlua_ls --doc path/to/addon --doc_out_path path/to/output
```

See the [CLI guide](https://tradeskillmaster.github.io/wowlua-ls/guide/cli) for details.

## Documentation

Full documentation at **[tradeskillmaster.github.io/wowlua-ls](https://tradeskillmaster.github.io/wowlua-ls/)**

- [Getting Started](https://tradeskillmaster.github.io/wowlua-ls/guide/getting-started)
- [Annotation Guide](https://tradeskillmaster.github.io/wowlua-ls/guide/basic-annotations)
- [Configuration Reference](https://tradeskillmaster.github.io/wowlua-ls/reference/configuration)
- [Diagnostic Plugins](https://tradeskillmaster.github.io/wowlua-ls/guide/plugins)
- [Contributing](https://tradeskillmaster.github.io/wowlua-ls/guide/development)
- [Discord](https://discord.gg/XgqevqEqJK)

## License

GPL-3.0
