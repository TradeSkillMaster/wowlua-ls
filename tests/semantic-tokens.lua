-- Tests for LSP semantic-token classification (the `tok:` assertion).
--
-- The feature is intentionally narrow: it only emits a `function` token for
-- bare Names that resolve to a function symbol, so that e.g. a global function
-- referenced as a value still renders in the function color. Everything else
-- (parameters, local variables, fields, method/dot access, class bindings) is
-- left to the editor's built-in Lua grammar and asserts `tok: none`.

---@class Widget
---@field name string
local Widget = {}

---@param self Widget
---@param label string
function Widget:SetLabel(label)
--                       ^ tok: none
    self.name = label
--       ^ tok: none
--                ^ tok: none
end

---@deprecated use SetLabel instead
function Widget:SetName(name) end

local w = Widget:SetLabel("hi")
--    ^ tok: none
--          ^ tok: none
--                 ^ tok: none

-- Deprecated method call — not emitted (grammar handles method coloring).
Widget:SetName("x")
--     ^ tok: none

-- Built-in Lua global function used as a value (the motivating case).
local mapper = strupper
--              ^ tok: function defaultLibrary

-- Called the same way — classification must not depend on invocation.
local up = strupper("hello")
--           ^ tok: function defaultLibrary

-- A WoW API namespace table — grammar colors it as a plain variable.
local info = C_Item
--            ^ tok: none

-- Field access on a stub namespace — grammar handles dot access coloring.
C_Item.GetItemInfo(1)
--        ^ tok: none

-- Local function reference — plain function, no defaultLibrary.
local function helper() end
local h = helper
--          ^ tok: function

-- Plain local variable
local count = 5
local shown = count
--              ^ tok: none

-- Local function declaration — the name token at the binding site resolves to
-- the function, so it classifies as `function` (no declaration modifier).
local function counter() return 0 end
--             ^ tok: function

-- for-in loop variables bind as local names. `ipairs` here is a stub function.
for i, v in ipairs({1,2}) do
--  ^ tok: none
--     ^ tok: none
--          ^ tok: function defaultLibrary
    local x = i + v
--            ^ tok: none
--                ^ tok: none
end

-- numeric for-loop variable
for n = 1, 10 do
--  ^ tok: none
    local y = n
--            ^ tok: none
end

-- _-prefixed field on a @class is implicitly protected — grammar colors the
-- dot-access as a plain property either way.
---@class Bag
---@field _items table
local bag = nil ---@type Bag
local items = bag._items
--                  ^ tok: none

-- Anonymous function assigned to a local — the local binds as `function`.
local cb = function(x) return x end
--    ^ tok: function
--                  ^ tok: none

-- A local typed as an INSTANCE of a class — grammar colors as variable.
---@class Operation
---@field id number
local Operation = {}
--    ^ tok: none

local operationSettings = nil ---@type Operation
--    ^ tok: none
local opId = operationSettings.id
--           ^ tok: none
--                             ^ tok: none

-- A local re-bound to a class table is still a variable, and grammar handles
-- the class reference on the RHS.
local Aliased = Operation
--    ^ tok: none
--              ^ tok: none

-- A local that shadows a stub global. The reference on the next line must
-- resolve to the local (a string), not the stub function — so no token.
do
    local strupper = "x"
    local shadowed = strupper
--                     ^ tok: none
    return shadowed
end

-- Method definition with field access — the class table, method name,
-- parameters, and field-access bases must NOT receive semantic tokens.
-- (Grammar fix for @return description coloring validated manually;
-- this ensures semantic tokens don't interfere.)
---@class TreeNode
---@field _firstChild table
---@field childrenTemp table
local TreeNode = {}

local pvt = { childrenTemp = {} }

---@return ...number @The children
function TreeNode:GetChildren(node)
--       ^ tok: none
--                ^ tok: none
--                            ^ tok: none
    assert(not next(pvt.childrenTemp))
--    ^ tok: function defaultLibrary
--             ^ tok: function defaultLibrary
--                  ^ tok: none
--                      ^ tok: none
    local child = self._firstChild[node]
--        ^ tok: none
--                    ^ tok: none
--                                 ^ tok: none
    while child do
--        ^ tok: none
    end
end
