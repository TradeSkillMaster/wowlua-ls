**New**

- Guard functions: a function whose body is a single `return <condition>` (`function Cache:IsLoaded() return self.items ~= nil end`) narrows its arguments at every call site, in other files too, with no annotation ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/type-guards.html#guard-functions)).
- `undefined-field` now checks intersection types declared with `@type`/`@param`/`@as` (`Frame & {test: boolean}`) and anonymous table shapes (`{name: string}`) ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#undefined-field)).
- Table shapes are checked structurally: a class instance passes when it has the shape's fields, so `Frame & {obj: MyWidget}` accepts the frame the widget is built from, and a missing or mistyped field is a `type-mismatch` ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/basic-annotations.html#anonymous-table-shapes)).
- Generic classes with a `table<K, V>` parent or a `[K] V` field take their key and value types from each use (`Registry<string, Frame>`), in fields, aliases, nested types and class parents alike ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/classes.html#dictionary-classes-table-k-v-parent)).
- XML: members an inline `<OnLoad>` handler assigns become frame fields, `parentArray` elements carry their `KeyValue`s and children, and Blizzard templates gain the members their `OnLoad` handlers set ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/xml-scanning.html)).
- `crossfile-types` subcommand lists the global and field paths whose type differs between files, to find silent cross-file type losses ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/development.html#cross-file-type-consistency)).

**Bug Fixes**

- Lists built on the addon namespace or a global table (`ns.queue`, `MyAddon.entries`) now keep their element type in other files, including across addon roots.
- Fields assigned a call or expression (`ns.frame = CreateFrame(...)`, `ns.IsRetail = ...`, saved variables an event handler fills, `@defclass` modules named through a local) now have the same type in every file.
- Module data tables (`Data.Names = { [101] = "a" }`) are typed by their entries in every file, and fields a base class writes in its methods read the same in subclasses and other files.
- A class redeclared bare on its constructor (`---@class Config` above `local cfg = {…}` in another file) keeps the `@field` types and checks of its full declaration.
- Members written through a frame field (`self.icon.scrollable = true`) or a `---@class` local's frame field no longer leak onto every frame of that widget type.
- Hover on a field the LS can't resolve shows its type or `?` instead of nothing, and a field typed only by `x or Make()` writes or through a `---@class` local no longer hovers as `any`.
- Backward parameter inference no longer reports `undefined-field` for a member the inferred class lacks, and `{}` passes for a class whose fields are all optional (false `missing-fields`).
- Calls with `@overload`s are checked against the signature they match (or the closest one), so an argument only another overload rejects isn't reported; a possibly-nil argument is now `need-check-nil`, not `type-mismatch`.
- A `@cast` followed by a blank line or comment was ignored, and `@cast` now applies to the statement being typed (completion after `val.`).
- The spaced `--- @diagnostic` form now suppresses diagnostics, and the suppression quick fixes add to it.
- `GetObjectType`/`IsObjectType`/`@type-narrows` guards on a field (`self.region:GetObjectType() == "FontString"`) narrow that field rather than the variable, dot calls (`Frame.IsObjectType(f, "Button")`) narrow the first argument, and a same-named function on an untyped table narrows nothing.
- A field nil check in any operand of an `or` chain narrows the rest of the chain, and a field assigned a non-nil value in every branch of an `if` stays narrowed after it (false `need-check-nil`).
- A recursive parameterized alias (`---@alias Node<T> {value: T, next: Node<T>?}`) crashed the server.
- `.toc`: repeated list headers (`SavedVariables`, `Dependencies`, …) are no longer `toc-duplicate-header` and stay in completion, and their values may be separated by spaces, tabs or commas.
- Saving a `.toc` now refreshes open files' diagnostics at once (a new SavedVariable kept its `undefined-global` until the file was edited).

**Improvements**

- Completion no longer stalls, and CPU no longer spikes, in files with many `CreateFrame` calls; a completion typed during a workspace re-analysis is answered from the latest text.
- WoW API stubs refreshed from the latest wow-ui-source and BlizzardInterfaceResources snapshots; AceDB objects gain `callbacks`, `parent` and `children`.
