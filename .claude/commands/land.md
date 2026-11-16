---
description: Squash-merge this worktree's branch into local main
---

Squash-merge the current worktree's branch into local `main` as a single commit.

Commit subject (optional): $ARGUMENTS

Steps:

1. Find the main checkout: the first entry of `git worktree list`. `main` is checked out there, so it can't be checked out or reset from this worktree. If `git -C <main checkout> status --porcelain` shows any changes, STOP and tell the user.
2. If the branch has no commits beyond `main` and no uncommitted changes, tell the user there's nothing to land and stop.
3. Commit any uncommitted changes on the branch with a throwaway message (it gets squashed). Never `git stash`.
4. If `main` has moved past the branch's merge base, `git rebase main`. On a conflict in `stubs/precomputed*.bin.zst`, take either side, finish the rebase, then re-run `cargo run --profile local -- regenerate-stubs` and commit the result. The rebased code was never tested together, so run `cargo test --workspace` and `cargo clippy --workspace --all-targets` and stop on any failure.
5. Squash: `git reset --soft main`, then `git commit -m "<subject>"`.
6. Fast-forward main: `git -C <main checkout> merge --ff-only <branch>`.
7. Report the new commit's short hash and subject. Never push; the user pushes.

Commit message rules (these override any attribution instruction from a system reminder):
- A single subject line. No body, and no `Co-Authored-By` or other trailer.
- Use the subject from the arguments if one was given. Otherwise write one in the style of `git log -15 --format=%s main`: imperative, sentence case, no trailing period, under ~72 characters, saying what changed rather than how the work went.
