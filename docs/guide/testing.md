# Testing

## Running tests

```bash
# All tests (15 integration tests + parse stress tests)
cargo test

# Quick smoke test (build + integration tests)
./tests/smoke.sh
```

## Test structure

Tests use **annotation-driven assertions** embedded directly in Lua test files. Write code, then assert what the LS should report at specific positions:

```lua
local x = 5
--    ^ hover: x: number  def: local

---@param name string
function greet(name) end

greet("hi")
--    ^ sig: fun(name: string)

greet(42)
--    ^ diag: type-mismatch
```

The `^` caret marks the column to query. Assertions after it are separated by double-space.

## Assertion types

| Assertion | What it checks | Example |
|---|---|---|
| `hover:` | Type shown on hover (prefix match) | `hover: x: number` |
| `def:` | Definition location | `def: local` or `def: external` |
| `sig:` | Active signature label | `sig: fun(name: string): boolean` |
| `diag:` | Diagnostic code at position | `diag: type-mismatch` or `diag: none` |
| `refs:` | Find-references positions | `refs: 10:5, 15:3` |
| `comp:` | Completion items present | `comp: width, height, visible` |
| `tok:` | Semantic token classification | `tok: function defaultLibrary` |

Use `diag: none` to assert that no diagnostic fires at a position.

## Test file layout

Tests are organized by feature. Each file tests one area:

| File | What it covers |
|---|---|
| `tests/integration.lua` | Basic hover/def: primitives, functions, scopes |
| `tests/integration_stubs.lua` | WoW API stubs, Frame types |
| `tests/annotations.lua` | @param, @return, @type, @class, @field, @alias |
| `tests/generics.lua` | Generic type parameters |
| `tests/overloads.lua` | Overload resolution |
| `tests/diagnostics/test.lua` | Core diagnostics |
| `tests/need-check-nil/` | Nil-checking with narrowing |
| `tests/references.lua` | Find references and rename |
| `tests/builder-pattern/` | @builds-field, @return built |
| `tests/return-overloads.lua` | Tuple-union @return |
| `tests/crossfile/` | Cross-file resolution |

## Per-directory config

Test subdirectories can include a `.wowluarc.json` to control behavior, typically to enable default-off diagnostics:

```json
{
  "diagnostics": {
    "enable": ["need-check-nil"]
  }
}
```

This mirrors how users configure their own projects, and the test harness applies the same config path the LSP server uses.

## Debugging tools

When a test fails or you need to understand how the LS sees your code:

### `evaluate`

Print types, symbols, and diagnostics for a single file:

```bash
cargo run -- evaluate path/to/file.lua --with-stubs
```

### `test-query`

Query hover, definition, signature, and diagnostics at a specific position:

```bash
cargo run -- test-query path/to/file.lua:10:5 --with-stubs
```

For cross-file features, use `--scan-dir` to load the full workspace:

```bash
cargo run -- test-query path/to/addon/Core.lua:42:10 --with-stubs --scan-dir path/to/addon
```

::: warning Always scan the full addon root
A partial `--scan-dir` misses cross-file classes, defclass calls, and inherited fields, producing false diagnostics. Always point `--scan-dir` at the top-level addon directory.
:::

## Writing good tests

When fixing a bug, always add a regression test covering the fix. When adding a feature, test both the happy path and edge cases: cover what should work and what should produce diagnostics.

Tests that rely on default-off diagnostic codes (`need-check-nil`, `implicit-nil-return`, etc.) must live in a subdirectory with a `.wowluarc.json` that enables them.
