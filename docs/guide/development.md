# Development

## Building from source

```bash
git clone https://github.com/TradeSkillMaster/wowlua-ls.git
cd wowlua-ls
cargo build --release
```

The binary is at `target/release/wowlua_ls`. Configure your editor to run it as an LSP server over stdio for Lua files.

By default, WoW API stubs are embedded in the binary so it works as a standalone executable. To build a smaller binary that loads stubs from a `stubs/` directory next to the executable instead:

```bash
cargo build --release --no-default-features
```

## Checks before committing

```bash
cargo test --workspace                  # a bare `cargo test` skips the library crates' tests
cargo clippy --workspace --all-targets
```

Warnings are denied workspace-wide (`[workspace.lints]` in `Cargo.toml`), so a plain `cargo build` or `cargo test` fails on any rustc warning, and clippy must be clean. The toolchain is pinned in `rust-toolchain.toml` so a compiler update can't surface new warnings; bump it deliberately and re-run clippy when you do.

Some changes need a follow-up step:

| When | Also do |
|---|---|
| You change `crates/wowlua_stub_gen/`, `crates/wowlua_analysis/src/xml_scan.rs`, or `stubs/overrides/` | Run `cargo run -- regenerate-stubs` and commit both `stubs/precomputed.bin.zst` and `stubs/precomputed-files.bin.zst` (see [Stub Generation](./stub-generation)) |
| You run the VS Code extension from source | `cargo build`, then `cd editors/vscode && npm run build` (the extension loads the bundled `dist/extension.js`) |
| You edit a grammar in `editors/vscode/syntaxes/` | Run `python3 editors/jetbrains/textmate/sync_grammars.py` and commit the regenerated JetBrains copies |

## Project structure

The project is a cargo workspace. Code lives in layered library crates under `crates/`, and each crate can only use the crates listed above it:

```
crates/
├── wowlua_syntax/       # Lexer, parser, arena syntax tree (syntax/), typed AST (ast.rs)
├── wowlua_core/         # Shared vocabulary: IR types (types.rs), flavor bitmask (flavor.rs:
│                        #   retail, classic, classic_era, forever), annotation type definitions
├── wowlua_analysis/     # Per-file analysis engine
│   ├── analysis/        # Ir, analysis phases (prescan, build_ir, resolve), narrowing
│   │   └── queries/     # LSP queries: hover, definition, completion, references, inlay hints, ...
│   ├── annotations/     # Annotation parsing and cross-file scanners (globals, @defclass, @built-name)
│   ├── diagnostics/     # DiagnosticDef catalog, DiagnosticPass trait, run_all(), one module per pass
│   ├── pre_globals/     # PreResolvedGlobals: WoW API + workspace type database shared across files
│   ├── config.rs        # .wowluarc.json loading
│   └── xml_scan.rs      # XML frame/template scanning
├── wowlua_lsp/          # lsp/main_loop/ (server loop, handlers, workspace scan, rebuild,
│                        #   stub loading), plugins/ (Lua plugin engine), toc/ (.toc support)
├── wowlua_doc/          # Markdown API doc generation
└── wowlua_stub_gen/     # Stub generation pipeline (see Stub Generation)
src/                     # wowlua_ls binary
├── main.rs              # Entry point; runs the LSP server over stdio when no subcommand is given
├── cli/                 # Subcommands: check, evaluate, test-query, dump-types, dump-stubs, doc,
│                        #   profile, regenerate-stubs
└── lib.rs               # Re-exports every crate's modules as wowlua_ls::<module>
```

