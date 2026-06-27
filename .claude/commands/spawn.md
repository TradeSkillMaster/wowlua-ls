---
description: Spawn a Vibe Kanban workspace for a task
---

Spawn a Vibe Kanban workspace for a task.

The task description is: $ARGUMENTS

Steps:

1. Use `get_context` to get the current MCP context — you need its `repo_id` to spawn into the same repository.
2. Use `start_workspace` to create a new workspace (this also starts its first session) with:
   - `name`: a short descriptive name for the task
   - `repo_id`: the repo ID from `get_context` (same repository as the current workspace)
   - `branch`: `main` (the base branch to branch from)
   - `configuration`: leave unset to use the default agent configuration (Settings → Agents); pass a configuration name only if the user asks for a specific agent or model
   - `prompt`: a detailed prompt describing the task, including:
     - What the problem is and how to reproduce it
     - Relevant file paths and line numbers
     - Suggested approach if known
     - Reminder to check CLAUDE.md for conventions
     - Reminder to ensure zero warnings from `cargo build` and all tests pass with `cargo test`

Rules:
- Do NOT create kanban issues — only start the workspace
- If the user references an existing GitHub issue, pass its number as `github_issue_number` (e.g. `42`) to `start_workspace`. This links the issue and auto-closes it when the workspace PR merges — there is no separate issue-lookup tool to call first.
- If the task comes from a `.context/ACTION-*.md` file, include the relevant details from that file in the prompt
- Keep the workspace name under 50 characters
- Include this git instruction in the prompt: "IMPORTANT: This repo uses a local-only main branch (no remote tracking). Do NOT try to push, pull, fetch, or rebase against origin/main. Just commit your changes to the local branch — the workspace tooling handles merging."
