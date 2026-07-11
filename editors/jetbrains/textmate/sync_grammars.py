#!/usr/bin/env python3
"""Generate the JetBrains-vendored TextMate grammars from the VS Code ones.

The vendored copies are GENERATED — do not edit them by hand, and do not
copy the VS Code grammars over verbatim. Run this script after any change
to editors/vscode/syntaxes/{lua,toc}.tmLanguage.json.

Why a transform instead of a straight copy: IntelliJ's TextMate engine
mishandles the capture-with-nested-patterns construct
(`"captures": {"N": {"patterns": [...]}}`). When such a rule matches inside
an open begin/end region (our `---` doc-comment region), the engine loses
one pop of the enclosing region, permanently leaking a scope onto the rule
stack. Files with long runs of annotation lines (WoW API stubs) accumulate
~1-2 leaked scopes per line until the engine stops emitting tokens and
syntax coloring dies mid-file. Verified against the real engines driven
standalone: 2025.2 leaks when the capture has no `name`; 2026.1 and the
2026.2 EAP leak regardless of `name`.

The transform flattens each such capture to a plain named scope (dropping
the nested tokenization of type-expression internals — union pipes,
generic brackets, etc. render in the flat type color, which is close to
what IntelliJ's scope->color mapping shows anyway). VS Code's engine
handles the construct correctly, so the VS Code grammar keeps the richer
form.
"""

import json
import sys
from pathlib import Path

# Scope used when a flattened capture has no usable name. All current
# capture-with-patterns sites tokenize annotation type expressions, so a
# type scope is the right flat fallback. The meta.* wrapper (added for the
# 2025.2 engine, harmless in VS Code) is unstyled by themes and would
# render as plain text if kept — map it to the type scope instead.
FLAT_FALLBACK = "support.type.lua"
UNSTYLED_WRAPPERS = {"meta.type.annotation.lua"}


def flatten(node) -> int:
    count = 0
    if isinstance(node, dict):
        for capkey in ("captures", "beginCaptures", "endCaptures"):
            caps = node.get(capkey)
            if isinstance(caps, dict):
                for k, v in caps.items():
                    if isinstance(v, dict) and "patterns" in v:
                        name = v.get("name")
                        if not name or name in UNSTYLED_WRAPPERS:
                            name = FLAT_FALLBACK
                        caps[k] = {"name": name}
                        count += 1
        for v in node.values():
            count += flatten(v)
    elif isinstance(node, list):
        for v in node:
            count += flatten(v)
    return count


def main() -> None:
    here = Path(__file__).resolve().parent
    vscode_syntaxes = here.parent.parent / "vscode" / "syntaxes"
    for lang in ("lua", "toc"):
        src = vscode_syntaxes / f"{lang}.tmLanguage.json"
        dst = here / lang / "syntaxes" / f"{lang}.tmLanguage.json"
        grammar = json.loads(src.read_text(encoding="utf-8"))
        flattened = flatten(grammar)
        dst.write_text(
            json.dumps(grammar, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        print(f"{lang}: wrote {dst.relative_to(here.parent)} ({flattened} captures flattened)")


if __name__ == "__main__":
    sys.exit(main())
