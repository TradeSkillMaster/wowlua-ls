#!/usr/bin/env python3
"""
Generate LuaLS annotation stubs for classic-only WoW APIs by scraping
the Warcraft wiki (warcraft.wiki.gg).

Usage (from repo root):
    python3 generate_classic_stubs.py [--include-undocumented]

This script:
1. Downloads GlobalAPI, FrameXML, and Frames lists from BlizzardInterfaceResources for retail, classic_era, and classic
2. Diffs them to find APIs/frames that only exist in classic versions
3. Filters out APIs already covered by existing stubs (vscode-wow-api + overrides)
4. Bulk-exports wiki pages for the missing function APIs
5. Parses {{apisig}} and parameter/return markup into ---@param/---@return annotations
6. Generates global frame variable stubs for classic-only frames
7. Writes stubs/classic/ClassicGlobals.lua

Requires: Python 3.8+ (stdlib only, no pip dependencies)
"""

import argparse
import os
import re
import sys
import time
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

WIKI_EXPORT_URL = "https://warcraft.wiki.gg/wiki/Special:Export"
RESOURCE_URL = "https://raw.githubusercontent.com/Ketho/BlizzardInterfaceResources/{branch}/Resources/{file}"
USER_AGENT = "wowlua-ls-stub-generator/1.0"
BATCH_SIZE = 50  # wiki export batch size
MW_NS = {"mw": "http://www.mediawiki.org/xml/export-0.11/"}

# Type mappings from wiki markup to LuaLS types
TYPE_MAP = {
    "bool": "boolean",
    "Boolean": "boolean",
    "String": "string",
    "Number": "number",
    "Table": "table",
    "Function": "function",
    "Frame": "Frame",
    "Object": "table",
    "unknown": "any",
    "unk": "any",
    "any": "any",
    "nil": "nil",
    "UnitId": "UnitToken",
    "UnitToken": "UnitToken",
    "fileID": "number",
    "BigUInteger": "number",
    "ClassFile": "string",
    "WOWGUID": "string",
}


def fetch_url(url, data=None):
    """Fetch a URL with a proper User-Agent."""
    if data is not None:
        data = urllib.parse.urlencode(data).encode()
    req = urllib.request.Request(url, data=data, headers={"User-Agent": USER_AGENT})
    resp = urllib.request.urlopen(req, timeout=60)
    return resp.read().decode("utf-8")


def parse_global_api(text):
    """Extract function names from a GlobalAPI.lua file."""
    return set(re.findall(r'"(\w+)"', text))


def get_existing_stubs(stubs_dir, exclude_dir=None):
    """Find function names already defined in existing stub files."""
    existing = set()
    if exclude_dir:
        exclude_dir = os.path.normpath(exclude_dir)
    for root, _dirs, files in os.walk(stubs_dir):
        if exclude_dir and os.path.normpath(root).startswith(exclude_dir):
            continue
        for f in files:
            if f.endswith(".lua"):
                path = os.path.join(root, f)
                with open(path) as fh:
                    content = fh.read()
                existing.update(re.findall(r"^function (\w+)\s*\(", content, re.MULTILINE))
    return existing


def get_existing_globals(stubs_dir, exclude_dir=None):
    """Find global variable names already defined in existing stub files."""
    existing = set()
    if exclude_dir:
        exclude_dir = os.path.normpath(exclude_dir)
    for root, _dirs, files in os.walk(stubs_dir):
        if exclude_dir and os.path.normpath(root).startswith(exclude_dir):
            continue
        for f in files:
            if f.endswith(".lua"):
                path = os.path.join(root, f)
                with open(path) as fh:
                    content = fh.read()
                # Match global assignments like "VarName = " and function defs
                existing.update(re.findall(r"^(\w+)\s*=\s*", content, re.MULTILINE))
                existing.update(re.findall(r"^function (\w+)\s*\(", content, re.MULTILINE))
    return existing


def fetch_wiki_pages(api_names):
    """Bulk-export wiki pages for the given API names."""
    pages = {}
    names = list(api_names)

    for i in range(0, len(names), BATCH_SIZE):
        batch = names[i : i + BATCH_SIZE]
        pages_text = "\n".join(f"API {n}" for n in batch)
        print(f"  Fetching wiki batch {i // BATCH_SIZE + 1} ({len(batch)} APIs)...", file=sys.stderr)

        xml_text = fetch_url(WIKI_EXPORT_URL, {"pages": pages_text, "curonly": "1"})
        root = ET.fromstring(xml_text)

        for page in root.findall(".//mw:page", MW_NS):
            title = page.find("mw:title", MW_NS).text
            redirect = page.find("mw:redirect", MW_NS)
            if redirect is not None:
                continue  # skip redirects
            text_el = page.find(".//mw:text", MW_NS)
            if text_el is not None and text_el.text:
                api_name = title.replace("API ", "")
                pages[api_name] = text_el.text

        if i + BATCH_SIZE < len(names):
            time.sleep(1)  # be polite to the wiki

    return pages


