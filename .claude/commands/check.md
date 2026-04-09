Run a full diagnostic check against a WoW addon project and produce a detailed analysis report.

The project path is: $ARGUMENTS

If no path is provided, ask the user which project to check.

Steps:

1. Build the LS in release mode: `cargo build --release`
2. Run the full check: `cargo run --release -- check <project_path> --with-stubs --severity hint`
3. Save the output to a temp file and count diagnostics by category (warnings and hints separately)
4. Launch parallel analysis agents for the major categories:
   - need-check-nil (typically the largest)
   - type-mismatch
   - undefined-field
   - All remaining categories combined
5. For each category, classify every warning as one of:
   - **LS bug** — incorrect behavior in the language server
   - **LS limitation** — correct behavior but the LS lacks the sophistication to avoid false positives
   - **Annotation issue** — fixable by improving annotations in the target project
   - **Real issue** — genuine code concern
   - **Stub gap** — missing WoW API stubs
6. Write the analysis into `.context/` as separate ACTION files:
   - `.context/ACTION-ls-bugs.md` — LS bugs to fix (with file locations, fix suggestions, regression test ideas)
   - `.context/ACTION-ls-enhancements.md` — Feature improvements to reduce false positives
   - `.context/ACTION-annotations.md` — Annotation changes needed in the target project
   - `.context/ACTION-missing-stubs.md` — Missing globals, Classic API gaps, stub fields
7. Verify every item in the ACTION files against the actual check output — confirm counts match, file:line references exist, and no stale items remain
8. Present a summary table showing counts by category and classification

Rules:
- If the user asks to spawn workspaces for any of the findings, use the `/spawn` skill — do NOT manually call MCP tools like `create_issue` or `start_workspace`
- Do NOT create issues in the kanban board unless the user explicitly asks for issues to be created
- This repo uses a local-only main branch (no remote tracking). Do NOT try to push, pull, fetch, or rebase against origin/main.
