### New

- **AceEvent-3.0 event handlers are now typed from the event payload.** An inline callback passed to `self:RegisterEvent("BAG_UPDATE", function(event, bagID) end)` gets its `event` (string) and payload parameters typed automatically. A handler passed **by name** (`"OnBagUpdate"`) has that method's unannotated parameters typed from the payload too. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/events.html))
- **New `keyof T` type.** `keyof self` / `keyof ClassName` is a string constrained to one of the target's field/method names — with completion, go-to-definition, and hover on the string literal, plus a `type-mismatch` when the literal isn't a member. ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html#keyof-t))
- **Optional LSP4IJ backend for the JetBrains plugin.** The plugin can now run on the LSP4IJ client (opt-in via settings), with automatic fallback on IDEs that lack the built-in LSP module (e.g. Community Edition).

### Bug Fixes

- Fixed cross-addon namespace pollution in multi-addon workspaces, where classes and globals from one addon could leak into another addon sharing the workspace.
- FrameXML static factories (e.g. `UiMapPoint.CreateFromCoordinates`) now return their declared class instead of an anonymous table shape, fixing a spurious `type-mismatch` at call sites and pinning the nominal type on hover.

### Improvements

- Removed the `serverPath` server-binary setting from the VS Code and JetBrains plugins.
- The "Analyzing…" progress message now shows the filename being analyzed.
