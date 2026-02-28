const { workspace, window } = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const path = require("path");
const fs = require("fs");

let client;

function activate(context) {
  const config = workspace.getConfiguration("wowLs");
  let serverPath = config.get("serverPath");

  if (!serverPath) {
    // Try to find the binary relative to the extension
    const candidates = [
      path.join(__dirname, "../../target/debug/wow_ls"),
      path.join(__dirname, "../../target/release/wow_ls"),
    ];
    serverPath = candidates.find((p) => fs.existsSync(p));
    if (!serverPath) {
      window.showErrorMessage(
        "wow_ls binary not found. Run `cargo build` in the wow_ls repo, or set wowLs.serverPath in settings."
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

  client = new LanguageClient("wowLs", "WoW LS", serverOptions, clientOptions);
  client.start();
}

function deactivate() {
  if (client) {
    return client.stop();
  }
}

module.exports = { activate, deactivate };
