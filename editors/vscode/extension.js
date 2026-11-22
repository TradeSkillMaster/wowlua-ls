const vscode = require("vscode");
const { workspace, window, commands, env, Uri, Position, Range, Location } = vscode;
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const path = require("path");
const { SERVER_PATH_SETTING, resolveServer } = require("./server-path");

const BUILD_DOCS_URL =
  "https://tradeskillmaster.github.io/wowlua-ls/guide/getting-started#platforms-without-a-bundled-server";

let client;
// `client.start()`'s promise, settled either way. vscode-languageclient can't
// stop a client that is still starting (it throws, leaving the server running),
// so stopping waits for this first. A failed start has already been reported.
let clientStarted = Promise.resolve();
// Restarts run one after another, so two quick setting edits (the Settings UI
// writes as you type) can't leave two clients running.
let restarting = Promise.resolve();
let deactivated = false;

function activate(context) {
  registerCommands(context);
  context.subscriptions.push(
    workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration(SERVER_PATH_SETTING)) restartServer();
    })
  );
  startServer();
}

function startServer() {
  // Only the User (or Remote) value: the setting names a program to run, so a
  // workspace must not be able to choose it. The `machine` scope in
  // package.json already keeps workspace values out; reading `globalValue`
  // states that here instead of relying on the declaration alone.
  const configuredPath = workspace.getConfiguration().inspect(SERVER_PATH_SETTING)?.globalValue;
  const server = resolveServer({
    configuredPath: typeof configuredPath === "string" ? configuredPath : undefined,
    extRoot: path.resolve(__dirname, ".."),
    platform: process.platform,
    arch: process.arch,
  });
  if (!server.path) {
    showServerError(server.message);
    return;
  }

  const serverOptions = {
    command: server.path,
    transport: TransportKind.stdio,
  };

  const clientOptions = {
    documentSelector: [
      { scheme: "file", language: "lua" },
      { scheme: "file", language: "toc" },
    ],
  };

  client = new LanguageClient("wowluals", "WoW Lua LS", serverOptions, clientOptions);
  clientStarted = client.start().catch(() => {});
}

async function stopServer() {
  const running = client;
  client = undefined;
  if (!running) return;
  await clientStarted;
  await running.stop().catch(() => {});
}

function restartServer() {
  restarting = restarting.then(async () => {
    await stopServer();
    if (!deactivated) startServer();
  });
}

async function showServerError(message) {
  const choice = await window.showErrorMessage(message, "Open Settings", "Build Instructions");
  if (choice === "Open Settings") {
    commands.executeCommand("workbench.action.openSettings", SERVER_PATH_SETTING);
  } else if (choice === "Build Instructions") {
    env.openExternal(Uri.parse(BUILD_DOCS_URL));
  }
}

function registerCommands(context) {
  // The LSP server emits code-lens Commands whose arguments are plain JSON.
  // VS Code built-in commands (showReferences, findReferences, goToDefinition)
  // require real vscode.Uri / vscode.Position instances, so we register thin
  // wrappers that deserialize the JSON and forward the call.

  // Deserialize an LSP `Location[]` command argument (plain JSON) into real
  // vscode.Location instances. Shared by the wrappers so they decode Location
  // payloads identically.
  const toLocations = (locations) =>
    (locations || []).map(
      (loc) =>
        new Location(
          Uri.parse(loc.uri),
          new Range(
            new Position(loc.range.start.line, loc.range.start.character),
            new Position(loc.range.end.line, loc.range.end.character)
          )
        )
    );

  context.subscriptions.push(
    commands.registerCommand(
      "wowlua-ls.showReferences",
      (uriStr, position, locations) => {
        const uri = Uri.parse(uriStr);
        const pos = new Position(position.line, position.character);
        const locs = toLocations(locations);
        commands.executeCommand("editor.action.showReferences", uri, pos, locs);
      }
    )
  );

  context.subscriptions.push(
    commands.registerCommand(
      "wowlua-ls.showSuperDefinition",
      (uriStr, position, locations) => {
        const uri = Uri.parse(uriStr);
        const pos = new Position(position.line, position.character);
        const locs = toLocations(locations);
        // The server resolves the overridden definition and embeds its
        // Location(s); jump to them (mirrors the JetBrains GoToLocationsAction).
        commands.executeCommand(
          "editor.action.goToLocations",
          uri,
          pos,
          locs,
          "goto",
          "No super definition found"
        );
      }
    )
  );
}

function deactivate() {
  deactivated = true;
  return restarting.then(stopServer);
}

module.exports = { activate, deactivate };
