// Run with `npm run test:server-path`.
const test = require("node:test");
const assert = require("node:assert");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { resolveServer, elfInterpreter } = require("../server-path");

/** A fresh directory laid out like an installed extension, removed after the test. */
function extRoot(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "wowlua-ext-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

function writeFile(file, contents = "", mode = 0o755) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, contents, { mode });
  return file;
}

/**
 * A minimal 64-bit little-endian ELF file with a PT_LOAD program header and,
 * when `interp` is given, a PT_INTERP one naming it.
 */
function elf(interp) {
  const headers = interp === undefined ? 1 : 2;
  const interpOffset = 64 + 56 * headers;
  const name = interp === undefined ? Buffer.alloc(0) : Buffer.from(`${interp}\0`, "latin1");
  const buf = Buffer.alloc(interpOffset + name.length);
  buf.writeUInt32BE(0x7f454c46, 0);
  buf[4] = 2; // ELFCLASS64
  buf[5] = 1; // ELFDATA2LSB
  buf[6] = 1; // EV_CURRENT
  buf.writeBigUInt64LE(64n, 32); // e_phoff
  buf.writeUInt16LE(64, 52); // e_ehsize
  buf.writeUInt16LE(56, 54); // e_phentsize
  buf.writeUInt16LE(headers, 56); // e_phnum
  buf.writeUInt32LE(1, 64); // PT_LOAD
  if (interp !== undefined) {
    const ph = 64 + 56;
    buf.writeUInt32LE(3, ph); // PT_INTERP
    buf.writeBigUInt64LE(BigInt(interpOffset), ph + 8); // p_offset
    buf.writeBigUInt64LE(BigInt(name.length), ph + 32); // p_filesz
    name.copy(buf, interpOffset);
  }
  return buf;
}

const MISSING_LOADER = "/nonexistent-wowlua-test/ld-linux-x86-64.so.2";

test("runs the binary bundled for this platform", (t) => {
  const root = extRoot(t);
  const bundled = writeFile(path.join(root, "server", "linux-arm64", "wowlua_ls"));
  writeFile(path.join(root, "server", "linux-x64", "wowlua_ls"));
  assert.deepStrictEqual(resolveServer({ extRoot: root, platform: "linux", arch: "arm64" }), {
    path: bundled,
  });
});

test("Windows on Arm runs the x64 build under emulation", (t) => {
  const root = extRoot(t);
  const x64 = writeFile(path.join(root, "server", "win32-x64", "wowlua_ls.exe"));
  assert.deepStrictEqual(resolveServer({ extRoot: root, platform: "win32", arch: "arm64" }), {
    path: x64,
  });

  // A native build, once one ships, wins over emulation.
  const native = writeFile(path.join(root, "server", "win32-arm64", "wowlua_ls.exe"));
  assert.deepStrictEqual(resolveServer({ extRoot: root, platform: "win32", arch: "arm64" }), {
    path: native,
  });
});

test("names the platform and the setting when no server is bundled for it", (t) => {
  const root = extRoot(t);
  writeFile(path.join(root, "server", "linux-x64", "wowlua_ls"));
  for (const [platform, arch] of [
    ["linux", "riscv64"],
    ["linux", "arm"],
    ["freebsd", "x64"],
  ]) {
    const { path: found, message } = resolveServer({ extRoot: root, platform, arch });
    assert.strictEqual(found, undefined);
    assert.match(message, new RegExp(`doesn't include a server for ${platform}-${arch}\\b`));
    assert.match(message, /`wowlua-ls\.server\.path`/);
  }
});

test("a bundled glibc build on a system without its loader is reported, not spawned", (t) => {
  const root = extRoot(t);
  writeFile(path.join(root, "server", "linux-x64", "wowlua_ls"), elf(MISSING_LOADER));
  const { path: found, message } = resolveServer({ extRoot: root, platform: "linux", arch: "x64" });
  assert.strictEqual(found, undefined);
  assert.match(message, /built for glibc-based Linux/);
  assert.ok(message.includes(MISSING_LOADER), message);
  assert.match(message, /`wowlua-ls\.server\.path`/);
});

test("a bundled binary whose loader exists, or that needs none, runs", (t) => {
  const root = extRoot(t);
  const loader = writeFile(path.join(root, "lib", "ld-test.so"));
  const bundled = writeFile(path.join(root, "server", "linux-x64", "wowlua_ls"), elf(loader));
  assert.deepStrictEqual(resolveServer({ extRoot: root, platform: "linux", arch: "x64" }), {
    path: bundled,
  });

  writeFile(bundled, elf(undefined));
  assert.deepStrictEqual(resolveServer({ extRoot: root, platform: "linux", arch: "x64" }), {
    path: bundled,
  });
});

test("the loader check is skipped on Windows", (t) => {
  const root = extRoot(t);
  const bundled = writeFile(path.join(root, "server", "win32-x64", "wowlua_ls.exe"), elf(MISSING_LOADER));
  assert.deepStrictEqual(resolveServer({ extRoot: root, platform: "win32", arch: "x64" }), {
    path: bundled,
  });
});

