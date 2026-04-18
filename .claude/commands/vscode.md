Open a project in VS Code with the wowlua-ls extension loaded for development.

The project path is: $ARGUMENTS

If no path is provided, ask the user which project to open.

Steps:
1. **Before doing anything else**, check whether VS Code already has a window open for `<project_path>`. Run `pgrep -af "code.*<project_path>"` (or an equivalent check). If a window is already open for that folder, STOP and ask the user to close it first — VS Code silently reuses the existing window and ignores the new `--extensionDevelopmentPath`, so the build won't actually load. Do NOT attempt the launch and then warn afterward; by that point the wrong VS Code is already running.
2. Build the language server: `cargo build`
3. Resolve the absolute path to the vscode extension directory: `editors/vscode` relative to the wowlua-ls repo root.
4. Run: `code --extensionDevelopmentPath=<extension_dir> --disable-extensions <project_path>`

Rules:
- Use the absolute path for --extensionDevelopmentPath
- The pre-launch check in step 1 is mandatory. A post-launch "reminder to close VS Code" is useless — by then VS Code has already foregrounded the wrong instance.
