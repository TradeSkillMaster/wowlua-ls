# wowlua-ls

A Language Server Protocol implementation for World of Warcraft addon development. Provides intelligent Lua editing with full support for LuaLS-style annotations and the WoW API.

## Features

### LSP capabilities
- **Hover** — Type information and documentation on mouse-over
- **Go to Definition** — Jump to symbol definitions, including across files
- **Completions** — Context-aware suggestions triggered by `.` and `:`
- **Signature Help** — Parameter hints for function calls
- **Find References** — Locate all usages of a symbol
- **Rename** — Safe symbol renaming across scopes
- **Diagnostics** — 20+ semantic checks (type mismatches, undefined globals/fields, unused locals, nil safety, and more)

### Annotation support
Supports [LuaLS](https://luals.github.io/)-style annotations:

| Annotation | Description |
|---|---|
| `@param` | Function parameter types and optionality |
| `@return` | Return types |
| `@type` | Variable type annotation |
| `@class` | Class definition with inheritance |
| `@field` | Class field with visibility (public/private/protected) |
| `@alias` | Type aliases |
| `@overload` | Function overload signatures |
| `@deprecated` | Mark symbols as deprecated |
| `@nodiscard` | Warn when return values are ignored |
| `@meta` | Declaration-only files (suppresses all diagnostics) |
| `@diagnostic` | Suppress specific diagnostics inline |

Type syntax supports unions (`A | B`), arrays (`T[]`), parameterized types (`table<K, V>`), and generics.

### Diagnostics

Each diagnostic can be individually suppressed with `---@diagnostic disable:diagnostic-name`.

| Diagnostic | Severity | Description |
|---|---|---|
| `deprecated` | Warning | Usage of `@deprecated` symbols |
| `discard-returns` | Warning | Ignoring `@nodiscard` return values |
| `type-mismatch` | Warning | Argument type vs `@param` mismatch |
| `return-mismatch` | Warning | Return type vs `@return` mismatch |
| `field-type-mismatch` | Warning | Field assignment vs `@field` type mismatch |
| `assign-type-mismatch` | Warning | Reassignment vs `@type` mismatch |
| `missing-param` | Warning | Missing required function arguments |
| `redundant-param` | Warning | Extra function arguments |
| `missing-return-value` | Warning | Return with fewer values than `@return` |
| `missing-return` | Warning | Function missing return statement |
| `undefined-global` | Warning | Reference to unresolved global name |
| `undefined-field` | Warning | Accessing nonexistent field on `@class` |
| `need-check-nil` | Warning | Field/method access on possibly-nil value |
| `private-access` | Warning | Accessing `@field private` from outside |
| `protected-access` | Warning | Accessing `@field protected` from outside hierarchy |
| `duplicate-index` | Warning | Duplicate keys in table constructors |
| `unused-local` | Hint | Unreferenced local variables |
| `redefined-local` | Hint | Same-scope local variable redefinition |
| `inject-field` | Hint | Setting undeclared fields on `@class` tables |
| `unreachable-code` | Hint | Code after return |
| `code-after-break` | Hint | Code after break |

## Project Configuration

Place a `.wowluarc.json` file in any directory to configure the language server for that directory and its subdirectories. All fields are optional.

```json
{
  "ignore": ["Libs/", "External/"],
  "diagnostics": {
    "disable": ["unused-local", "inject-field"],
    "severity": {
      "undefined-global": "error",
      "unused-function": "warning"
    }
  }
}
```

| Field | Description |
|---|---|
| `ignore` | Array of path prefixes to exclude from scanning, relative to the config file's directory. Patterns ending with `/` match directory prefixes. |
| `diagnostics.disable` | Array of diagnostic codes to suppress for files in this directory tree. |
| `diagnostics.severity` | Map of diagnostic code to severity override (`"error"`, `"warning"`, `"info"`, `"hint"`). |

Config files are hierarchical, like `.gitignore`: place one at the workspace root for project-wide settings, and additional ones in subdirectories for directory-specific overrides. Ignore patterns are relative to the directory containing the config file. Disabled diagnostics are unioned across all ancestor configs. Severity overrides from deeper configs take precedence.

Configs are discovered during workspace scanning and automatically reloaded when any `.wowluarc.json` is saved.

## Building

```bash
cargo build --release
```

## Usage

### As a language server

Run the binary with no arguments to start the LSP server over stdio. Configure your editor to use it for Lua files.

### CLI tools

```bash
# Check a directory for diagnostics (errors + warnings by default)
cargo run -- check path/to/addon

# Include hints (unused locals, inject-field, etc.)
cargo run -- check path/to/addon --severity hint

# Use custom stubs directory instead of built-in WoW API stubs
cargo run -- check path/to/addon --stubs path/to/stubs

# Evaluate a file — prints AST, type info, symbols, and diagnostics
cargo run -- evaluate path/to/file.lua --with-stubs

# Test a query at a specific location (hover, definition, signature, completions, diagnostics)
cargo run -- test-query path/to/file.lua:10:5 --with-stubs
```

The `check` command exits with code 1 if any diagnostics are found, making it suitable for CI pipelines.

## WoW API Stubs

WoW API type definitions are loaded from `stubs/vscode-wow-api/`. These provide type information for the WoW API (frames, widgets, global functions, enums, etc.).

## Acknowledgments

The lexer, parser, and AST are based on [plusmouse/wow_ls](https://github.com/plusmouse/wow_ls).

## License

GPL-3.0
