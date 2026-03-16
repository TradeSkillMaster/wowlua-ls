#!/usr/bin/env python3
"""
Generate LuaLS annotation stubs for WoW global variables from vscode-wow-api data.

Usage (from repo root):
    python3 generate_global_stubs.py

Reads globals.ts (known global names) and globalstring/enUS.ts (string values)
from stubs/vscode-wow-api/src/data/, filters out names already defined in the
Lua annotation stubs, and writes two files:
  - stubs/overrides/GlobalStrings.lua   — string constants with actual values
  - stubs/overrides/GlobalVariables.lua — remaining globals (frames, mixins, etc.)

Requires: Python 3.8+ (stdlib only, no pip dependencies)
"""

import os
import re
import sys


def get_existing_names(stubs_dir):
    """Find names already defined in existing Lua stub files."""
    existing = set()
    for root, _dirs, files in os.walk(stubs_dir):
        for f in files:
            if f.endswith(".lua"):
                path = os.path.join(root, f)
                with open(path) as fh:
                    content = fh.read()
                existing.update(re.findall(r"^function (\w+)", content, re.MULTILINE))
                existing.update(re.findall(r"^(\w+)\s*=", content, re.MULTILINE))
                existing.update(re.findall(r"---@class\s+(\w+)", content))
    return existing


def escape_lua_string(s):
    """Escape a string for use in a Lua double-quoted string literal.

    Input comes from TypeScript String.raw`` which preserves backslashes literally,
    so \\\" in the source means the actual string contains \", and \\n means literal
    backslash-n (not a newline). We unescape the TS-level escapes first, then
    re-escape for Lua.
    """
    # Unescape TS String.raw sequences: \\" -> ", \\n -> newline, \\\\ -> backslash
    s = s.replace('\\"', '"')
    s = s.replace("\\n", "\n")
    s = s.replace("\\r", "\r")
    s = s.replace("\\t", "\t")
    s = s.replace("\\\\", "\\")
    # Now escape for Lua
    s = s.replace("\\", "\\\\")
    s = s.replace('"', '\\"')
    s = s.replace("\n", "\\n")
    s = s.replace("\r", "\\r")
    s = s.replace("\t", "\\t")
    return s


def main():
    script_dir = os.path.dirname(os.path.abspath(__file__))
    data_dir = os.path.join(script_dir, "stubs", "vscode-wow-api", "src", "data")
    stubs_dir = os.path.join(script_dir, "stubs")
    overrides_dir = os.path.join(stubs_dir, "overrides")

    # Parse globals.ts
    with open(os.path.join(data_dir, "globals.ts")) as f:
        all_globals = set(
            n
            for n in re.findall(r'"([^"]+)":\s*true', f.read())
            if re.match(r"^[A-Za-z_][A-Za-z0-9_]*$", n)
        )

    # Parse enUS.ts
    with open(os.path.join(data_dir, "globalstring", "enUS.ts")) as f:
        text = f.read()
    globalstrings = {}
    for m in re.finditer(r'(?:"([^"]+)"|(\w+)):\s*String\.raw`([^`]*)`', text):
        name = m.group(1) or m.group(2)
        value = m.group(3)
        if re.match(r"^[A-Za-z_][A-Za-z0-9_]*$", name):
            globalstrings[name] = value

    # Filter out names already in stubs (but exclude our own output files)
    existing = set()
    for root, _dirs, files in os.walk(stubs_dir):
        for f in files:
            if f.endswith(".lua") and f not in ("GlobalStrings.lua", "GlobalVariables.lua"):
                path = os.path.join(root, f)
                with open(path) as fh:
                    content = fh.read()
                existing.update(re.findall(r"^function (\w+)", content, re.MULTILINE))
                existing.update(re.findall(r"^(\w+)\s*=", content, re.MULTILINE))
                existing.update(re.findall(r"---@class\s+(\w+)", content))

    missing = sorted(all_globals - existing)

    strings_out = []
    vars_out = []

    for name in missing:
        if name in globalstrings:
            strings_out.append((name, globalstrings[name]))
        else:
            vars_out.append(name)

    # Write GlobalStrings.lua
    lines = ["---@meta _", "-- WoW global string constants (auto-generated from vscode-wow-api enUS data)", ""]
    for name, value in strings_out:
        escaped = escape_lua_string(value)
        lines.append(f'{name} = "{escaped}"')
    strings_path = os.path.join(overrides_dir, "GlobalStrings.lua")
    with open(strings_path, "w") as f:
        f.write("\n".join(lines) + "\n")

    # Write GlobalVariables.lua
    lines = ["---@meta _", "-- WoW global variables (auto-generated from vscode-wow-api globals data)", ""]
    for name in vars_out:
        lines.append(f"---@type any")
        lines.append(f"{name} = nil")
    vars_path = os.path.join(overrides_dir, "GlobalVariables.lua")
    with open(vars_path, "w") as f:
        f.write("\n".join(lines) + "\n")

    print(f"GlobalStrings.lua:   {len(strings_out)} string constants", file=sys.stderr)
    print(f"GlobalVariables.lua: {len(vars_out)} global variables", file=sys.stderr)
    print(f"Written to: {overrides_dir}/", file=sys.stderr)


if __name__ == "__main__":
    main()
