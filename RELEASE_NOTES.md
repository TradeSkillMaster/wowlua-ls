**New**

- `unknown-cast-variable` diagnostic: reports a `@cast` that names a variable not in scope ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#unknown-cast-variable)).
- Lists built by inserting into `{}` (`table.insert`, `tinsert`, `list[#list + 1] =`) now get their element type from the inserted values, including across files, in fields, and on addon objects ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/why-wowlua-ls.html#list-element-inference)).
- `@stores-into N` annotation marks your own insert helpers so they type lists the same way ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html)).
- `WOW_PROJECT_ID == WOW_PROJECT_CAMELOT` narrows to WoW Forever. `WOW_PROJECT_MAINLINE` now means retail only, and the `_Forever` TOC suffix and the `forever` game type are recognized ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/flavor-filtering.html#world-of-warcraft-forever)).
- Blizzard XML templates missing from the stubs (newer retail, Glue UI and Classic-only templates) now have classes, so frames created from them are typed.

**Bug Fixes**

- Fields, methods and table entries defined in one file are now typed the same in every other file. This covers constructor instance fields, frames created inside functions, nested `@class` writes and namespace paths, exported local and module tables, methods on call-assigned fields, and template children on namespace frames.
- Fixed false `undefined-field` on classes defined through a local `ns.Foo` alias.
- A local read inside a function is now typed by every value the file assigns it, not only the one visible where the function is defined.
- Fields written through a `CreateFrame` local no longer leak onto every `Frame`, and `CreateFramePool("Button", …)` locals are typed as the pool instead of a `Button`.
- A nil check on a variable no longer hides `need-check-nil` on its fields.
- `SetScript`/`HookScript` handlers are now typed on `Button`, `EditBox` and other widget types.
- Completion on Frame subclasses now offers every ancestor's methods.
- More FrameXML global tables and their methods are typed.
- Table-entry `@type` annotations now apply when comment lines come between the annotation and the entry.
- Fixed false `undefined-doc-name` on variadic generics (`...M`).
- `Frame:SetMinResize`/`SetMaxResize` are no longer offered on Classic, which has also removed them.
- `Interface-Classic` and other `Interface-<Type>` TOC lines are now recognized and count toward the addon's flavors.
- Unsaved edits stay visible to other files across workspace rebuilds, and a closed file's discarded edits no longer show up in other files.
- Released binaries now report their real version (they used to report 0.1/0.0.0).

**Improvements**

- Lower memory use.

**Docs**

- Fixed stale and inaccurate text across the docs site, README and marketplace listings.
