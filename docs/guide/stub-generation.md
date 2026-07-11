# Stub Generation

wowlua-ls ships with precomputed type stubs for the entire WoW API — around 150,000 symbols covering functions, classes, enums, events, and global variables across retail, classic, and classic era. This page documents the data sources, pipeline, and output artifacts that make this work.

Stubs are regenerated with:

```bash
cargo run -- regenerate-stubs
```

The git repositories below (Ketho/vscode-wow-api and Gethe/wow-ui-source) are shallow-cloned into a persistent cache under the platform cache directory (`$XDG_CACHE_HOME`/`~/.cache/wowlua-ls/clones/` on Linux/macOS, `%LOCALAPPDATA%\wowlua-ls\clones\` on Windows). On subsequent runs the cached clones are updated in place with `git fetch --depth 1` + `git reset --hard` rather than re-cloned from scratch, saving most of the clone time. Set the `WOWLUA_LS_REFRESH_CLONES` environment variable to force fresh clones.

## Data sources

The pipeline integrates six major sources into a single type database.

### 1. Ketho/vscode-wow-api

[Ketho/vscode-wow-api](https://github.com/Ketho/vscode-wow-api) is shallow-cloned with its submodules (which pulls in [NumyAddon/FramexmlAnnotations](https://github.com/nicemods/FramexmlAnnotations) into `Annotations/FrameXML/`).

- **`Annotations/Core/` and `Annotations/FrameXML/`** — Pre-written LuaLS-style annotation `.lua` files containing type stubs for classes, functions, and fields related to frames and widgets. These are scanned directly as Lua. Widget stubs with a `---[Documentation](...)` link but no `@param`/`@return` annotations are enriched by fetching the linked wiki page and injecting parsed type annotations into the vendor files before scanning.

The submodule initialization fetches **NumyAddon/FramexmlAnnotations** into `Annotations/FrameXML/`. **Ketho/BlizzardInterfaceResources** data is accessed via GitHub raw URLs rather than the local clone.

### 2. wago.tools

[wago.tools](https://wago.tools/) provides database exports via HTTP API.

- **`api/builds/{product}/latest`** — Fetched to discover the latest build number for a given WoW product (e.g. `wow` for retail). Returns JSON with a `version` field (e.g. `12.0.5.67602`).
- **`db2/GlobalStrings/csv?build={build}&locale=enUS`** — Full CSV export of the `GlobalStrings` DB2 table for the given build. Columns: `ID`, `BaseTag` (variable name), `TagText_lang` (English string value), `Flags`. Used to generate `GlobalStrings.lua`. CRLF line endings in values are normalised to LF. All valid identifier names not already covered by hand-written stubs are emitted directly.
- **`db2/GlobalColor/csv`** — Full CSV export of the `GlobalColor` DB2 table (no build filter — uses latest available data for maximum coverage). Columns: `ID`, `LuaConstantName`, `Color` (signed int32, packed ARGB). Used to generate `GlobalColors.lua`. For each entry, two globals are emitted: the color object itself (typed `colorRGBA`) and a `_CODE` string variant (e.g. `GREEN_FONT_COLOR` and `GREEN_FONT_COLOR_CODE`). The `_CODE` variants are color escape strings generated at runtime by WoW's FrameXML via `GenerateHexColorMarkup()`.

### 3. Ketho/BlizzardInterfaceResources

[Ketho/BlizzardInterfaceResources](https://github.com/AreWeReadyYet/BlizzardInterfaceResources) is fetched via raw GitHub URLs across three branches: `live`, `classic_era`, `classic`.

- **`Resources/GlobalAPI.lua` and `Resources/FrameXML.lua`** — Simple name lists used to compute the classic-only API diff by identifying names present in classic branches but absent from retail/`live`. Also used as the source of global variable names for `GlobalVariables.lua` and per-API flavor bitmasks (derived from which branches contain each name).
- **`Resources/LuaEnum.lua`** — Parsed for `LE_*` legacy enum constant values (nested `Enum = { Category = { ValueName = N } }` structures converted from CamelCase to `LE_UPPER_SNAKE` format, cross-referenced against wow-ui-source FrameXML `LE_*` references); `Enum.*` enum category stubs (merged with APIDocumentation enumerations); and the `Constants` global table (sub-tables with typed number/boolean/string fields).
- **`Resources/CVars.lua`** — Parsed for `["cvarName"] = { ... }` entries to generate the `CVar` string literal union type alias.

### 4. Gethe/wow-ui-source

[Gethe/wow-ui-source](https://github.com/AreWeReadyYet/wow-ui-source) is shallow-cloned across three branches: `live`, `classic_era`, `classic`.

- **`Interface/AddOns/**/*.xml`** — XML files parsed via regex to extract `<Frame>`, `<Button>` elements with `name=`, `mixin=`, and `inherits=` attributes, producing frame global names with type and mixin associations. Inheritance is resolved transitively with cycle detection.
- **`Interface/AddOns/Blizzard_APIDocumentationGenerated/*.lua`** — Parsed directly for structured function/event/structure data. Each file is a Lua table with `Type = "System"` (game APIs) or `Type = "ScriptObject"` (widget methods, skipped). Functions include namespace, arguments, returns, and `MayReturnNothing`. Events include `LiteralName` and `Payload`. Structures include typed `Fields`. Params with a `Mixin` field use the mixin name (Lua class) instead of the C++ `Type`. Types are normalized minimally: `bool`→`boolean`, `cstring`→`string`, `luaIndex`→`number`; all other type names (e.g. `WOWGUID`, `fileID`, `time_t`) are kept as-is since they have `@alias` definitions in Ketho's `BlizzardType.lua`. Array params (`Type = "table", InnerType = "Foo"`) produce `Foo[]`. Generated stubs only fill gaps — functions/structures already covered by Ketho's richer annotations are skipped via name deduplication. Also parsed for classic-only constants (structured `{Name, Type, Value}` entries) and enumerations (`{Name, EnumValue}` entries).
- **`Interface/AddOns/**/*.lua`** — All FrameXML Lua files scanned for: top-level `UPPER_SNAKE` constant assignments (classic vs retail diff), `LE_*` name references (cross-referenced with BlizzardInterfaceResources `LuaEnum.lua` for values), and field/method assignments on frame globals (`FrameName.field = rhs`, `function FrameName:method(...)`) to infer field types. Also detects `PanelTemplates_SetNumTabs` calls to inject `numTabs`/`selectedTab` fields.

### 5. warcraft.wiki.gg

[warcraft.wiki.gg](https://warcraft.wiki.gg/) provides community-maintained API documentation.

Function names are discovered by querying the MediaWiki API for the `API_functions`, `API_functions/Removed`, `API_functions/deprecated`, and `API_functions/Noflavor` categories. Names that duplicate a Blizzard API namespace function (e.g. `GetAddOnMetadata` shadowed by `C_AddOns.GetAddOnMetadata`) are filtered unless they still exist as bare globals in BlizzardInterfaceResources' `GlobalAPI.lua`.

Page names from all three passes are collected upfront and batch-fetched in a single HTTP POST to `Special:Export` (the endpoint is behind Cloudflare, which rejects concurrent requests, so this is intentionally one request), then distributed to each processor:

The raw `Special:Export` XML dump is the single most expensive step (~25–40s). It is persistently cached under the platform cache directory (`$XDG_CACHE_HOME`/`~/.cache/wowlua-ls/` on Linux/macOS, `%LOCALAPPDATA%\wowlua-ls\` on Windows) with a 24-hour TTL. The cache is keyed by a hash of the requested page-name set — not by parsing logic — so iterating on `parse_wikitext()` or stub formatting reuses the cached dump and only a change to *which* pages are requested forces a re-fetch. Set the `WOWLUA_LS_REFRESH_WIKI` environment variable to force a fresh fetch regardless of cache age.


- **Classic-only APIs** — Wiki pages for APIs in `(classic_era ∪ classic) \ retail` are parsed to extract parameter types, names, and return types, generating typed stubs in `ClassicGlobals.lua`.
- **Wiki-documented globals** — Function names from the wiki category query are parsed with `parse_wikitext()` to generate `WikiGlobals.lua`. Functions without a wiki page or whose markup can't be parsed get a bare `function name(...) end` stub with a doc link.
- **Widget method enrichment** — Vendor widget stubs that have a doc link but no annotations are enriched by parsing type annotations via `parse_widget_wiki_annotations()`.

Wiki parsing handles <code v-pre>{{apisig|...}}</code> templates, `== Arguments ==` / `== Returns ==` sections, <code v-pre>{{apitype|type|nilable}}</code> type annotations, embedded `<!-- luals ... -->` blocks, optional parameters `[, param]`, and redirect resolution.

### 6. Local overrides

Hand-written override files in `stubs/overrides/` take precedence over vendor stubs when matched by filename stem. These handle cases that require wowlua-ls-specific annotations not expressible in standard LuaLS (generics, intersections, variadic types, etc.). The full set (42 files, alphabetical — keep in sync with `ls stubs/overrides/*.lua`):

| File | Purpose |
|------|---------|
| `AceAddon-3.0.lua` | AceAddon library stubs; the library class inherits the embeddable `AceAddon` prototype so `---@class Foo : AceAddon-3.0` resolves `NewModule`/`GetModule`/… |
| `AceEvent-3.0.lua` | AceEvent library stubs; `RegisterEvent`/`RegisterMessage` type the handler as `keyof self` so the handler string navigates to (and is checked against) the method on `self` |
| `AceGUI-3.0.lua` | AceGUI library stubs |
| `AceLocale-3.0.lua` | AceLocale library stubs; default-locale `NewLocale(app, locale, true)` returns a non-nil table so the `L[key] = true` idiom doesn't trip `need-check-nil` |
| `BattlePetTooltip.lua` | Runtime-injected `AddLine` method that Ketho's vendor annotation omits |
| `CallbackRegistryMixin.lua` | `@generates-events` on `GenerateCallbackEvents` so the synthesized `.Event` enum table resolves |
| `ClassicLegacyEnums.lua` | `LE_*` constants used by addons but not referenced by FrameXML (so not auto-discovered) |
| `coroutine.lua` | Coroutine yield/resume types |
| `CreateFont.lua` | `@creates-global` for `CreateFont`/`CreateFontFamily` named-font side effect |
| `CreateFrame.lua` | Intersection types (`CreateFrame(..., template) → T & Tp`) and `@creates-global` for the named-frame side effect |
| `debugstack.lua` | Debug stack trace function |
| `EquipmentManager.lua` | Full-arity `@return` for `EquipmentManager_UnpackLocation` (deprecated on retail with no `@return`, still live on Classic) so destructuring its result doesn't false-positive `unbalanced-assignments` |
| `EventRegistry.lua` | `@class EventRegistry : CallbackRegistryMixin` with `FrameEvent`-typed callback params |
| `GameTooltip.lua` | `GameTooltip` frame class + script-handler (`GetScript`/`SetScript`) typing |
| `GetCursorInfo.lua` | Cursor info return type overloads |
| `GetObjectType.lua` | `@returns-class-name` on `FrameScriptObject:GetObjectType` for equality-comparison receiver narrowing (`region:GetObjectType() == "FontString"`) |
| `HookScript.lua` | Event handler hook typing |
| `hooksecurefunc.lua` | Backtick-generic `@overload` for the table-less `hooksecurefunc(name, hook)` form |
| `ipairs.lua` | Generic iterator with `K!, V!` (non-nil keys/values) |
| `IsObjectType.lua` | `@type-narrows` for `IsObjectType()` → frame subclass narrowing |
| `LibDBIcon-1.0.lua` | LibDBIcon stubs with the `db` options table fields optional (no false `missing-fields` on `Register`) |
| `LibSharedMedia-3.0.lua` | Full replacement of the vendored LibSharedMedia annotations: `mediatype` is `string` (not a closed enum) so custom media types registered via `:Register` type-check |
| `LibStub.lua` | Library version management |
| `loadstring.lua` | `loadstring` return tuple (compiled chunk, or nil + error message) |
| `MixinFunctions.lua` | Variadic generics for `Mixin`/`CreateFromMixins`/`CreateAndInitFromMixin`: `(T, ...M) → T & ...M` |
| `NamePlateBaseMixin.lua` | Base mixin for name plates |
| `newproxy.lua` | Userdata proxy creation |
| `next.lua` | Generic next iterator with `K!, V!` |
| `pairs.lua` | Generic iterator with `K!, V!` (non-nil keys/values) |
| `pcall.lua` | Generic success/error tuple returns |
| `pcallwithenv.lua` | Generic pcall variant with environment |
| `PlaySound.lua` | String channel override for `uiSoundSubType` (Blizzard docs say enum, Lua takes strings) |
| `plugin_api.lua` | Plugin diagnostic API types |
| `Pools.lua` | Generic `ObjectPool<T>`/`FramePool`/`FramePoolCollection` types (FrameXML-defined, no upstream source) |
| `RuntimeMissingGlobals.lua` | Globals used by addons but not in BlizzardInterfaceResources |
| `select.lua` | `returns<F>` projection for variadic return truncation |
| `SetScript.lua` | Contextual callback typing with event-param narrowing |
| `string_match.lua` | Pattern matching return types |
| `table.lua` | Generic overloads for `insert`, `remove`, etc. |
| `tonumber.lua` | `tonumber` base-argument overload + `@nodiscard` |
| `unpack.lua` | Variadic unpacking with `...T` syntax |
| `WorldFrame.lua` | Global frame instance |

## Generated stub files

The pipeline produces these intermediate Lua files (written to a temp directory for scanning, not persisted):

| File | Source |
|------|--------|
| `GlobalStrings.lua` | wago.tools `db2/GlobalStrings` (latest retail build, `enUS` locale) |
| `GlobalColors.lua` | wago.tools `db2/GlobalColor` (~360 `colorRGBA` objects + `_CODE` string variants) |
| `GlobalVariables.lua` | BlizzardInterfaceResources global name lists (names not covered by GlobalStrings or GlobalColors) |
| `ClassicGlobals.lua` | BlizzardInterfaceResources diff + wiki scraping + APIDocumentation + `LE_*` constants + XML frame globals |
| `WikiGlobals.lua` | Wiki category query function names + wiki scraping |
| `BlizzardAPI.lua` | Blizzard `APIDocumentationGenerated` functions (deduped against Ketho) |
| `BlizzardStructures.lua` | Blizzard `APIDocumentationGenerated` structure types (deduped against Ketho) |
| `BlizzardEvents.lua` | Blizzard `APIDocumentationGenerated` events |
| `Enum.lua` | APIDocumentation enumerations merged with LuaEnum.lua categories |
| `CVar.lua` | BlizzardInterfaceResources `CVars.lua` string literal union |
| `Constants.lua` | LuaEnum.lua `Constants` table (57 sub-tables with typed fields) |

## Output artifacts

| File | Format | Content |
|------|--------|---------|
| `stubs/precomputed.bin.zst` | Magic (`0x574F575F`) + Version (4B) + zstd-9 payload | Bincode-serialized `PrecomputedStubs` (PreResolvedGlobals + ClassDecl + ExternalGlobal) |
| `stubs/precomputed-files.bin.zst` | Version (4B) + zstd-9 payload | Bincode-serialized `HashMap<String, String>` of stub file contents for go-to-definition |
| `stubs/precomputed-provenance.txt` | Text | Generation timestamp, source repo, commit hash, symbol/function/table/file counts |

The main blob contains the fully resolved type database (~150k symbols, ~45k functions, ~24k tables, ~21k classes, ~103k globals). The files blob contains the source text of all referenced stub files (~2,800 files) so the LSP can support go-to-definition into stub code.

The blob version is incremented whenever `PreResolvedGlobals`, `ClassDecl`, `ExternalGlobal`, or any serialized type changes shape.

## Loading

The `embedded-stubs` Cargo feature (default on) bakes both blobs into the binary via `include_bytes!`. Without the feature (`--no-default-features`), blobs are loaded at runtime from a `stubs/` directory next to the executable — used for universal editor plugin packages that share one copy of stubs across per-platform binaries.

## Validation

Before writing, the pipeline validates minimum counts (symbols ≥ 50k, functions ≥ 20k, tables ≥ 10k, files ≥ 1k, globals ≥ 50k, classes ≥ 10k) to catch truncated data from network failures or upstream structure changes.

The `dump-stubs` CLI subcommand outputs every global name and its resolved type as a tab-separated list, sorted alphabetically. This is useful for diffing before and after stub regeneration to catch regressions:

```bash
# Save baseline before changes
wowlua_ls dump-stubs > before.txt

# After regenerating stubs, diff
wowlua_ls dump-stubs > after.txt
diff before.txt after.txt
```
