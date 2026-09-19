# WoW Lua Language Server - Zed Extension

Connects [Zed](https://zed.dev) to the [wowlua_ls](https://github.com/TradeSkillMaster/wowlua-ls) language server.

## Features

All features come from the `wowlua_ls` language server:

- 9,000+ WoW API stubs built in (retail, classic, classic era)
- Event handler typing with per-event payload params
- XML frame scanning for frame definitions, virtual templates, and mixins
- TOC file support - hover, completions, go-to-definition, and diagnostics
- Metatable inference, correlated narrowing, mixin and template support
- Flavor filtering - warns on APIs unavailable in your target game version
- 80+ diagnostics for type safety, nil checking, annotation correctness, and WoW-specific checks
- Diagnostic plugins for project-specific conventions
- Powerful generics, builder patterns, signature help with overload resolution
- Code completion, go-to-definition, find references, rename, semantic tokens

## Companion extensions

This extension provides only the language server; the languages it attaches to come from elsewhere:

- **Lua** - install the [Lua](https://github.com/zed-extensions/lua) extension (Zed offers it the first time you open a `.lua` file). It supplies the `Lua` language and its Tree-sitter grammar.
- **WoW TOC** - install the [wow-toc](https://github.com/Alexayy/zed-wow-toc) extension to get `.toc` files analyzed. Without it Zed has no `WoW TOC` language, so `.toc` buffers are plain text and the server never sees them.

## Setup

### 1. Install the extension

Search for **WoW Lua Language Server** in Zed's extensions view (`zed: extensions`). To run it before it appears there - or while working on it - use **Install Dev Extension** and pick this directory instead (see [Development](#development)).

### 2. Open an addon

That's it. Open a WoW addon folder and the server starts, scans `.lua`/`.xml`/`.toc` files, loads the WoW API stubs, and begins reporting diagnostics. No configuration required; see the [configuration guide](https://tradeskillmaster.github.io/wowlua-ls/guide/configuration) for `.wowluarc.json` options.

## The server binary

The extension resolves `wowlua_ls` in this order:

1. `lsp.wowlua-ls.binary.path` from your Zed settings.
2. A `wowlua_ls` on your `$PATH` (e.g. a `cargo build --release` checkout).
3. The release binary for your platform, downloaded from [GitHub Releases](https://github.com/TradeSkillMaster/wowlua-ls/releases) into the extension's work directory. Release binaries embed the WoW API stubs, so nothing else is fetched. If GitHub can't be reached, a binary downloaded earlier is reused.

Releases are published for macOS (x86_64, arm64), Linux (x86_64), and Windows (x86_64, also used on arm64 through emulation). On any other platform - Linux arm64, for instance - build the server from source and point the extension at it:

```json
{
  "lsp": {
    "wowlua-ls": {
      "binary": {
        "path": "/path/to/wowlua-ls/target/release/wowlua_ls"
      }
    }
  }
}
```

`binary.arguments` and `binary.env` are passed through as well.

## Settings

### Running alongside another Lua language server

Installing the Lua extension also brings [LuaLS](https://luals.github.io/), which would report its own (WoW-unaware) diagnostics on the same files. Turn it off for Lua buffers:

```json
{
  "languages": {
    "Lua": {
      "language_servers": ["wowlua-ls", "!lua-language-server", "..."]
    }
  }
}
```

### Inlay hints

Zed requests inlay hints only when they're enabled:

```json
{
  "languages": {
    "Lua": {
      "inlay_hints": { "enabled": true }
    }
  }
}
```

Which hints the server produces is configured per project under `hint.*` in `.wowluarc.json`.

### Semantic tokens

Zed uses Tree-sitter highlighting by default and requests LSP semantic tokens only when asked to. The server classifies the cases a grammar can't (a dotted name that resolves to a class, property, method, or function; deprecated and standard-library markers):

```json
{
  "languages": {
    "Lua": {
      "semantic_tokens": "combined"
    }
  },
  "global_lsp_settings": {
    "semantic_token_rules": [
      { "token_type": "builtinConstant", "style": ["constant"] }
    ]
  }
}
```

The `builtinConstant` rule colors `true`/`false`/`nil` inside `expression<>` strings, a custom token type Zed has no default rule for.

## Notes

- Code lenses are off by default in Zed (`"code_lens": "on"` turns them on). The "N usages" and "overrides X" lenses render, but clicking them does nothing: they invoke editor-side commands that only the VS Code and JetBrains clients implement.
- Zed matches this extension to buffers by language, not by file extension, so a `.lua` file Zed has assigned to some other language won't be analyzed.

## Development

Build the extension locally:

```bash
cd editors/zed
cargo build --target wasm32-wasip2 --release
```

Then install it from the extensions view with **Install Dev Extension**, pointing at this directory. Zed rebuilds the crate itself, so the manual build above is only useful for a quick compile check. `zed --foreground` surfaces the extension's stdout/stderr and INFO-level logs.

The crate is excluded from the repository's Cargo workspace (it compiles for `wasm32-wasip2`) and pins the `stable` toolchain in `rust-toolchain.toml` rather than inheriting the workspace's pinned compiler.

### Publishing

Extensions are published from [zed-industries/extensions](https://github.com/zed-industries/extensions):

1. Bump `version` in `extension.toml` (the extension's own version - it is independent of the language server version, since the server binary is resolved at runtime).
2. Open a PR against that repository updating this extension's `extensions.toml` entry, which points at this repo as a submodule with `path = "editors/zed"`.
