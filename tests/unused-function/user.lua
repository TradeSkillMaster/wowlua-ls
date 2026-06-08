---@diagnostic disable: unused-local
-- Uses some of the globals defined in defs.lua.

local a = UsedGlobal()
local b = UsedAssignFunc()
local c = NS.UsedMethod()
local d = NS:UsedColonMethod()

---@param widget AlphaWidget|BetaWidget
local function useUnionWidget(widget)
    widget:Process()
end
useUnionWidget(AlphaWidget)
useUnionWidget(BetaWidget)
-- Function-as-value: local variable assignment.
local e = NS.FuncAsValueMethod
-- Function-as-value: passed as a callback argument (the original false-positive pattern).
UsedGlobal(NS.FuncAsArgMethod)
-- Function-as-value: stored in a table constructor.
local t = { handler = NS.FuncInTableMethod }
