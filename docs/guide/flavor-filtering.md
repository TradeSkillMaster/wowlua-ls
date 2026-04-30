# Flavor Filtering

WoW ships three game versions — retail, classic, and classic era — and their APIs differ. If your addon targets more than one, calling an API that doesn't exist in one of your targets is a runtime error you won't catch until someone reports it. wowlua-ls catches it at edit time.

## Setup

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

## The `wrong-flavor-api` diagnostic

With flavors configured, the LS warns when you call an API that isn't available in all your targets:

```lua
-- This function only exists in retail
AbbreviateLargeNumbers(100)
-- warning: wrong-flavor-api — not available in classic
```

Hovering over a WoW API function shows its availability: `Flavors: Retail, Classic`.

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