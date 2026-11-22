// Locates the wowlua_ls binary the extension runs. Kept free of the `vscode`
// module so `node --test` can exercise it (test/server-path.test.js).
const fs = require("fs");
const os = require("os");
const path = require("path");

const SERVER_PATH_SETTING = "wowlua-ls.server.path";

// Where a user who has to build or download a server is told to put its path.
// The setting is machine-scoped: in a remote window (container, WSL, SSH) only
// the Remote settings apply, since that is where the binary has to be.
const SET_IT = `set \`${SERVER_PATH_SETTING}\` in your User settings (Remote settings in a remote window) to its path`;

// A platform without a bundled server of its own whose OS runs another
// platform's binary: Windows 11 on Arm runs x64 programs under emulation.
const EMULATED_PLATFORMS = { "win32-arm64": "win32-x64" };

/**
 * Picks the server to run: the user's `wowlua-ls.server.path`, else the binary
 * bundled for this platform, else a development build next to the extension.
 *
 * @param {object} options
 * @param {string | undefined} options.configuredPath the setting's User-level value
 * @param {string} options.extRoot the extension's root directory
 * @param {string} options.platform `process.platform`
 * @param {string} options.arch `process.arch`
 * @returns {{ path: string } | { message: string }} the binary, or why there is
 *   none the extension can run and what the user can do about it
 */
function resolveServer({ configuredPath, extRoot, platform, arch }) {
  const exe = platform === "win32" ? "wowlua_ls.exe" : "wowlua_ls";
  const checkLoader = platform !== "win32";

  const configured = (configuredPath || "").trim();
  if (configured) {
    const file = expandHome(configured);
    const problem = unusableReason(file, checkLoader);
    if (problem) {
      return { message: `\`${SERVER_PATH_SETTING}\` is set to "${configured}", which ${problem}.` };
    }
    return { path: file };
  }

  const serverDir = path.join(extRoot, "server");
  const key = `${platform}-${arch}`;
  for (const dir of [key, EMULATED_PLATFORMS[key]]) {
    const bundled = dir && path.join(serverDir, dir, exe);
    if (!bundled || !isFile(bundled)) continue;
    const loader = checkLoader && missingLoader(bundled);
    if (loader) {
      return {
        message:
          `The bundled wowlua_ls server is built for glibc-based Linux and can't run here: it needs ` +
          `${loader}, which this system doesn't have (musl-based systems such as Alpine don't). ` +
          `Build wowlua_ls on this machine and ${SET_IT}.`,
      };
    }
    return { path: bundled };
  }

  // A release VSIX bundles a `server/` directory; without one this is a
  // development checkout of the extension, run against the repo's own build.
  if (isDirectory(serverDir)) {
    return {
      message: `wowlua-ls doesn't include a server for ${key}. Build wowlua_ls from source and ${SET_IT}.`,
    };
  }
  for (const profile of ["release", "debug"]) {
    const dev = path.join(extRoot, "..", "..", "target", profile, exe);
    if (isFile(dev)) return { path: dev };
  }
  return {
    message: `No wowlua_ls binary found. Run \`cargo build\` in the repo, or ${SET_IT}.`,
  };
}

/** Why `file` can't be run as the server, or null when nothing rules it out. */
function unusableReason(file, checkLoader) {
  // The client starts the server from the workspace folder, so a relative path
  // would be checked here but run from there: a program the workspace chooses.
  if (!path.isAbsolute(file)) return "is not an absolute path";
  if (!fs.existsSync(file)) return "doesn't exist";
  if (!isFile(file)) return "is not a file";
  if (checkLoader) {
    try {
      fs.accessSync(file, fs.constants.X_OK);
    } catch {
      return "is not executable (run `chmod +x` on it)";
    }
    const loader = missingLoader(file);
    if (loader) return `can't run on this system: it needs ${loader}, which doesn't exist here`;
  }
  return null;
}

/**
 * The dynamic loader a Linux binary names that this system lacks, or null.
 * Starting a binary whose loader is missing fails with a bare ENOENT, as if the
 * binary itself were missing: a glibc build on Alpine, or on NixOS.
 */
function missingLoader(file) {
  const loader = elfInterpreter(file);
  return loader && path.isAbsolute(loader) && !fs.existsSync(loader) ? loader : null;
}

const PT_INTERP = 3;

/**
 * The program interpreter (`PT_INTERP`) a 64-bit little-endian ELF file names,
 * or null for a file that isn't one or names none (a static binary).
 */
function elfInterpreter(file) {
  let fd;
  try {
    fd = fs.openSync(file, "r");
    const header = read(fd, 0, 64);
    if (!header || header.readUInt32BE(0) !== 0x7f454c46) return null;
    // EI_CLASS 2 = 64-bit, EI_DATA 1 = little-endian: every Linux target we build.
    if (header[4] !== 2 || header[5] !== 1) return null;
    const phoff = Number(header.readBigUInt64LE(32));
    const phentsize = header.readUInt16LE(54);
    const phnum = header.readUInt16LE(56);
    if (phentsize < 56) return null;
    for (let i = 0; i < phnum; i++) {
      const ph = read(fd, phoff + i * phentsize, 56);
      if (!ph) return null;
      if (ph.readUInt32LE(0) !== PT_INTERP) continue;
      const size = Number(ph.readBigUInt64LE(32));
      if (size === 0 || size > 4096) return null;
      const name = read(fd, Number(ph.readBigUInt64LE(8)), size);
      if (!name) return null;
      const nul = name.indexOf(0);
      return name.toString("latin1", 0, nul === -1 ? name.length : nul);
    }
    return null;
  } catch {
    return null;
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
  }
}

/** `length` bytes of `fd` at `position`, or null when the file is shorter. */
function read(fd, position, length) {
  const buf = Buffer.alloc(length);
  return fs.readSync(fd, buf, 0, length, position) === length ? buf : null;
}

function expandHome(file) {
  return file === "~" || file.startsWith("~/") || file.startsWith("~\\")
    ? path.join(os.homedir(), file.slice(1))
    : file;
}

function isFile(file) {
  try {
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

function isDirectory(file) {
  try {
    return fs.statSync(file).isDirectory();
  } catch {
    return false;
  }
}

module.exports = { SERVER_PATH_SETTING, resolveServer, elfInterpreter };
