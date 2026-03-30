Open a project in VS Code with the wowlua-ls extension loaded for development.

The project path is: $ARGUMENTS

If no path is provided, ask the user which project to open.

Steps:
1. Build the language server: `cargo build`
2. Resolve the absolute path to the vscode extension directory: `editors/vscode` relative to the wowlua-ls repo root.
3. Run: `code --extensionDevelopmentPath=<extension_dir> --disable-extensions <project_path>`

Rules:
- Use the absolute path for --extensionDevelopmentPath
