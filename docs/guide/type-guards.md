# Custom Type Guards

wowlua-ls narrows types automatically for built-in patterns like `if x then`, `type(x) == "string"`, and `assert()`. But WoW addons often have their own type-checking functions. The `@type-narrows` annotation lets you teach the LS about them.

## Index-based form

`@type-narrows <target_param> <classname_param>` — both are 1-based call-site argument positions:

```lua
---@param element UIElement
---@param typeName string
---@type-narrows 1 2
---@return boolean
function UI.IsType(element, typeName) end
```

When used in a condition, the LS narrows the first argument to the class named by the second:

```lua
---@param parent UIElement
function example(parent)
    if UI.IsType(parent, "ScrollFrame") then
        parent._scrollbar -- parent is now ScrollFrame
    end

    -- Works with early exit too
    if not UI.IsType(parent, "ScrollFrame") then return end
    parent._scrollbar -- narrowed for the rest of the function
end
```

Use `0` for the receiver (`self`) in colon method calls:

```lua
---@param typeName string
---@type-narrows 0 1
---@return boolean
function UIElement:IsType(typeName) end

if element:IsType("ScrollFrame") then
    element._scrollbar -- narrowed
end
```

## Method-style form

`@type-narrows ClassName` — narrows `self` to a fixed class. Useful for boolean predicate methods:

```lua
---@class AuctionRow
---@class AuctionSubRow : AuctionRow
---@field parentId number

---@type-narrows AuctionSubRow
---@return boolean
function AuctionRow:IsSubRow() return false end
```

```lua
---@param row AuctionRow
function example(row)
    if row:IsSubRow() then
        row.parentId -- row is AuctionSubRow
    end

    assert(row:IsSubRow())
    row.parentId -- also works with assert
end
```

## Where narrowing applies

Custom type guards work in all the same places as built-in narrowing:

- `if guard() then ... end` (then branch)
- `if guard() then ... else ... end` (both branches)
- `if not guard() then return end` (early exit)
- `assert(guard())` (rest of function)
- `guard() and expr` / `guard() or expr` (short-circuit)

## Literal boolean discrimination

A related feature that doesn't need `@type-narrows`: when union member types have methods returning literal `true` or `false`, the LS discriminates automatically:

```lua
---@class BaseRow
---@return false
function BaseRow:IsSubRow() return false end

---@class SubRow
---@return true
function SubRow:IsSubRow() return true end

---@param row BaseRow | SubRow
function handle(row)
    if row:IsSubRow() then
        row -- SubRow (returns true)
    else
        row -- BaseRow (returns false)
    end
end
```

No `@type-narrows` needed — the literal boolean return types are enough. Requirements:
- All union members must define the method
- Every return must be literal `true` or literal `false` (not generic `boolean`)
- At least one of each