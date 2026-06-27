# Flavor Filtering

WoW ships three game versions — retail, classic, and classic era — and their APIs differ. If your addon targets more than one, calling an API that doesn't exist in one of your targets is a runtime error you won't catch until someone reports it. wowlua-ls catches it at edit time.

## Setup

There are two ways to configure flavor filtering: manually via `.wowluarc.json`, or automatically from your `.toc` files. Both can be used together — when they are, the effective flavor for each file is the **intersection** of the two.

### Option 1: `.wowluarc.json` (project-wide)

Declare your target flavors in `.wowluarc.json`:

```json
{
  "flavors": ["retail", "classic"]
}
```

Accepted values:

| Name | Meaning |
|---|---|
| `retail` (alias: `mainline`) | The live retail game |
| `classic` | Rolling Classic progression (including MoP Classic) |
| `classic_era` | Classic Era (vanilla) |

### Option 2: TOC-based detection (per-file)

If your addon uses [flavor-specific TOC files](https://warcraft.wiki.gg/wiki/TOC_format), the LS reads the file listings from each TOC and automatically determines which flavors each `.lua` file is loaded for. No `.wowluarc.json` configuration is needed.

**Filename suffixes:**

| TOC suffix | Flavors |
|---|---|
| `_Mainline`, `_Standard` | Retail |
| `_Classic` | Classic + Classic Era |
| `_Vanilla` | Classic Era |
| `_Cata`, `_Wrath`, `_TBC`, `_Mists` | Classic |

The unsuffixed (base) TOC covers whichever flavors aren't claimed by any suffixed TOC in the same addon.

```
MyAddon/
├── MyAddon.toc           # Loaded on Classic + Classic Era (Mainline has its own)
├── MyAddon_Mainline.toc  # Loaded on Retail only
└── MyAddon_Vanilla.toc   # Loaded on Classic Era only
```

A file listed in `MyAddon_Mainline.toc` is treated as retail-only. A file listed in both `_Mainline.toc` and `_Vanilla.toc` is available on retail and classic era. A file in the base `MyAddon.toc` covers whichever flavors don't have a suffixed TOC.

**`AllowLoadGameType` restrictions:**

The LS also respects `## AllowLoadGameType:` headers and per-line `[AllowLoadGameType]` directives:

```toc
## AllowLoadGameType: vanilla
Core.lua
```

```toc
[AllowLoadGameType mainline] RetailUI.lua
SharedCode.lua
```

These intersect with the TOC's suffix flavor, further restricting which flavors a file is loaded for.

**Path variables (`[Family]` and `[Game]`):**

TOC files can use `[Family]` and `[Game]` variables in file paths to load different files per flavor:

```toc
Compat/[Game]/Init.lua
```

The LS expands each variable to all possible values and checks which files exist on disk:

| Variable | Values |
|---|---|
| `[Family]` | `Mainline` (retail), `Classic` (classic + classic era) |
| `[Game]` | `Standard` (retail), `Vanilla` (classic era), `Cata`/`Wrath`/`TBC`/`Mists` (classic) |

Each expanded file gets the flavor mask of its expansion value. Files that don't exist on disk are skipped.

**Intersection with `.wowluarc.json`:**

When both sources provide flavor information, the effective flavor for a file is the intersection. For example, if `.wowluarc.json` declares `["classic_era"]` and a file is listed in `_Classic.toc` (classic + classic_era), the file's effective flavor is classic_era.

## The `wrong-flavor-api` diagnostic

With flavors configured, the LS warns when you call an API that isn't available in all your targets:

```lua
-- This function only exists in retail
AbbreviateLargeNumbers(100)
-- warning: wrong-flavor-api — not available in classic
```

Hovering over a WoW API function shows its availability: `Flavors: Retail, Classic`.

## Flavor-aware deprecation

WoW API deprecations are retail-side: many functions Blizzard marks `@deprecated`
(e.g. `GetMerchantItemInfo`, replaced on retail by `C_MerchantFrame.GetItemInfo`)
remain the live, correct API on Classic and Classic Era. The `deprecated`
diagnostic accounts for this — it won't flag a call when the API is still live in
a flavor your addon targets (and the editor won't strike the call through
either — the semantic-token "deprecated" styling follows the same rule):

```lua
-- In a Classic Era addon (or a multi-flavor addon that includes Classic):
local _, _, price = GetMerchantItemInfo(index)  -- no `deprecated` warning here

-- In a retail-only addon, the same call IS flagged:
-- warning: deprecated — 'GetMerchantItemInfo' is deprecated
```

The addon's targeted flavors are taken from your `.wowluarc.json` `flavors`, else
a flavor-specific `.toc` (suffix / `AllowLoadGameType`), else the `.toc`
`## Interface:` version line — so a config-less multi-version addon
(`## Interface: 120005, 50503, 11508`) is still recognized as targeting Classic.
If there is no flavor signal at all, the warning fires as before. Unlike
`wrong-flavor-api`, this never requires a `flavors` config and the `## Interface:`
fallback applies only to `deprecated`.

## Conditional narrowing

The LS understands flavor-conditional code:

### `WOW_PROJECT_ID` guards

```lua
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    -- Narrowed to retail only
    AbbreviateLargeNumbers(100) -- no warning
else
    -- Narrowed to non-retail flavors
end
```

### Custom flavor guards with `@flavor-narrows`

Mark your own guard functions or boolean variables:

```lua
---@flavor-narrows retail
---@return boolean
local function IsRetail()
    return WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
end

if IsRetail() then
    AbbreviateLargeNumbers(100) -- no warning (narrowed to retail)
end

if not IsRetail() then return end
-- Rest of file is retail-only
AbbreviateLargeNumbers(100) -- no warning
```

#### Boolean flavor guards

`@flavor-narrows` also works on boolean variables and fields, avoiding the overhead of a function call:

```lua
---@type boolean
---@flavor-narrows retail
local isRetail = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

if isRetail then
    AbbreviateLargeNumbers(100) -- no warning
end
```

This is especially useful with the addon namespace pattern, where the boolean is set in one file and used across others:

```lua
-- In init.lua:
local _, ns = ...
---@type boolean
---@flavor-narrows retail
ns.isRetail = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

-- In another file:
local _, ns = ...
if ns.isRetail then
    AbbreviateLargeNumbers(100) -- no warning
end
```

`@flavor-narrows` works with all narrowing patterns: if/else, early exit, `not`.

## When to use it

Flavor filtering is most valuable for addons that ship a single codebase across retail and classic. Without it, you won't know about missing APIs until a classic player reports a Lua error.

If your addon only targets one flavor, you don't need to configure `flavors` — the diagnostic is disabled when no flavors are declared.