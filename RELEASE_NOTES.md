### New

- **`@creates-global N` annotation** — Mark a function whose call creates a named global as a side effect (like WoW's `CreateFrame("Frame", "MyFrame")` defining `_G.MyFrame`). Reading that global in another file no longer triggers a false `undefined-global`, and it's typed from the call's actual return type — so `CreateFrame("Frame", "MyFrame", parent, "MyTemplate")` yields `Frame & MyTemplate`, not a bare `Frame`. This replaces the previous hard-coded handling. ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html))
- **Class-name completions for backtick-generic parameters** — Typing inside the quotes of a backtick-generic string argument (e.g. `CreateFrame("")`) now suggests class names. When the generic is constrained (`@generic T: Base`), only classes satisfying the constraint are offered. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/generics.html))

### Improvements

- Mixin methods are now scanned from the Classic `wow-ui-source` branches, improving type coverage for Classic and Classic Era APIs (previously retail-only).

### Bug Fixes

- Fixed `undefined-field` false positives on mixin-embed union types.
- Fixed completions for loop variables and for recursive references within a function's own body.
- Fixed auto-inserted `end` landing in the wrong place after a closing paren on function arguments.
- `library` directories now suppress diagnostics across the whole subtree, including nested `.wowluarc.json` files inside them — a vendored library shipping its own config can no longer re-enable diagnostics for itself.
- Backtick generics now resolve correctly when passed through a constrained type-variable argument.
- Fixed syntax highlighting of dotted method definitions (e.g. `Foo.Bar:method`) — the segment before the `:` is no longer mis-colored as a class name.
