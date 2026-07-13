# Metatable Inference

WoW addon OOP is built on metatables. wowlua-ls understands `setmetatable`, `__index` chains, `__call`, operator metamethods, and `getmetatable` - often without any annotations at all.

## `setmetatable` + `__index`

The most common WoW addon pattern:

```lua
local MyClass = {}
MyClass.__index = MyClass

function MyClass:GetName()
    return self.name
end

local obj = setmetatable({ name = "test" }, MyClass)
obj:GetName() -- resolves to string via __index
obj.name      -- string (direct field, takes priority)
```

The LS traces the metatable chain: `obj` → metatable `MyClass` → `__index` is `MyClass` → `GetName` is found.

### Self-referential metatables

The `mt.__index = mt` pattern is the most common in WoW addons, and it's fully supported:

```lua
local M = {}
M.__index = M

function M.new()
    return setmetatable({}, M)
end

function M:method()
    return "hello"
end

local inst = M.new()
inst:method() -- resolves through __index = M
```

### Chained metatables

Instance → Child → Base chains work:

```lua
local Base = {}
Base.__index = Base
function Base:baseMethod() return "base" end

local Child = setmetatable({}, Base)
Child.__index = Child
function Child:childMethod() return "child" end

local inst = setmetatable({}, Child)
inst:childMethod() -- found on Child
inst:baseMethod()  -- found on Base (via chain)
```

### Factory functions

A factory that returns `setmetatable({}, self)` propagates the class type:

```lua
local Widget = {}
Widget.__index = Widget

---@return Widget
function Widget:New()
    return setmetatable({}, self)
end

function Widget:Show()
    print("showing")
end

local w = Widget:New()
w:Show() -- resolved
```

### Statement form

`setmetatable` doesn't need to be in an assignment - it mutates the table in place:

```lua
local t = {}
setmetatable(t, { __index = SomeClass })
t:method() -- resolves through __index
```

### Instance field priority

Direct fields on the table always win over `__index` fields:

```lua
local M = {}
M.__index = M
M.x = 10

local obj = setmetatable({ x = 20 }, M)
obj.x -- 20 (direct field), not 10 (from __index)
```

## `__call` metamethod

Tables with `__call` become callable:

```lua
local Counter = setmetatable({ n = 0 }, {
    __call = function(self)
        self.n = self.n + 1
        return self.n
    end
})

local val = Counter() -- val: number
```

## Operator metamethods

Arithmetic operators check for metamethods and use their return types:

```lua
---@class Vec2
---@field x number
---@field y number
local Vec2 = {}
Vec2.__index = Vec2

---@param a Vec2
---@param b Vec2
---@return Vec2
Vec2.__add = function(a, b)
    return setmetatable({ x = a.x + b.x, y = a.y + b.y }, Vec2)
end

---@type Vec2
local a, b
local c = a + b -- c: Vec2
c.x             -- number
```

Supported metamethods: `__add` (+), `__sub` (-), `__mul` (*), `__div` (/), `__mod` (%), `__pow` (^), `__concat` (..), `__unm` (unary -), `__len` (#).

The LS checks the table's metatable first, then the table itself (for `@class` tables with metamethods as direct fields).

## `getmetatable()`

Returns the raw metatable:

```lua
local mt = { __index = { z = 3 } }
local obj = setmetatable({}, mt)
local m = getmetatable(obj) -- m has mt's type
```

## Class name propagation

When a metatable's `__index` points to a `@class` table, the class name propagates to instances. This is how `setmetatable({}, MyClass)` creates instances of `MyClass` even without a `@return MyClass` annotation:

```lua
---@class Tooltip
local Tooltip = {}
Tooltip.__index = Tooltip

function Tooltip:Show() end

local inst = setmetatable({}, Tooltip)
-- inst's type is Tooltip (class name propagated from __index)
inst:Show() -- resolved
```

The LS tries three sources for the class name (in priority order):
1. `__index` as a direct table with `class_name`
2. The metatable itself having `class_name`
3. `__index` as a function that returns access to a class-typed table

## Cross-file support

Metatable inference works across files when the `__index` target is a `@class` table (since those are globally registered). For unannotated metatables in other files, add a `@class` annotation to get cross-file resolution.

## When to annotate vs. rely on inference

The LS resolves a lot from metatables alone, but there are cases where annotations help:

| Situation | Recommendation |
|---|---|
| Simple `mt.__index = mt` | Inference handles it |
| Factory returning `setmetatable({}, self)` | Add `@return ClassName` for clarity |
| Complex multi-file class | Use `@class` + `@field` |
| Dynamic `__index` function | Inference may not follow - use `@class` |
| `__index` from another file | Needs `@class` on the target for cross-file |