test("the configured path wins over the bundled binary", (t) => {
  const root = extRoot(t);
  writeFile(path.join(root, "server", "linux-x64", "wowlua_ls"));
  const own = writeFile(path.join(root, "own", "wowlua_ls"));
  assert.deepStrictEqual(
    resolveServer({ configuredPath: `  ${own}  `, extRoot: root, platform: "linux", arch: "x64" }),
    { path: own }
  );
});

test("a blank configured path means the bundled binary", (t) => {
  const root = extRoot(t);
  const bundled = writeFile(path.join(root, "server", "linux-x64", "wowlua_ls"));
  for (const configuredPath of [undefined, "", "   "]) {
    assert.deepStrictEqual(
      resolveServer({ configuredPath, extRoot: root, platform: "linux", arch: "x64" }),
      { path: bundled }
    );
  }
});

test("a configured path that can't run says why, instead of falling back", (t) => {
  const root = extRoot(t);
  writeFile(path.join(root, "server", "linux-x64", "wowlua_ls"));
  const resolve = (configuredPath) =>
    resolveServer({ configuredPath, extRoot: root, platform: "linux", arch: "x64" });

  const missing = path.join(root, "nowhere", "wowlua_ls");
  assert.deepStrictEqual(resolve(missing), {
    message: `\`wowlua-ls.server.path\` is set to "${missing}", which doesn't exist.`,
  });

  const dir = path.join(root, "own");
  fs.mkdirSync(dir);
  assert.match(resolve(dir).message, /which is not a file\.$/);

  const wrongLoader = writeFile(path.join(root, "own", "glibc_build"), elf(MISSING_LOADER));
  const { message } = resolve(wrongLoader);
  assert.match(message, /can't run on this system/);
  assert.ok(message.includes(MISSING_LOADER), message);
});

test("a relative configured path is refused, since the server starts in the workspace", (t) => {
  const root = extRoot(t);
  writeFile(path.join(root, "server", "linux-x64", "wowlua_ls"));
  for (const configuredPath of ["wowlua_ls", ".cargo/bin/wowlua_ls", "./wowlua_ls", "~user/wowlua_ls"]) {
    assert.deepStrictEqual(
      resolveServer({ configuredPath, extRoot: root, platform: "linux", arch: "x64" }),
      { message: `\`wowlua-ls.server.path\` is set to "${configuredPath}", which is not an absolute path.` }
    );
  }
});

test("a configured path that isn't executable says so", { skip: process.platform === "win32" }, (t) => {
  const root = extRoot(t);
  const download = writeFile(path.join(root, "Downloads", "wowlua_ls"), "", 0o644);
  assert.match(
    resolveServer({ configuredPath: download, extRoot: root, platform: "linux", arch: "x64" }).message,
    /is not executable \(run `chmod \+x` on it\)/
  );
});

test("a configured path may start at the home directory", (t) => {
  const home = extRoot(t);
  t.mock.method(os, "homedir", () => home);
  const own = writeFile(path.join(home, "bin", "wowlua_ls"));
  assert.deepStrictEqual(
    resolveServer({ configuredPath: "~/bin/wowlua_ls", extRoot: extRoot(t), platform: "linux", arch: "x64" }),
    { path: own }
  );
});

test("a development checkout runs the repo's own build", (t) => {
  const repo = extRoot(t);
  const root = path.join(repo, "editors", "vscode");
  fs.mkdirSync(root, { recursive: true });

  const missing = resolveServer({ extRoot: root, platform: "linux", arch: "x64" });
  assert.match(missing.message, /^No wowlua_ls binary found\. Run `cargo build` in the repo/);

  const debug = writeFile(path.join(repo, "target", "debug", "wowlua_ls"));
  assert.deepStrictEqual(resolveServer({ extRoot: root, platform: "linux", arch: "x64" }), {
    path: debug,
  });
  const release = writeFile(path.join(repo, "target", "release", "wowlua_ls"));
  assert.deepStrictEqual(resolveServer({ extRoot: root, platform: "linux", arch: "x64" }), {
    path: release,
  });
});

test("elfInterpreter reads PT_INTERP and nothing else", (t) => {
  const root = extRoot(t);
  assert.strictEqual(
    elfInterpreter(writeFile(path.join(root, "dynamic"), elf("/lib/ld-linux-aarch64.so.1"))),
    "/lib/ld-linux-aarch64.so.1"
  );
  assert.strictEqual(elfInterpreter(writeFile(path.join(root, "static"), elf(undefined))), null);
  assert.strictEqual(elfInterpreter(writeFile(path.join(root, "script"), "#!/bin/sh\nexit 0\n")), null);
  assert.strictEqual(elfInterpreter(writeFile(path.join(root, "empty"), "")), null);
  assert.strictEqual(elfInterpreter(path.join(root, "absent")), null);
  // Cut off inside the interpreter's program header.
  const truncated = elf("/lib/ld-musl-x86_64.so.1").subarray(0, 64 + 56 + 20);
  assert.strictEqual(elfInterpreter(writeFile(path.join(root, "truncated"), truncated)), null);
});

test("elfInterpreter reads the loader a real Linux binary names", { skip: process.platform !== "linux" }, () => {
  const loader = elfInterpreter(process.execPath);
  // Node is dynamically linked on every mainstream distribution; a static
  // build names no loader at all.
  if (loader !== null) {
    assert.ok(path.isAbsolute(loader), loader);
    assert.ok(fs.existsSync(loader), loader);
  }
});
