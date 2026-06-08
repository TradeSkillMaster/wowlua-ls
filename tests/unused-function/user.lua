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
