# WoW Lua Language Server

A language server for World of Warcraft addon development. Built specifically for WoW Lua - not a general-purpose Lua LS with WoW bolted on.

## Features

- **9,000+ WoW API stubs built in** - every function, frame type, enum, and global for retail, classic, and classic era. No setup, no addon manager.
- **Event handler typing** - `SetScript("OnEvent", handler)` types `self`, `event`, and per-event payload params. Works with custom event systems too via the `@event` annotation.
- **XML frame scanning** - automatically scans `.xml` files for frame definitions, virtual templates, `parentKey` children, `inherits` chains, and `mixin` attributes.
- **TOC file support** - hover documentation, completions, go-to-definition on file paths, and diagnostics for `.toc` files. SavedVariables are auto-detected as allowed globals.
- **Metatable inference** - understands `setmetatable` + `__index`, chained metatables, `__call`, operator metamethods. Your OOP patterns just work.
- **Correlated narrowing** - check one return value, and the LS narrows the rest. Eliminates false positives from multi-return functions.
- **Mixin and template support** - `CreateFrame("Frame", nil, nil, "BackdropTemplate")` returns `Frame & BackdropTemplate` automatically.
- **Flavor filtering** - declare target flavors and get warnings on APIs that don't exist in all your targets.
- **70 diagnostics** - type safety, nil checking, annotation correctness, code quality, and WoW-specific checks. Each one individually configurable.
- **Diagnostic plugins** - write custom Lua scripts to enforce project-specific conventions.
- **CI-ready CLI** - `wowlua_ls check path/to/addon` lints your addon and exits non-zero on diagnostics.
- **Powerful generics** - parameterized classes, constrained type parameters, function-type projections. Class-level generics propagate through method calls automatically.
- **Builder pattern** - `@builds-field` tracks progressive type construction across chained method calls.

## Getting started

Open a WoW addon folder. The extension automatically scans `.lua` files, loads WoW API stubs, and starts reporting diagnostics. No configuration required.

For project-specific settings, add a `.wowluarc.json` to your addon root:

```json
{
  "ignore": ["Libs/"],
  "flavors": ["retail", "classic"],
  "diagnostics": {
    "enable": ["need-check-nil"]
  }
}
```

## Annotations

LuaLS-compatible `---@` annotations:

`@param` `@return` `@type` `@class` `@enum` `@field` `@alias` `@overload` `@generic` `@cast` `@as` `@deprecated` `@nodiscard` `@meta` `@diagnostic` `@see`

Plus WoW-specific extensions:

`@defclass` `@builds-field` `@built-name` `@built-extends` `@type-narrows` `@correlated` `@flavor-narrows` `@constructor` `@accessor`

## Documentation

Full documentation at **[tradeskillmaster.github.io/wowlua-ls](https://tradeskillmaster.github.io/wowlua-ls/)**

- [Getting Started](https://tradeskillmaster.github.io/wowlua-ls/guide/getting-started)
- [Annotation Guide](https://tradeskillmaster.github.io/wowlua-ls/guide/basic-annotations)
- [Configuration Reference](https://tradeskillmaster.github.io/wowlua-ls/reference/configuration)
- [Diagnostic List](https://tradeskillmaster.github.io/wowlua-ls/reference/diagnostics)
- [Discord](https://discord.gg/XgqevqEqJK)

## Settings

| Setting | Description | Default |
|---------|-------------|---------|
| `wowluals.serverPath` | Path to the `wowlua_ls` binary. If empty, uses the bundled binary. | `""` |

## License

GPL-3.0 - see [LICENSE](https://github.com/TradeSkillMaster/wowlua-ls/blob/main/LICENSE.md).