For a file-by-file map, see [`CLAUDE.md`](https://github.com/TradeSkillMaster/wowlua-ls/blob/main/CLAUDE.md) in the repository.

## How analysis works

Each file goes through three phases:

### Phase 0: Pre-scan (`prescan.rs`)

Imports external classes and aliases from the shared `PreResolvedGlobals`, then scans the file for local `@class` and `@alias` declarations. This establishes the type namespace before any expressions are analyzed.

### Phase 1: Build IR (`build_ir.rs`)

Walks the AST and creates the intermediate representation:
- **Scopes**: nested lexical scopes tracking variable visibility
- **Symbols**: local variables, parameters, globals (with version tracking for reassignment)
- **Functions**: parameter/return annotations, overloads, generic constraints
- **Tables**: fields, class names, parent classes, metatable links
- **Expressions**: lowered to `Expr` nodes by `lower_expression.rs` (symbol refs, field access, function calls, literals, etc.)

Type narrowing from control flow (nil guards, type checks, flavor filtering) is handled by `narrowing.rs` during this phase.

### Phase 2: Resolve types (`resolve.rs`)

A fixpoint loop that iterates until no more types change. Each iteration resolves expressions by walking their dependencies: if a symbol's type depends on a function call, the function's return type must be resolved first. The loop handles:

- Function call return types (via `resolve_call.rs`)
- Metatable `__index` chains
- Generic type parameter binding
- Nil narrowing (from guards analyzed in Phase 1)
- Backward parameter type inference
- Correlated return overload synthesis

### Diagnostic passes

After type resolution, `run_all()` in `crates/wowlua_analysis/src/diagnostics/mod.rs` orchestrates all diagnostic passes. Passes are organized into three groups that run sequentially:

1. **`run` passes**: walk the IR to check resolved types (type mismatches, undefined globals, missing fields, etc.)
2. **`visit_node` passes**: walk the AST for syntax-level checks (empty blocks, `not` precedence, unused varargs)
3. **`run_inject` passes**: the type-mismatch → inject-field pipeline, where type mismatch passes produce excess field info and `inject_field` consumes it last

Every diagnostic code is a `DiagnosticDef` constant in `mod.rs`, listed in its `CATALOG` array. Each pass module implements the `DiagnosticPass` trait, emitting diagnostics via `CONSTANT_NAME.emit(diags, message, start, end)`. See [Adding a Diagnostic](./adding-diagnostics) for the full walkthrough.

## Hover regression testing

The `dump-types` subcommand outputs the hover type for every identifier in a project. The output is deterministic and sorted by file, making it suitable for diffing against a saved baseline to catch hover/type regressions.

```bash
wowlua_ls dump-types path/to/addon --with-stubs
```

Each line has the format:

```
File.lua:line:col name → type
```

Identifiers that don't resolve show `→ <none>`.

Save a baseline, then diff after making changes:

```bash
# Save baseline
wowlua_ls dump-types path/to/addon --with-stubs > baseline.txt

# After changes, check for regressions
wowlua_ls dump-types path/to/addon --with-stubs | diff baseline.txt -
```

Any new `<none>` entries or changed types indicate a regression.

## Fuzzing

Three [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets live in `fuzz/`, covering the three main layers:

| Target | What it exercises |
|--------|-------------------|
| `fuzz_lexer` | Tokenization via `lexer::lex_all()` |
| `fuzz_parser` | Recursive descent parser via `Parser::new(s).parse()` |
| `fuzz_analysis` | Full pipeline: parse → `Analysis::new_with_tree` → `resolve_types()` |

All targets require valid UTF-8 input (non-UTF-8 bytes are rejected early). The `fuzz_analysis` target uses `PreResolvedGlobals::empty()` (no WoW API stubs) to keep iterations fast, and caches it in a `OnceLock` so it's built only once.

### Setup

```bash
rustup install nightly    # cargo-fuzz requires nightly
cargo install cargo-fuzz
```

### Running

```bash
# Run a target against its seed corpus (runs until Ctrl-C)
cargo +nightly fuzz run fuzz_lexer    fuzz/corpus/fuzz_lexer
cargo +nightly fuzz run fuzz_parser   fuzz/corpus/fuzz_parser
cargo +nightly fuzz run fuzz_analysis fuzz/corpus/fuzz_analysis

# Run for a fixed duration (seconds)
cargo +nightly fuzz run fuzz_analysis fuzz/corpus/fuzz_analysis -- -max_total_time=600
```

Run `fuzz_analysis` after changes to the parser or analysis pipeline; it covers the most code and is most likely to find issues. The lexer and parser targets are useful after changes to `lexer.rs` or `parser.rs` specifically.

### When it finds something

Crashes and timeouts are saved to `fuzz/artifacts/<target>/`. To minimize a crashing input to its smallest reproducing form:

```bash
cargo +nightly fuzz tmin fuzz_analysis fuzz/artifacts/fuzz_analysis/<crash-file>
```

The minimized input should be added as a regression test or kept in the seed corpus.

### Seed corpus

Hand-written seed files live in `fuzz/corpus/<target>/` (small `.lua` files covering representative syntax). Auto-generated corpus entries (hex-named files created by libFuzzer during runs) are gitignored.

## The two-tier index space

External globals (WoW API stubs) use indices ≥ `EXT_BASE` (1,000,000). Per-file locals use indices below that. This means lookups like `sym()`, `func()`, and `table()` route through an `idx >= EXT_BASE` check: external data lives in the shared `PreResolvedGlobals` while local data is on the per-file `Analysis`.

This avoids cloning ~170,000 external symbols per file, and external indices are stable across files, which is what makes workspace-wide find-references and rename work.

## Workspace startup

Before any files are analyzed, the LS scans the workspace (`crates/wowlua_lsp/src/lsp/main_loop/scan.rs`) to collect cross-file information:

1. **Pass 1** (runs while the stubs load): scan `.xml` files for frames and templates, then parse every `.lua` file and collect its annotations (`@class`, `@alias`, `@event`) and file-level globals. The parse trees are kept for the later passes.
2. **Pass 2** (needs the stubs): discover `@defclass` factory calls, `@built-name` builder classes, named globals created by `@creates-global` calls (`CreateFrame(type, "Name")`), and `@generates-events` callback registries.
3. **Pass 3**: scan methods of known classes for `self.field = …` assignments.

The results are layered onto the precomputed stub database by `PreResolvedGlobals::build_on_stubs()`, which registers classes and aliases, populates fields, builds methods, resolves inheritance, and sets up global entries.
