# Adding a Diagnostic

Each diagnostic lives in its own module under `src/diagnostics/`. Adding a new one is straightforward.

## 1. Create the module

Create `src/diagnostics/my_new_check.rs`:

```rust
use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "my-new-check";

pub fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "description of what's wrong".to_string(),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
```

The `CODE` string is what users reference in `@diagnostic disable:my-new-check` and in `.wowluarc.json` — suppression works automatically by matching this string.

## 2. Register the module

Add `pub mod my_new_check;` to `src/diagnostics/mod.rs`.

## 3. Call the check

Where you call `check()` depends on what information the diagnostic needs:

- **Phase 1 checks** (`build_ir.rs`) — for things detectable from syntax alone, like `empty-block` or `unreachable-code`. These don't need resolved types.

- **Deferred checks** (`checks.rs`) — for anything that needs type information. Most diagnostics live here. The check runs after the Phase 2 fixpoint loop resolves all types. Common patterns:
  - `check_return_type_diagnostics()` — return type mismatches
  - `check_unused_local_diagnostics()` — unused variables
  - `check_deferred_type_mismatch()` — argument type errors

Deferred checks consume queues populated during Phase 1. For example, `deferred.return_type_checks` collects return statements during IR building, then `checks.rs` drains them after types are resolved.

## 4. Add a test

Add test assertions to the appropriate test file. If your diagnostic is default-on, add it to `tests/diagnostics/test.lua`. If it's default-off, create a subdirectory with a `.wowluarc.json` that enables it.

```lua
-- Test that the diagnostic fires
local x = badThing()
--        ^ diag: my-new-check

-- Test that it doesn't fire on valid code
local y = goodThing()
--        ^ diag: none
```

Run `cargo test` to verify.

## 5. Document it

Add the diagnostic to the table in `README.md`.

## Severity guidelines

| Severity | Use for |
|---|---|
| **Warning** | Likely bugs, type errors, annotation problems |
| **Hint** | Code quality suggestions, unused variables, style |

## Default-off diagnostics

Some diagnostics are too noisy for unannotated codebases. Make them default-off by adding the code to `disabled_diagnostics_for()` in `src/lsp/diagnostics.rs`. Users opt in via `diagnostics.enable` in `.wowluarc.json`.

Examples: `need-check-nil`, `implicit-nil-return`, `unknown-param-type`.
