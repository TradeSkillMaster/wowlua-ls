---@diagnostic disable: unused-local

-- A file re-declaring the class still sees the nested table the methods were
-- defined on.
---@class DeepMethodRoot
local Root = DeepMethodRoot

local price = Root.API.v1.GetPrice(1)
--    ^ hover: (local) price: number?

---@class DeepMethodCalls
local Calls = DeepMethodCalls

local callExtra = Calls.Fmt:Extra()
--    ^ hover: (local) callExtra: number
