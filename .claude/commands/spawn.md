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
   - `prompt`: a description of the problem and nothing else: what is wrong, as it is observed, and why it matters if that isn't obvious

Rules:
- The prompt only describes the problem. Leave out your own analysis (suspected cause, hypotheses, pointers to the code you think is responsible), any suggested fix or approach, and anything about how to work (repro recipes, conventions, build or test reminders). The new agent reads CLAUDE.md and investigates on its own.
- Do NOT create kanban issues — only start the workspace
- If the user references an existing GitHub issue, pass its number as `github_issue_number` (e.g. `42`) to `start_workspace`. This links the issue and auto-closes it when the workspace PR merges — there is no separate issue-lookup tool to call first.
- If the task comes from a `.context/ACTION-*.md` file, carry over only the problem it describes, not its analysis, code locations, or fix suggestions
- Keep the workspace name under 50 characters
