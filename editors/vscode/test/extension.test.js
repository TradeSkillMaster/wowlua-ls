// Run with `npm run test:extension`. Loads extension.js against stand-ins for
// the `vscode` module and vscode-languageclient, which only exist inside VS Code.
const test = require("node:test");
const assert = require("node:assert");
const fs = require("fs");
const os = require("os");
const path = require("path");
const Module = require("module");

/**
 * A stand-in LanguageClient that, like vscode-languageclient 9.0.1, refuses to
 * stop until it has finished starting. Tracks which servers are still running.
 */
function fakeLanguageClient(running) {
  let created = 0;
  return class FakeLanguageClient {
    constructor() {
      this.id = ++created;
      this.state = "initial";
    }
    start() {
      this.state = "starting";
      running.add(this.id);
      return new Promise((resolve) =>
        setTimeout(() => {
          this.state = "running";
          resolve();
        }, 20)
      );
    }
    async stop() {
      if (this.state !== "running") throw new Error(`Client is not running and can't be stopped: ${this.state}`);
      this.state = "stopped";
      running.delete(this.id);
    }
  };
}

/** Loads a fresh copy of the extension; returns it and its config-change listener. */
function loadExtension(t, serverPath, running) {
  let onConfigChange;
  const vscode = {
    workspace: {
      onDidChangeConfiguration: (listener) => {
        onConfigChange = listener;
        return { dispose() {} };
      },
      getConfiguration: () => ({ inspect: () => ({ globalValue: serverPath }) }),
    },
    window: { showErrorMessage: async () => undefined },
    commands: { registerCommand: () => ({ dispose() {} }), executeCommand() {} },
    env: {},
    Uri: {},
    Position: class {},
    Range: class {},
    Location: class {},
  };
  const languageClient = { LanguageClient: fakeLanguageClient(running), TransportKind: {} };
  const originalLoad = Module._load;
  t.mock.method(Module, "_load", function (request, ...rest) {
    if (request === "vscode") return vscode;
    if (request === "vscode-languageclient/node") return languageClient;
    return originalLoad.call(this, request, ...rest);
  });
  const extensionPath = require.resolve("../extension");
  delete require.cache[extensionPath];
  const extension = require(extensionPath);
  extension.activate({ subscriptions: [] });
  return { extension, changeSetting: () => onConfigChange({ affectsConfiguration: () => true }) };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 150));

function serverBinary(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "wowlua-ext-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, process.platform === "win32" ? "wowlua_ls.exe" : "wowlua_ls");
  fs.writeFileSync(file, "", { mode: 0o755 });
  return file;
}

test("quick setting edits while the server starts leave one server running", async (t) => {
  const running = new Set();
  const { changeSetting } = loadExtension(t, serverBinary(t), running);
  // The Settings UI writes the setting as the user types.
  changeSetting();
  changeSetting();
  changeSetting();
  await settle();
  assert.strictEqual(running.size, 1, `servers still running: ${[...running]}`);
});

test("deactivating during a restart leaves no server running", async (t) => {
  const running = new Set();
  const { extension, changeSetting } = loadExtension(t, serverBinary(t), running);
  changeSetting();
  await extension.deactivate();
  await settle();
  assert.deepStrictEqual([...running], []);
});
