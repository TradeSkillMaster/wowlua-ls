# Development

## Building from source

```bash
git clone https://github.com/TradeSkillMaster/wowlua-ls.git
cd wowlua-ls
cargo build --release
```

The binary is at `target/release/wowlua_ls`. Configure your editor to run it as an LSP server over stdio for Lua files.

## Project structure

```
src/
├── main.rs              # CLI entry point (check, evaluate, test-query, LSP)
├── types.rs             # Core IR: ValueType, Expr, Symbol, Scope, Function, TableInfo
├── annotations.rs       # Annotation parser (@param, @return, @class, @field, etc.)
├── config.rs            # .wowluarc.json loading
├── pre_globals.rs       # PreResolvedGlobals — WoW API stubs, shared across files
├── flavor.rs            # Retail/classic/classic_era flavor bitmask
├── stub_gen.rs          # Stub generation from Ketho's vscode-wow-api
├── analysis/
│   ├── mod.rs           # Analysis struct, two-tier lookups, core helpers
│   ├── prescan.rs       # Phase 0: class/alias pre-scan, annotation type resolution
│   ├── build_ir.rs      # Phase 1: AST walk → scopes, symbols, expressions
│   ├── resolve.rs       # Phase 2: fixpoint type resolution loop
│   ├── checks.rs        # Deferred diagnostic checks (run after type resolution)
│   ├── queries.rs       # LSP queries: hover, definition, completion, signature help
│   └── semantic_tokens.rs
├── diagnostics/
│   ├── mod.rs           # WowDiagnostic struct + submodule declarations
│   └── *.rs             # One module per diagnostic (55+)
├── syntax/
│   ├── parser.rs        # Recursive descent + Pratt parser
│   ├── tree.rs          # Arena-based syntax tree
│   ├── lexer.rs         # Tokenization
│   └── syntax_kind.rs   # SyntaxKind enum
└── lsp/
    ├── main_loop.rs     # LSP server loop, request handlers
    └── diagnostics.rs   # Diagnostic publishing with suppression
```

## How analysis works

Each file goes through three phases:

### Phase 0: Pre-scan (`prescan.rs`)

Imports external classes and aliases from the shared `PreResolvedGlobals`, then scans the file for local `@class` and `@alias` declarations. This establishes the type namespace before any expressions are analyzed.

### Phase 1: Build IR (`build_ir.rs`)

Walks the AST and creates the intermediate representation:
- **Scopes** — nested lexical scopes tracking variable visibility
- **Symbols** — local variables, parameters, globals (with version tracking for reassignment)
- **Functions** — parameter/return annotations, overloads, generic constraints
- **Tables** — fields, class names, parent classes, metatable links
- **Expressions** — lowered to `Expr` nodes (symbol refs, field access, function calls, literals, etc.)

This phase also runs some diagnostics that don't need type resolution (like `empty-block` or `unreachable-code`).

### Phase 2: Resolve types (`resolve.rs`)

A fixpoint loop that iterates until no more types change. Each iteration resolves expressions by walking their dependencies — if a symbol's type depends on a function call, the function's return type must be resolved first. The loop handles:

- Function call return types
- Metatable `__index` chains
- Generic type parameter binding
- Nil narrowing (from guards analyzed in Phase 1)
- Backward parameter type inference
- Correlated return overload synthesis

After the fixpoint settles, deferred diagnostic checks run (type mismatches, missing params, etc.).

## The two-tier index space

External globals (WoW API stubs) use indices ≥ `EXT_BASE` (1,000,000). Per-file locals use indices below that. This means lookups like `sym()`, `func()`, and `table()` route through an `idx >= EXT_BASE` check — external data lives in the shared `PreResolvedGlobals` while local data is on the per-file `Analysis`.

This avoids cloning ~9,000 external symbols per file, and external indices are stable across files, which is what makes workspace-wide find-references and rename work.

## Workspace startup

Before any files are analyzed, the LS runs four scanning passes to collect cross-file information:

1. **Pass 1** — Scan annotations and file-level globals (`@class`, `@alias`, top-level functions)
2. **Pass 2** — Discover `@defclass` factory calls and extract constructor fields
3. **Pass 3** — Discover `@built-name` classes from builder-pattern call sites
4. **Pass 4** — Scan colon-method bodies for typed `self.field` assignments

This feeds into `PreResolvedGlobals::build()`, which runs five phases of its own to register classes, populate fields, build methods, resolve inheritance (fixpoint loop for deep hierarchies), and set up global functions.
