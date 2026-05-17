---
description: Open a project in VS Code with the wowlua-ls extension loaded
---

Open a project in VS Code with the wowlua-ls extension loaded for development.

The project path is: $ARGUMENTS

If no path is provided, ask the user which project to open.

Steps:
1. Resolve `<project_path>` to an absolute path. Run `code --status 2>&1 | grep "folder-uri=" | grep -qF "<resolved_path>"`. If it matches, STOP and ask the user to close that VS Code window first (it will silently reuse the existing window and ignore the new extension path).
2. Run `cargo build --release` and `cd editors/vscode && npm run build`.
3. Run `code --extensionDevelopmentPath=<absolute_path_to_editors/vscode> --disable-extensions <project_path>`.
