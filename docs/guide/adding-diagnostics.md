# Adding a Diagnostic

Diagnostics use a trait-based architecture. Each diagnostic pass lives in its own module under `crates/wowlua_analysis/src/diagnostics/` and implements the `DiagnosticPass` trait. All diagnostic codes are defined centrally in `crates/wowlua_analysis/src/diagnostics/mod.rs`.

## 1. Define the diagnostic code

Add a `DiagnosticDef` constant to `crates/wowlua_analysis/src/diagnostics/mod.rs`:

```rust
pub const MY_NEW_CHECK: DiagnosticDef = DiagnosticDef { code: "my-new-check", severity: DiagnosticSeverity::WARNING };
```

Also add it to the `CATALOG` array in the same file so it's recognized by the suppression and validation systems.

The `code` string is what users reference in `@diagnostic disable:my-new-check` and in `.wowluarc.json` - suppression works automatically by matching this string.

## 2. Create the module

Create `crates/wowlua_analysis/src/diagnostics/my_new_check.rs`. The `DiagnosticPass` trait has four methods, all with default implementations - implement whichever fits your diagnostic:

### `run()`: full-analysis pass

Best for diagnostics that walk the IR (symbols, functions, expressions) after type resolution:

```rust
use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use crate::types::SymbolIdentifier;
use super::{DiagnosticPass, WowDiagnostic};

pub struct MyNewCheck;

impl DiagnosticPass for MyNewCheck {
    fn run(&self, analysis: &AnalysisResult, _tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        // Iterate this file's symbols through the IR query surface
        for (_, sym) in analysis.local_symbols() {
            let SymbolIdentifier::Name(name) = &sym.id else { continue };
            for ver in &sym.versions {
                if /* condition */ {
                    super::MY_NEW_CHECK.emit(
                        diags,
                        format!("'{name}' is wrong because ..."),
                        ver.def_node.start as usize,
                        ver.def_node.end as usize,
                    );
                }
            }
        }
    }
}
```

The IR arenas are private, so read them through the query surface on `AnalysisResult`: `local_symbols()`, `local_functions()`, `local_tables()`, and `local_exprs()` iterate the file's own entries, and `sym()`, `func()`, `table()`, `expr()`, and `scope()` resolve an index, including indices into the shared WoW API stubs.

### `visit_node()`: AST walk pass

Best for diagnostics that check syntax patterns. Called once per AST node during a shared tree walk:

```rust
use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, BinaryExpression};
use crate::syntax::SyntaxKind;
use super::{DiagnosticPass, WowDiagnostic};

pub struct MyNewCheck;

impl DiagnosticPass for MyNewCheck {
    fn visit_node(&self, node: crate::syntax::SyntaxNode<'_>, _analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
        if node.kind() != SyntaxKind::BinaryExpression { return; }
        let Some(bin) = BinaryExpression::cast(node) else { return };
        // Check the node and emit diagnostics
    }
}
```

### `run_inject()`: inject-field pipeline

For diagnostics that participate in the type-mismatch → inject-field pipeline. Used by `type_mismatch`, `return_mismatch`, `field_type_mismatch`, `assign_type_mismatch`, and `inject_field`. You probably don't need this unless your diagnostic feeds the inject-field system.

### `runs_in_meta()`: `@meta` files

Passes are skipped in `@meta` (declaration-only stub) files by default. Override this to return `true` only for a `run()` pass that validates the annotations themselves, such as one flagging an undefined type name or a malformed annotation.

## 3. Register the module

In `crates/wowlua_analysis/src/diagnostics/mod.rs`:

1. Add `mod my_new_check;` to the module declarations at the top
2. Add your pass to the appropriate list in `run_all()`:
   - `run_passes`: for `run()` implementations (most diagnostics)
   - `node_passes`: for `visit_node()` implementations (AST walks)
   - `inject_passes`: for `run_inject()` implementations (type-mismatch pipeline)

```rust
let run_passes: &[&dyn DiagnosticPass] = &[
    // ... existing passes ...
    &my_new_check::MyNewCheck,
];
```

## 4. Add a test

Add test assertions to the appropriate test file. If your diagnostic is default-on, add it to `tests/diagnostics/test.lua`. If it's default-off, create a subdirectory with a `.wowluarc.json` that enables it.

```lua
-- Test that the diagnostic fires
local x = badThing()
--        ^ diag: my-new-check
```

The harness fails on any diagnostic without a `diag:` assertion, so valid code needs no assertion to prove the diagnostic stays quiet (see [Testing](./testing)).

Run `cargo test --workspace` to verify.

## 5. Document it

- Add the diagnostic to the table in `docs/reference/diagnostics.md`.
- Add a section to `docs/guide/diagnostics.md` under the matching category.
- Add the code to the `$defs.diagnosticCode.examples` list in `editors/vscode/wowluarc.schema.json`, which drives `.wowluarc.json` completion.
- If it's default-off, also add it to the default-off list in that schema's `diagnostics.enable` description and to the "Diagnostics disabled by default" list in `docs/guide/configuration.md`.

## Severity guidelines

| Severity | Use for |
|---|---|
| **Error** | Analysis failures, not code findings (`safety-limit`) |
| **Warning** | Likely bugs, type errors, annotation problems |
| **Hint** | Code quality suggestions, unused variables, style |

## Default-off diagnostics

Some diagnostics are too noisy for unannotated codebases. Make them default-off by adding the code to `DEFAULT_DISABLED_CODES` in `crates/wowlua_analysis/src/diagnostics/mod.rs`. Users opt in via `diagnostics.enable` in `.wowluarc.json`.

Examples: `need-check-nil`, `implicit-nil-return`, `unknown-param-type`.

## Hybrid modules

Some diagnostic modules are "hybrid" - they implement `DiagnosticPass` for the post-analysis phase AND export `pub` helper functions called from `build_ir.rs` or `resolve.rs` during IR construction. Both roles share the same `DiagnosticDef` constants from the catalog. This is used when a diagnostic needs to emit during IR construction (e.g. when specific AST context is available) and also during post-analysis passes.