def normalize_type(t):
    """Normalize a wiki type string to a LuaLS type."""
    t = t.strip()
    if not t:
        return "any"
    # Handle Enum.X references
    if t.startswith("Enum."):
        return t
    # Handle array notation
    is_array = t.endswith("[]")
    if is_array:
        t = t[:-2]
    # Handle union types (separated by |)
    parts = [p.strip() for p in t.split("|")]
    mapped = []
    for p in parts:
        mapped.append(TYPE_MAP.get(p, p))
    result = "|".join(mapped)
    if is_array:
        result += "[]"
    return result


def parse_wikitext(api_name, wikitext):
    """Parse wikitext into a function signature with @param/@return annotations."""
    # Check for embedded luals annotations
    luals_match = re.search(r"<!-- luals\n(.*?)\n-->", wikitext, re.DOTALL)
    if luals_match:
        return luals_match.group(1)

    # Parse {{apisig|...}} — replace {{=}} first so the regex isn't confused by nested braces
    wikitext_clean = wikitext.replace("{{=}}", "=")
    sig_match = re.search(r"\{\{apisig\|(.+?)\}\}", wikitext_clean, re.DOTALL)
    if not sig_match:
        return None

    sig_text = sig_match.group(1).replace("\n", " ")

    # Split into returns and function call
    if "=" in sig_text:
        ret_part, call_part = sig_text.split("=", 1)
        ret_names = [r.strip().rstrip(",") for r in ret_part.split(",") if r.strip()]
        # Handle varargs in returns like "subClass1, subClass2, ..."
        has_vararg_return = "..." in ret_part
        if has_vararg_return:
            ret_names = [r for r in ret_names if r != "..."]
    else:
        ret_names = []
        call_part = sig_text
        has_vararg_return = False

    # Extract function name and args from call
    call_match = re.match(r"\s*(\w[\w.]*)\s*\(([^)]*)\)", call_part)
    if not call_match:
        return None

    func_name = call_match.group(1)
    args_text = call_match.group(2).strip()

    # Clean optional brackets from args: "foo [, bar]" -> "foo, bar"
    # and track which params are optional
    orig_args = call_match.group(2).strip()
    optional_params = set()
    for m in re.finditer(r"\[\s*,?\s*(\w+)", orig_args):
        optional_params.add(m.group(1))
    # Replace "[, param" with ", param" and remove closing brackets
    args_text = re.sub(r"\[\s*,\s*", ", ", args_text)
    args_text = re.sub(r"\[\s*", "", args_text)
    args_text = args_text.replace("]", "").strip()

    if args_text and args_text != "...":
        arg_names = [a.strip() for a in args_text.split(",") if a.strip()]
        has_vararg_param = "..." in args_text
        arg_names = [a for a in arg_names if a != "..."]
    elif args_text == "...":
        arg_names = []
        has_vararg_param = True
    else:
        arg_names = []
        has_vararg_param = False

    # Parse parameter and return type annotations from wikitext
    section = None  # "args" or "returns"
    param_types = {}  # name -> (type, optional)
    return_types = {}  # name -> (type, optional)

    for line in wikitext.split("\n"):
        line_stripped = line.strip()
        line_lower = line_stripped.lower()
        # Detect section headers
        section_match = re.match(r"==+\s*(.+?)\s*==+", line_lower)
        if section_match:
            sec = section_match.group(1)
            if any(k in sec for k in ("arg", "param", "input")):
                section = "args"
            elif any(k in sec for k in ("ret", "val", "output", "result")):
                section = "returns"
            else:
                section = None
            continue

        # Parse param lines like :;name:{{apitype|type}} or :;name:type description
        if line_stripped.startswith(":;"):
            # Strip numbering like ":;1. name:"
            clean = re.sub(r"^:;\d+\.\s*", ":;", line_stripped)
            # Strip hyperlinks [[...|text]] or [[text]]
            clean = re.sub(r"\[\[(?:[^|\]]*\|)?([^\]]*)\]\]", r"\1", clean)

            name = None
            typ = None
            optional = False

            # Try: :;name:{{apitype|type}}
            pm = re.match(r":;(\w+)\s*[:,]\s*\{\{apitype\|([^}]+)\}\}", clean)
            if pm:
                name = pm.group(1)
                typ = pm.group(2).strip()
            else:
                # Try: :;name:type - description (no apitype template)
                pm2 = re.match(r":;(\w+)\s*[:,]\s*(\w[\w|.]*)", clean)
                if pm2:
                    candidate_type = pm2.group(2)
                    # Only accept if it looks like a type
                    if candidate_type.lower() in (
                        "boolean", "number", "string", "table", "function",
                        "nil", "any", "frame", "integer", "float",
                    ):
                        name = pm2.group(1)
                        typ = candidate_type

            if name and typ:
                optional = "?" in typ
                typ = typ.replace("?", "")
                # Handle multiple types separated by | within apitype
                typ = typ.split("|")[0] if "|" in typ else typ
                if section == "args":
                    param_types[name] = (normalize_type(typ), optional)
                elif section == "returns":
                    return_types[name] = (normalize_type(typ), optional)

    # Build the annotation string
    lines = []
    lines.append(f"---[Documentation](https://warcraft.wiki.gg/wiki/API_{api_name})")

    for arg in arg_names:
        typ, optional = param_types.get(arg, ("any", False))
        if arg in optional_params:
            optional = True
        opt = "?" if optional else ""
        lines.append(f"---@param {arg}{opt} {typ}")

    if has_vararg_param:
        lines.append("---@param ... any")

    for ret in ret_names:
        typ, optional = return_types.get(ret, ("any", False))
        opt = "?" if optional else ""
        lines.append(f"---@return {typ}{opt} {ret}")

    if has_vararg_return and not ret_names:
        lines.append("---@return any ...")

    # Build function signature
    all_args = list(arg_names)
    if has_vararg_param:
        all_args.append("...")
    args_str = ", ".join(all_args)
    lines.append(f"function {func_name}({args_str}) end")

    return "\n".join(lines)


