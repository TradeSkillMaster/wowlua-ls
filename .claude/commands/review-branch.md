---
description: Review this worktree's branch before landing it
---

Review the changes on this worktree's branch: correctness, the project's rules, and what the change does to diagnostics and cross-file types on the addon corpus. Then fix what the review finds.

Report only findings worth fixing: something that will actually go wrong, such as a false positive or missed diagnostic on realistic addon code, a wrong type or hover, a crash, a broken build or test, or a project rule broken in a way that will cause real trouble later. Leave out nit-picks (style, naming, wording, small refactors) and anything technically wrong that no realistic code or user will ever hit. If you can't say who it hurts and when, drop it.

Steps:

1. Scope: everything since `git merge-base HEAD main` (local `main`), committed or not, plus untracked files. If there is nothing, say so and stop. The main checkout is the first entry of `git worktree list`.

2. In the background: `cargo build --profile local`, then `cargo clippy --workspace --all-targets`, then `cargo test --workspace`. A warning or failure is a finding.

3. Run the `code-review` skill on the same changes for the general correctness review, at the level the user last used (never `max` or `ultra` unless they asked). Hold its findings to the bar above before it reports them. They are fixed in step 6 with the rest.

4. Project rules: spawn a fresh agent and give it the scope (base commit and untracked files), not your own account of the change. Have it read CLAUDE.md and the touched code itself and check the change against the project's rules, in particular:
   - A bug fix comes with a regression test in the matching test file (`.claude/TESTING.md`): no `diag: none`, and default-off codes only in an opted-in subdirectory.
   - Engine code never special-cases a named function; that behavior goes in stub annotations.
   - A type lost at the file boundary is fixed through `.claude/INVERSION.md`, never by a new scan pattern or build pass. The coarse scan registers that in-function writes exist; it never types them.
   - A diagnostic finds its own sites by walking the IR and AST through the query surface, not by consuming vectors collected during build or resolve.
   - A new semantic field on `ExternalGlobal`/`ClassDecl`/`AliasDecl`/`EventDecl`/`TypedSelfField` is compared in its `*_match()`; a new serialized path field is visited by `for_each_path_mut`.
   - A change to `stub_gen/`, `xml_scan.rs` or `stubs/overrides/` ships a regenerated blob. A missing or wrong API is fixed in `stub_gen/`, not with an override, and an override never replaces a vendor file wholesale.
   - A new diagnostic is in `CATALOG` and documented (both diagnostics docs pages, the schema's examples, and the default-off lists if it is off by default); new features and annotations are documented.
   - Annotation positions go through the one resolver per IR; secret-value rules live in `wowlua_core::secrets`.
   - No code from real addons in source, tests or comments.
   - Comments and the `.claude/` docs still describe the code after the change.

   Give it the bar above too. It backs each finding with the line that breaks the rule and, for a claim about behavior, a reproduction (`test-query`, or a test assertion that fails). Check each finding yourself and drop any that doesn't hold up or doesn't meet the bar.

5. Corpus check, unless the change can't affect analysis output (it touches only `docs/`, `editors/`, `.claude/` or `tests/`):
   - Before: `tests/baseline.sh "$(git merge-base HEAD main)"`. After: `target/local/wowlua_ls` from step 2.
   - With both binaries, run `check <addon> --severity hint` for each addon in `<main checkout>/.context/addons/*/`, and `crossfile-types` over all of them at once. Write the outputs under this worktree's `.context/review/`, never into the main checkout.
   - Diff before against after. Each added or removed line is either something the change sets out to do, or a finding: a new false positive, a lost true diagnostic, a new or wider cross-file type loss. Read the addon code at the line before deciding. When the diff is large, group it by diagnostic code and check examples from every group.

6. Fix every finding that held up and meets the bar, from steps 2 to 5 and the `code-review` skill alike, along with whatever the project's rules ask of the fix (a regression test, docs). Leave a finding unfixed only when fixing it is a decision for the user, such as a trade-off between two behaviors or work well beyond what the branch set out to do. Then check the fixes the way the review checked the change: rerun step 2, regenerate the stubs if a fix touches `stub_gen/`, `xml_scan.rs` or `stubs/overrides/`, and rerun step 5's diff if a fix can affect analysis output. Hold any new difference to the same standard.

7. Report once everything has finished:
   - The findings from steps 2, 4 and 5 (or that none are worth fixing), most severe first, each with `file:line` (for a corpus finding, the addon location that shows it), what's wrong, how it fails, and whether it was fixed (and how) or left (and why). Re-report the `code-review` skill's findings with each one's outcome instead of repeating them in the text.
   - A short summary of the corpus diff after the fixes: lines added and removed per diagnostic code and per cross-file path, and which of them are intended.
   - Whether clippy and the tests passed after the fixes.

Commit nothing.
