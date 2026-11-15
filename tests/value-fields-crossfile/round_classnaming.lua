---@diagnostic disable: create-global, missing-return
-- A local is the class its call names only when the callee returns the class
-- itself: a call returning a list of it (`T[]`) makes no instance, so methods
-- defined on the local stay off the class (round_classnaming_user.lua).
---@class RoundWidget
RoundWidget = {}
---@class RoundGadget
RoundGadget = {}

---@generic T
---@param kind `T`
---@return T[]
function RoundGetAll(kind) end

---@generic T
---@param kind `T`
---@return T
function RoundGetOne(kind) end

local all = RoundGetAll("RoundWidget")
function all:Refresh() end

local one = RoundGetOne("RoundGadget")
function one:Spin() end
