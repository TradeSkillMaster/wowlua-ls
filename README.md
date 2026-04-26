# wowlua-ls

A language server for World of Warcraft addon development. Built specifically for WoW Lua — not a general-purpose Lua LS with WoW bolted on.

## Why wowlua-ls

- **WoW API built in** — 9,000+ API stubs for retail, classic, and classic era. No setup, no addon manager.
- **Powerful generics** — parameterized classes, constrained type parameters, backtick factory annotations, function-type projections (`params<F>`, `returns<F>`). Class-level generics propagate through method calls automatically.
- **Metatable inference** — understands `setmetatable` + `__index`, chained metatables, `__call`, operator metamethods. Your OOP patterns just work.
- **Correlated narrowing** — check one return value, and the LS narrows the rest. Eliminates false positives from multi-return functions.
- **Mixin and template support** — `CreateFrame("Frame", nil, nil, "BackdropTemplate")` returns `Frame & BackdropTemplate` automatically.
- **Flavor filtering** — declare `flavors: ["retail", "classic"]` and get warnings on APIs that don't exist in all your targets.
- **Builder pattern** — `@builds-field` tracks progressive type construction across chained method calls.

Full feature list and comparisons in the [documentation](https://tradeskillmaster.github.io/wowlua-ls/guide/why-wowlua-ls).

## Install

### VS Code

Install **wowlua-ls** from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=sapu94.wowlua-ls). The extension bundles the language server binary — no separate install needed.

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

## What it understands

### Annotations

LuaLS-compatible `---@` annotations:

`@param` `@return` `@type` `@class` `@field` `@alias` `@overload` `@generic` `@cast` `@as` `@deprecated` `@nodiscard` `@meta` `@diagnostic` `@see`

Plus WoW-specific extensions:

`@defclass` `@builds-field` `@built-name` `@built-extends` `@type-narrows` `@correlated` `@flavor-narrows` `@constructor` `@accessor`

### Type system

Unions (`A | B`), intersections (`A & B`), arrays (`T[]`), generics (`@generic T`), parameterized classes (`@class Foo<T>`), anonymous table shapes (`{field: type}`), optionals (`T?`), lateinit (`T!`), tuple-union returns (`@return (A, B) | (C, D)`), variadic returns (`@return ...T`).

### Diagnostics

55+ diagnostics covering type safety, nil checking, annotation correctness, code quality, and WoW-specific checks. Each one is individually configurable.

See the [full diagnostic list](https://tradeskillmaster.github.io/wowlua-ls/reference/diagnostics).

## CLI

Lint an addon from the command line:

```bash
wowlua_ls check path/to/addon
wowlua_ls check path/to/addon --severity hint
```

Exit code is `1` if any diagnostics are found — suitable for CI.

## Documentation

Full documentation at **[tradeskillmaster.github.io/wowlua-ls](https://tradeskillmaster.github.io/wowlua-ls/)**

- [Getting Started](https://tradeskillmaster.github.io/wowlua-ls/guide/getting-started)
- [Annotation Guide](https://tradeskillmaster.github.io/wowlua-ls/guide/basic-annotations)
- [Configuration Reference](https://tradeskillmaster.github.io/wowlua-ls/reference/configuration)
- [Contributing](https://tradeskillmaster.github.io/wowlua-ls/guide/development)
- [Discord](https://discord.gg/XgqevqEqJK)

## License

GPL-3.0
