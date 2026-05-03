const vscode = require("vscode");
const { workspace, window, commands, Uri, Position, Range, Location } = vscode;
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const path = require("path");
const fs = require("fs");

let client;

function activate(context) {
  const config = workspace.getConfiguration("wowluals");
  let serverPath = config.get("serverPath");

  if (!serverPath) {
    const ext = process.platform === "win32" ? ".exe" : "";
    const platform = `${process.platform}-${process.arch}`;
    const extRoot = path.resolve(__dirname, "..");
    const candidates = [
      path.join(extRoot, "server", platform, `wowlua_ls${ext}`),
      path.join(extRoot, "../../target/release/wowlua_ls"),
      path.join(extRoot, "../../target/debug/wowlua_ls"),
    ];
    serverPath = candidates.find((p) => fs.existsSync(p));
    if (!serverPath) {
      window.showErrorMessage(
        `wowlua_ls binary not found for platform "${platform}". Install from a release VSIX, run \`cargo build\` in the repo, or set wowluals.serverPath in settings.`
      );
      return;
    }
  }

  const serverOptions = {
    command: serverPath,
    transport: TransportKind.stdio,
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "lua" }],
  };

  // The LSP server emits code-lens Commands whose arguments are plain JSON.
  // VS Code built-in commands (showReferences, findReferences, goToDefinition)
  // require real vscode.Uri / vscode.Position instances, so we register thin
  // wrappers that deserialize the JSON and forward the call.

  context.subscriptions.push(
    commands.registerCommand(
      "wowlua-ls.showReferences",
      (uriStr, position, locations) => {
        const uri = Uri.parse(uriStr);
        const pos = new Position(position.line, position.character);
        const locs = (locations || []).map(
          (loc) =>
            new Location(
              Uri.parse(loc.uri),
              new Range(
                new Position(loc.range.start.line, loc.range.start.character),
                new Position(loc.range.end.line, loc.range.end.character)
              )
            )
        );
        commands.executeCommand("editor.action.showReferences", uri, pos, locs);
      }
    )
  );

  context.subscriptions.push(
    commands.registerCommand(
      "wowlua-ls.showImplementations",
      (uriStr, position) => {
        const uri = Uri.parse(uriStr);
        const pos = new Position(position.line, position.character);
        commands.executeCommand("editor.action.findReferences", uri, pos);
      }
    )
  );

  context.subscriptions.push(
    commands.registerCommand(
      "wowlua-ls.showSuperDefinition",
      (uriStr, position) => {
        const uri = Uri.parse(uriStr);
        const pos = new Position(position.line, position.character);
        commands.executeCommand("editor.action.goToDefinition", uri, pos);
      }
    )
  );

  client = new LanguageClient("wowluals", "WoW Lua LS", serverOptions, clientOptions);
  client.start();
}

function deactivate() {
  if (client) {
    return client.stop();
  }
}

module.exports = { activate, deactivate };