def generate_stub_for_undocumented(api_name):
    """Generate a minimal stub for an API without a wiki page."""
    return f"---[Documentation](https://warcraft.wiki.gg/wiki/API_{api_name})\nfunction {api_name}(...) end"


def generate_stub_for_frame(frame_name):
    """Generate a stub for a global frame variable."""
    return f"---@type any\n{frame_name} = nil"


def fetch_resource(branch, filename):
    """Fetch a resource file from BlizzardInterfaceResources."""
    url = RESOURCE_URL.format(branch=branch, file=filename)
    try:
        return parse_global_api(fetch_url(url))
    except Exception as e:
        print(f"  Warning: could not fetch {filename} from {branch}: {e}", file=sys.stderr)
        return set()


def main():
    parser = argparse.ArgumentParser(description="Generate classic WoW API stubs")
    parser.add_argument(
        "--output",
        default=None,
        help="Output .lua file path (default: stubs/overrides/ClassicGlobals.lua)",
    )
    parser.add_argument(
        "--stubs-dir",
        default=None,
        help="Path to existing stubs directory (to avoid duplicates)",
    )
    parser.add_argument(
        "--include-undocumented",
        action="store_true",
        help="Include bare stubs for APIs without wiki pages",
    )
    args = parser.parse_args()

    # Default paths relative to this script's location (repo root)
    script_dir = os.path.dirname(os.path.abspath(__file__))
    stubs_dir = args.stubs_dir or os.path.join(script_dir, "stubs")
    output_dir = os.path.join(stubs_dir, "classic")
    os.makedirs(output_dir, exist_ok=True)
    output_path = args.output or os.path.join(output_dir, "ClassicGlobals.lua")

    # Step 1: Download resource lists from all branches
    print("Downloading resource lists...", file=sys.stderr)

    # GlobalAPI (functions + constants)
    retail = fetch_resource("live", "GlobalAPI.lua")
    classic_era = fetch_resource("classic_era", "GlobalAPI.lua")
    classic = fetch_resource("classic", "GlobalAPI.lua")

    all_classic_only = sorted((classic_era | classic) - retail)
    print(f"  Found {len(all_classic_only)} classic-only APIs", file=sys.stderr)

    # FrameXML (Lua-defined functions)
    retail_fxml = fetch_resource("live", "FrameXML.lua")
    classic_era_fxml = fetch_resource("classic_era", "FrameXML.lua")
    classic_fxml = fetch_resource("classic", "FrameXML.lua")

    classic_only_fxml = sorted((classic_era_fxml | classic_fxml) - retail_fxml)
    print(f"  Found {len(classic_only_fxml)} classic-only FrameXML functions", file=sys.stderr)

    # Frames (global frame variables)
    retail_frames = fetch_resource("live", "Frames.lua")
    classic_era_frames = fetch_resource("classic_era", "Frames.lua")
    classic_frames = fetch_resource("classic", "Frames.lua")

    classic_only_frames = sorted((classic_era_frames | classic_frames) - retail_frames)
    print(f"  Found {len(classic_only_frames)} classic-only frames", file=sys.stderr)

    # Step 2: Filter out already-covered APIs
    if stubs_dir and os.path.isdir(stubs_dir):
        existing = get_existing_stubs(stubs_dir, exclude_dir=output_dir)
        existing_globals = get_existing_globals(stubs_dir, exclude_dir=output_dir)
        missing = [n for n in all_classic_only if n not in existing]
        missing_fxml = [n for n in classic_only_fxml if n not in existing]
        missing_frames = [n for n in classic_only_frames if n not in existing_globals]
        print(
            f"  {len(all_classic_only) - len(missing)} APIs already in stubs, {len(missing)} to generate",
            file=sys.stderr,
        )
        print(
            f"  {len(classic_only_fxml) - len(missing_fxml)} FrameXML already in stubs, {len(missing_fxml)} to generate",
            file=sys.stderr,
        )
        print(
            f"  {len(classic_only_frames) - len(missing_frames)} frames already in stubs, {len(missing_frames)} to generate",
            file=sys.stderr,
        )
    else:
        missing = all_classic_only
        missing_fxml = classic_only_fxml
        missing_frames = classic_only_frames
        print("  No stubs dir found, generating all", file=sys.stderr)

    if not missing and not missing_fxml and not missing_frames:
        print("Nothing to generate!", file=sys.stderr)
        return

    # Step 3: Fetch wiki pages for GlobalAPI functions only
    # (FrameXML functions are Lua-defined helpers without wiki pages)
    if missing:
        print(f"Fetching wiki pages for {len(missing)} APIs...", file=sys.stderr)
        wiki_pages = fetch_wiki_pages(missing)
        print(f"  Got {len(wiki_pages)} wiki pages", file=sys.stderr)
    else:
        wiki_pages = {}

    # Step 4: Parse and generate
    print("Generating stubs...", file=sys.stderr)
    documented = []
    undocumented = []
    parse_failures = []

    for name in missing:
        if name in wiki_pages:
            result = parse_wikitext(name, wiki_pages[name])
            if result:
                documented.append((name, result))
            else:
                parse_failures.append(name)
                if args.include_undocumented:
                    undocumented.append(name)
        else:
            if args.include_undocumented:
                undocumented.append(name)

    # Step 5: Write output
    out_lines = ["---@meta _"]
    out_lines.append(
        "-- Classic-only WoW API stubs (auto-generated from warcraft.wiki.gg)"
    )
    out_lines.append("")

    for name, annotation in documented:
        out_lines.append(annotation)
        out_lines.append("")

    if undocumented:
        out_lines.append("-- Undocumented APIs (no wiki page or unparseable)")
        out_lines.append("")
        for name in undocumented:
            out_lines.append(generate_stub_for_undocumented(name))
            out_lines.append("")

    if missing_fxml:
        out_lines.append("-- Classic-only FrameXML functions")
        out_lines.append("")
        for name in missing_fxml:
            out_lines.append(f"function {name}(...) end")
            out_lines.append("")

    if missing_frames:
        out_lines.append("-- Classic-only global frames")
        out_lines.append("")
        for name in missing_frames:
            out_lines.append(generate_stub_for_frame(name))
            out_lines.append("")

    with open(output_path, "w") as f:
        f.write("\n".join(out_lines))

    print(f"\nResults:", file=sys.stderr)
    print(f"  Documented (with types): {len(documented)}", file=sys.stderr)
    print(f"  Parse failures: {len(parse_failures)}", file=sys.stderr)
    print(f"  No wiki page: {len(missing) - len(wiki_pages) - len(parse_failures)}", file=sys.stderr)
    if undocumented:
        print(f"  Undocumented API stubs: {len(undocumented)}", file=sys.stderr)
    if missing_fxml:
        print(f"  FrameXML function stubs: {len(missing_fxml)}", file=sys.stderr)
    if missing_frames:
        print(f"  Frame globals: {len(missing_frames)}", file=sys.stderr)
    print(f"  Written to: {output_path}", file=sys.stderr)

    if parse_failures:
        print(f"\nParse failures:", file=sys.stderr)
        for name in parse_failures[:20]:
            print(f"  {name}", file=sys.stderr)
        if len(parse_failures) > 20:
            print(f"  ... and {len(parse_failures) - 20} more", file=sys.stderr)


if __name__ == "__main__":
    main()
