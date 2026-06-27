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

The `classname` argument can be either a string literal (e.g. `"ScrollFrame"`) or
a single-name identifier that matches a known `@class` (e.g. `ScrollFrame` — the
class table itself). Dotted names like `MyLib.Dog` are not supported. This matches
idiomatic class-library patterns like `obj:isA(Class)`.

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

## In-place argument narrowing (`@narrows-arg`) {#narrows-arg}

`@narrows-arg N` narrows the Nth argument's type to the function's return type when the call is a bare statement (not assigned to a variable). This is useful for functions that mutate a value in-place, like WoW's `Mixin()`:

```lua
---@generic T, ...M
---@narrows-arg 1
---@param object T
---@param ... any
---@return T & ...M
function Mixin(object, ...) end
```

When you call `Mixin` without capturing the return value, the first argument's type is narrowed:

```lua
---@type Frame
local frame = {}

Mixin(frame, DraggableMixin)

-- frame is now Frame & DraggableMixin
frame:StartDragging() -- no warning
```

The index is 1-based and refers to call-site argument position (not counting `self`). Only bare function call statements trigger the narrowing — assignments like `local x = Mixin(f, M)` use the return type instead.

The narrowed argument can be a **field** as well as a local. A common pattern is creating a frame, storing it on a field, and mixing it in:

```lua
function MyPanelMixin:OnLoad()
  self.Splitter = CreateFrame("Frame", nil, self)
  Mixin(self.Splitter, SplitterMixin)
end

function MyPanelMixin:Refresh()
  self.Splitter:Cancel() -- SplitterMixin's method resolves here, in another method
end
```

The field's type becomes `Frame & SplitterMixin`, so the mixin's methods resolve on every read of `self.Splitter` — including from other methods of the same table. The same works for a field on a plain local table (`obj.x = ...; Mixin(obj.x, M)`).

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

## Field presence narrowing

When a variable is typed as a union of classes, checking a field's truthiness narrows the variable to only the union members where that field is **required** (non-optional). This lets you model mutually exclusive fields without boilerplate:

```lua
---@class ScrollTable.ColInfo.WithTitle
---@field title string
---@field font string

---@class ScrollTable.ColInfo.WithIcon
---@field titleIcon string
---@field font string

---@alias ScrollTable.ColInfo ScrollTable.ColInfo.WithTitle | ScrollTable.ColInfo.WithIcon

---@param col ScrollTable.ColInfo
function setupColumn(col)
    if col.title then
        col -- ScrollTable.ColInfo.WithTitle
        print(col.title) -- string, not string?
    else
        col -- ScrollTable.ColInfo.WithIcon
        print(col.titleIcon) -- string, not string?
    end
end
```

The LS splits the union: members where the checked field is required go to the then-branch; members where it's absent or optional go to the else-branch.

This works everywhere narrowing applies:

```lua
---@param col ScrollTable.ColInfo
function example(col)
    -- Early exit
    if not col.title then return end
    col -- WithTitle for the rest of the function

    -- Nil comparison
    if col.title ~= nil then ... end

    -- Assert
    assert(col.title)
    col -- WithTitle
end
```

**Rules:**
- A field is "required" if it exists on the class and is non-optional (no `?` suffix)
- A field is "absent or optional" if the class doesn't define it, or defines it with `?`
- If all union members have the field as required, no narrowing occurs (can't discriminate)
- Works with unions of 2+ members — narrows to whichever subset has the field