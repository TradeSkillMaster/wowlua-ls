**New**

- World of Warcraft: Forever flavor. Opt in with `"forever"` in `flavors` or a Forever `## Interface:` version, and `wrong-flavor-api` checks your code against Forever's API. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/flavor-filtering.html#world-of-warcraft-forever))
- `EnumUtil.MakeEnum` enums are now typed: members complete, typos get `undefined-field`, and `---@enum` above the call names the enum. The new `@returns-enum` annotation does the same for your own enum functions. ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html#returns-enum))

**Bug Fixes**

- Call hierarchy listed no incoming or outgoing calls, and call/type hierarchy left out built-ins like `CreateFrame` and `Frame`.
- Locals declared in a `repeat` body were flagged as `undefined-global` in the `until` condition (#62).
- False `undefined-field` when reading `t.key` after a string-key write `t["key"] = v`.
- XML `parentKey` fields are recognized on every UI element and typed as that element (false `undefined-field` on e.g. a `<PlayerModel>` field's `SetUnit`).
- `<NormalTexture/>`-style children without a `parentKey` no longer create a field the game never sets (#63).
- `GameTooltip:SetInboxItem` and other tooltip data accessors accept retail's full argument lists (false `redundant-parameter`).
- Cross-file inferred types no longer depend on which files were analyzed first (hover and diagnostics could differ between runs).

**Improvements**

- Globals assigned a function call, a reference, or a table constructor are now typed in other files (`MainFrame = CreateFrame("Frame")` is a `Frame`, and a constructor's entries are typed fields).
