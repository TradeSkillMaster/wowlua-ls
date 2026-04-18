-- Tests for LSP semantic-token classification (the `tok:` assertion).
-- Each `tok:` expectation lists the token type and any modifiers in any order.

---@class Widget
---@field name string
local Widget = {}

---@param self Widget
---@param label string
function Widget:SetLabel(label)
--                       ^ tok: parameter
    self.name = label
--       ^ tok: property
--                ^ tok: parameter
end

---@deprecated use SetLabel instead
function Widget:SetName(name) end

local w = Widget:SetLabel("hi")
--    ^ tok: variable
--          ^ tok: class
--                 ^ tok: method

-- Deprecated method should carry the deprecated modifier.
Widget:SetName("x")
--     ^ tok: method deprecated

-- Built-in Lua global function used as a value (the motivating case).
local mapper = strupper
--              ^ tok: function defaultLibrary

-- Called the same way — classification must not depend on invocation.
local up = strupper("hello")
--           ^ tok: function defaultLibrary

-- A WoW API namespace table.
local info = C_Item
--            ^ tok: namespace defaultLibrary

-- Field access on a stub namespace → function with defaultLibrary.
C_Item.GetItemInfo(1)
--        ^ tok: function defaultLibrary

-- Local function reference — plain function, no defaultLibrary.
local function helper() end
local h = helper
--          ^ tok: function

-- Plain local variable
local count = 5
local shown = count
--              ^ tok: variable

-- Local function declaration — the name token at the binding site resolves to
-- the function, so it classifies as `function` (no declaration modifier).
local function counter() return 0 end
--             ^ tok: function

-- for-in loop variables bind as local names. `ipairs` here is a stub function.
for i, v in ipairs({1,2}) do
--  ^ tok: variable
--     ^ tok: variable
--          ^ tok: function defaultLibrary
    local x = i + v
--            ^ tok: variable
--                ^ tok: variable
end

-- numeric for-loop variable
for n = 1, 10 do
--  ^ tok: variable
    local y = n
--            ^ tok: variable
end

-- _-prefixed field on a @class is implicitly protected; classification should
-- still be `property`, the protected visibility is orthogonal.
---@class Bag
---@field _items table
local bag = nil ---@type Bag
local items = bag._items
--                  ^ tok: property

-- Anonymous function assigned to a local — the local binds as `function`.
local cb = function(x) return x end
--    ^ tok: function
--                  ^ tok: parameter

-- A local typed as an INSTANCE of a class is a variable, not a class. Only the
-- class binding itself (`local Widget = {} ---@class Widget`, where the symbol
-- name matches the `class_name`) should be classified as `class`.
---@class Operation
---@field id number
local Operation = {}
--    ^ tok: class

local operationSettings = nil ---@type Operation
--    ^ tok: variable
local opId = operationSettings.id
--           ^ tok: variable
--                             ^ tok: property

-- A local re-bound to a class table is still a variable — the binding is not
-- the class itself.
local Aliased = Operation
--    ^ tok: variable
--              ^ tok: class
