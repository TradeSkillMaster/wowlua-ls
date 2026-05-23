# Stub Generation

wowlua-ls ships with precomputed type stubs for the entire WoW API — around 150,000 symbols covering functions, classes, enums, events, and global variables across retail, classic, and classic era. This page documents the data sources, pipeline, and output artifacts that make this work.

Stubs are regenerated with:

```bash
cargo run -- regenerate-stubs
```

## Data sources

The pipeline integrates six major sources into a single type database.

### 1. Ketho/vscode-wow-api

[Ketho/vscode-wow-api](https://github.com/Ketho/vscode-wow-api) is shallow-cloned with its submodules (which pulls in [NumyAddon/FramexmlAnnotations](https://github.com/nicemods/FramexmlAnnotations) into `Annotations/FrameXML/`).

- **`Annotations/Core/` and `Annotations/FrameXML/`** — Pre-written LuaLS-style annotation `.lua` files containing type stubs for classes, functions, and fields related to frames and widgets. These are scanned directly as Lua. Widget stubs with a `---[Documentation](...)` link but no `@param`/`@return` annotations are enriched by fetching the linked wiki page and injecting parsed type annotations into the vendor files before scanning.
- **`Annotations/Core/Data/Wiki.lua`** — Function names extracted as the source list for wiki-documented global stubs (the file itself is skipped; we generate our own `WikiGlobals.lua` from wiki scraping).

The submodule initialization fetches **NumyAddon/FramexmlAnnotations** into `Annotations/FrameXML/`. **Ketho/BlizzardInterfaceResources** data is accessed via GitHub raw URLs rather than the local clone.

### 2. wago.tools

[wago.tools](https://wago.tools/) provides database exports via HTTP API.

- **`api/builds/{product}/latest`** — Fetched to discover the latest build number for a given WoW product (e.g. `wow` for retail). Returns JSON with a `version` field (e.g. `12.0.5.67602`).
- **`db2/GlobalStrings/csv?build={build}&locale=enUS`** — Full CSV export of the `GlobalStrings` DB2 table for the given build. Columns: `ID`, `BaseTag` (variable name), `TagText_lang` (English string value), `Flags`. Used to generate `GlobalStrings.lua`. CRLF line endings in values are normalised to LF. All valid identifier names not already covered by hand-written stubs are emitted directly.

### 3. Ketho/BlizzardInterfaceResources

[Ketho/BlizzardInterfaceResources](https://github.com/AreWeReadyYet/BlizzardInterfaceResources) is fetched via raw GitHub URLs across three branches: `live`, `classic_era`, `classic`.

- **`Resources/GlobalAPI.lua` and `Resources/FrameXML.lua`** — Simple name lists used to compute the classic-only API diff by identifying names present in classic branches but absent from retail/`live`. Also used as the source of global variable names for `GlobalVariables.lua` and per-API flavor bitmasks (derived from which branches contain each name).
- **`Resources/LuaEnum.lua`** — Parsed for `LE_*` legacy enum constant values; nested `Enum = { Category = { ValueName = N } }` structures are converted from CamelCase to `LE_UPPER_SNAKE` format. Names are cross-referenced against `LE_*` references found in wow-ui-source FrameXML Lua files.

### 4. Gethe/wow-ui-source

[Gethe/wow-ui-source](https://github.com/AreWeReadyYet/wow-ui-source) is shallow-cloned across three branches: `live`, `classic_era`, `classic`.

- **`Interface/AddOns/**/*.xml`** — XML files parsed via regex to extract `<Frame>`, `<Button>` elements with `name=`, `mixin=`, and `inherits=` attributes, producing frame global names with type and mixin associations. Inheritance is resolved transitively with cycle detection.
- **`Interface/AddOns/Blizzard_APIDocumentationGenerated/*.lua`** — Parsed directly for structured function/event/structure data. Each file is a Lua table with `Type = "System"` (game APIs) or `Type = "ScriptObject"` (widget methods, skipped). Functions include namespace, arguments, returns, and `MayReturnNothing`. Events include `LiteralName` and `Payload`. Structures include typed `Fields`. Params with a `Mixin` field use the mixin name (Lua class) instead of the C++ `Type`. Types are normalized minimally: `bool`→`boolean`, `cstring`→`string`, `luaIndex`→`number`; all other type names (e.g. `WOWGUID`, `fileID`, `time_t`) are kept as-is since they have `@alias` definitions in Ketho's `BlizzardType.lua`. Array params (`Type = "table", InnerType = "Foo"`) produce `Foo[]`. Generated stubs only fill gaps — functions/structures already covered by Ketho's richer annotations are skipped via name deduplication. Also parsed for classic-only constants (structured `{Name, Type, Value}` entries) and enumerations (`{Name, EnumValue}` entries).
- **`Interface/AddOns/**/*.lua`** — All FrameXML Lua files scanned for: top-level `UPPER_SNAKE` constant assignments (classic vs retail diff), `LE_*` name references (cross-referenced with BlizzardInterfaceResources `LuaEnum.lua` for values), and field/method assignments on frame globals (`FrameName.field = rhs`, `function FrameName:method(...)`) to infer field types. Also detects `PanelTemplates_SetNumTabs` calls to inject `numTabs`/`selectedTab` fields.

### 5. warcraft.wiki.gg

[warcraft.wiki.gg](https://warcraft.wiki.gg/) provides community-maintained API documentation.

Page names from all three passes are collected upfront and batch-fetched in a single HTTP POST to `Special:Export`, then distributed to each processor:

- **Classic-only APIs** — Wiki pages for APIs in `(classic_era ∪ classic) \ retail` are parsed to extract parameter types, names, and return types, generating typed stubs in `ClassicGlobals.lua`.
- **Wiki-documented globals** — Function names from Ketho's `Wiki.lua` are parsed with `parse_wikitext()`, replacing Ketho's pre-parsed stubs with our own `WikiGlobals.lua`. Functions without a wiki page or whose markup can't be parsed get a bare `function name(...) end` stub with a doc link.
- **Widget method enrichment** — Vendor widget stubs that have a doc link but no annotations are enriched by parsing type annotations via `parse_widget_wiki_annotations()`.

Wiki parsing handles <code v-pre>{{apisig|...}}</code> templates, `== Arguments ==` / `== Returns ==` sections, <code v-pre>{{apitype|type|nilable}}</code> type annotations, embedded `<!-- luals ... -->` blocks, optional parameters `[, param]`, and redirect resolution.

### 6. Local overrides

Hand-written override files in `stubs/overrides/` take precedence over vendor stubs when matched by filename stem. These handle cases that require wowlua-ls-specific annotations not expressible in standard LuaLS (generics, intersections, variadic types, etc.). Currently 24 files:

| File | Purpose |
|------|---------|
| `AceAddon-3.0.lua` | AceAddon library stubs |
| `AceGUI-3.0.lua` | AceGUI library stubs |
| `CreateFrame.lua` | Intersection types: `CreateFrame(..., template) → T & Tp` |
| `GetCursorInfo.lua` | Cursor info return type overloads |
| `HookScript.lua` | Event handler hook typing |
| `LibStub.lua` | Library version management |
| `Mixin.lua` | Variadic generics: `Mixin(T, ...M) → T & ...M` |
| `NamePlateBaseMixin.lua` | Base mixin for name plates |
| `RuntimeMissingGlobals.lua` | Globals used by addons but not in BlizzardInterfaceResources |
| `SetScript.lua` | Contextual callback typing with event-param narrowing |
| `WorldFrame.lua` | Global frame instance |
| `coroutine.lua` | Coroutine yield/resume types |
| `debugstack.lua` | Debug stack trace function |
| `ipairs.lua` | Generic iterator with `K!, V!` (non-nil keys/values) |
| `newproxy.lua` | Userdata proxy creation |
| `next.lua` | Generic next iterator with `K!, V!` |
| `pairs.lua` | Generic iterator with `K!, V!` (non-nil keys/values) |
| `pcall.lua` | Generic success/error tuple returns |
| `pcallwithenv.lua` | Generic pcall variant with environment |
| `plugin_api.lua` | Plugin diagnostic API types |
| `select.lua` | `returns<F>` projection for variadic return truncation |
| `string_match.lua` | Pattern matching return types |
| `table.lua` | Generic overloads for `insert`, `remove`, etc. |
| `unpack.lua` | Variadic unpacking with `...T` syntax |

## Generated stub files

The pipeline produces these intermediate Lua files (held in memory, not written to disk):

| File | Source |
|------|--------|
| `GlobalStrings.lua` | wago.tools `db2/GlobalStrings` (latest retail build, `enUS` locale) |
| `GlobalVariables.lua` | BlizzardInterfaceResources global name lists (names not covered by GlobalStrings) |
| `ClassicGlobals.lua` | BlizzardInterfaceResources diff + wiki scraping + APIDocumentation + `LE_*` constants + XML frame globals |
| `WikiGlobals.lua` | `Wiki.lua` function names + wiki scraping |
| `BlizzardAPI.lua` | Blizzard `APIDocumentationGenerated` functions (deduped against Ketho) |
| `BlizzardStructures.lua` | Blizzard `APIDocumentationGenerated` structure types (deduped against Ketho) |
| `BlizzardEvents.lua` | Blizzard `APIDocumentationGenerated` events |

## Output artifacts

| File | Format | Content |
|------|--------|---------|
| `stubs/precomputed.bin.zst` | Magic (`0x574F575F`) + Version (4B) + zstd-9 payload | Bincode-serialized `PrecomputedStubs` (PreResolvedGlobals + ClassDecl + ExternalGlobal) |
| `stubs/precomputed-files.bin.zst` | Version (4B) + zstd-9 payload | Bincode-serialized `HashMap<String, String>` of stub file contents for go-to-definition |
| `stubs/precomputed-provenance.txt` | Text | Generation timestamp, source repo, commit hash, symbol/function/table/file counts |

The main blob contains the fully resolved type database (~150k symbols, ~45k functions, ~24k tables, ~21k classes, ~103k globals). The files blob contains the source text of all referenced stub files (~2,800 files) so the LSP can support go-to-definition into stub code.

Blob version is currently **24** and is incremented whenever `PreResolvedGlobals`, `ClassDecl`, `ExternalGlobal`, or any serialized type changes shape.

## Loading

The `embedded-stubs` Cargo feature (default on) bakes both blobs into the binary via `include_bytes!`. Without the feature (`--no-default-features`), blobs are loaded at runtime from a `stubs/` directory next to the executable — used for universal editor plugin packages that share one copy of stubs across per-platform binaries.

## Validation

Before writing, the pipeline validates minimum counts (symbols ≥ 50k, functions ≥ 20k, tables ≥ 10k, files ≥ 1k, globals ≥ 50k, classes ≥ 10k) to catch truncated data from network failures or upstream structure changes.
