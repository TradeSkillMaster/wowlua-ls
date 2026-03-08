const { workspace, window } = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const path = require("path");
const fs = require("fs");

let client;

function activate(context) {
  const config = workspace.getConfiguration("wowluals");
  let serverPath = config.get("serverPath");

  if (!serverPath) {
    // Try to find the binary relative to the extension
    const candidates = [
      path.join(__dirname, "../../target/debug/wowlua_ls"),
      path.join(__dirname, "../../target/release/wowlua_ls"),
    ];
    serverPath = candidates.find((p) => fs.existsSync(p));
    if (!serverPath) {
      window.showErrorMessage(
        "wowlua_ls binary not found. Run `cargo build` in the wowlua_ls repo, or set wowluals.serverPath in settings."
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

  client = new LanguageClient("wowluals", "WoW Lua LS", serverOptions, clientOptions);
  client.start();
}

function deactivate() {
  if (client) {
    return client.stop();
  }
}

module.exports = { activate, deactivate };
