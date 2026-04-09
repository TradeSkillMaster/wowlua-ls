Spawn a Vibe Kanban workspace for a task.

The task description is: $ARGUMENTS

Steps:

1. Use `get_context` to get the current workspace's repo info
2. Use `start_workspace` to create a new workspace with:
   - A short descriptive name for the task
   - The same repo as the current workspace, branched from `main`
   - Executor: `CLAUDE_CODE`
   - A detailed prompt describing the task, including:
     - What the problem is and how to reproduce it
     - Relevant file paths and line numbers
     - Suggested approach if known
     - Reminder to check CLAUDE.md for conventions
     - Reminder to ensure zero warnings from `cargo build` and all tests pass with `cargo test`

Rules:
- Do NOT create issues in the kanban board — only create the workspace
- If the user references an existing issue by number, use `list_issues` to find it and pass its `issue_id` to `start_workspace`
- If the task comes from a `.context/ACTION-*.md` file, include the relevant details from that file in the prompt
- Keep the workspace name under 50 characters
- Include this git instruction in the prompt: "IMPORTANT: This repo uses a local-only main branch (no remote tracking). Do NOT try to push, pull, fetch, or rebase against origin/main. Just commit your changes to the local branch — the workspace tooling handles merging."
