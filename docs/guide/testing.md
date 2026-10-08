# Testing

## Running tests

```bash
# All tests, across every crate in the workspace
cargo test --workspace

# Quick smoke test (build + all tests, condensed output)
./tests/smoke.sh
```

A bare `cargo test` runs only the root package's tests and skips the library crates' unit tests.

## Test structure

Tests use **annotation-driven assertions** embedded directly in Lua test files. Write code, then assert what the LS should report at specific positions:

```lua
---@diagnostic disable: unused-local
local x = 5
--    ^ hover: (local) x: number = 5  def: local

---@param name string
local function greet(name) end

greet("hi")
--    ^ sig: fun(name: string)

greet(42)
--    ^ diag: type-mismatch
```

The `^` caret marks the column to query. Assertions after it are separated by double-space.

## Assertion types

| Assertion | What it checks | Example |
|---|---|---|
| `hover:` | Type shown on hover. Exact match, except that a single-line expectation prefix-matches a multi-line hover; write `\n` to assert a full multi-line hover | `hover: (local) y: number` |
| `doc:` | Substring of the hover's doc text (`doc: !TEXT` asserts it is absent) | `doc: Returns the frame` |
| `def:` | Definition location | `def: local`, `def: local 4:7`, `def: external`, `def: None` |
| `defs:` | Number of distinct definition sites (a name defined in several files) | `defs: 2` |
| `typedef:` | Go to Type Definition location, same forms as `def:` | `typedef: local 12:11` |
| `typedefs:` | Number of distinct type-definition sites | `typedefs: 2` |
| `sig:` | Active signature label (prefix match) | `sig: fun(name: string): boolean` |
| `diag:` | Diagnostic code on the code line; `~text` also requires the message to contain `text` | `diag: type-mismatch ~expected string` |
| `refs:` | Find-references positions, including the declaration (any order) | `refs: 10:5, 15:3` |
| `highlight:` | Document highlight positions (any order) | `highlight: 3:1, 8:1` |
| `linked:` | Linked editing ranges (any order) | `linked: 5:7, 7:11` |
| `comp:` | The full completion list (any order), or `none` | `comp: width, height, visible` |
| `tok:` | Semantic token type and modifiers, or `none` | `tok: function defaultLibrary` |
| `hint:` | Inlay hint label at the caret, or `none` | `hint: name:` |
| `lens:` | Code lens on the code line: the function name for "N usages", `N implementations`, `overrides Parent`, or `none` | `lens: greet` |

The harness fails on any diagnostic that no `diag:` assertion covers, so a change that introduces an unexpected diagnostic fails the test on its own. Only assert diagnostics that should fire, and suppress incidental ones (an `unused-local` in a hover test, say) with a top-of-file `---@diagnostic disable: code1, code2`.

## Test file layout

Tests are organized by feature, one area per file or directory. A few examples (the full catalog is [`.claude/TESTING.md`](https://github.com/TradeSkillMaster/wowlua-ls/blob/main/.claude/TESTING.md) in the repository):

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

## Adding a test file

Each `.lua` test file runs from its own `#[test]` in `tests/integration_test.rs`:

```rust
#[test]
fn my_feature() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/my-feature.lua",
        with_stubs: false,       // true to load the WoW API stubs
        scan_dir: None,          // Some("tests/my-feature") to scan a directory for cross-file tests
    });
}
```

## Per-directory config

Test subdirectories can include a `.wowluarc.json` to control behavior. Tests that rely on default-off diagnostics (`need-check-nil`, `implicit-nil-return`, etc.) must live in a subdirectory whose `.wowluarc.json` enables them:

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
