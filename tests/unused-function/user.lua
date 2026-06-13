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
-- Stub|workspace union receiver: GameTooltip (stub) listed first wins the call
-- resolution, so CustomTip:AddDoubleLine is only reachable via the union member.
---@param tip GameTooltip|CustomTip
local function useTip(tip)
    tip:AddDoubleLine("a", "b")
end
useTip(CustomTip)
-- Function-as-value: local variable assignment.
local e = NS.FuncAsValueMethod
-- Function-as-value: passed as a callback argument (the original false-positive pattern).
UsedGlobal(NS.FuncAsArgMethod)
-- Function-as-value: stored in a table constructor.
local t = { handler = NS.FuncInTableMethod }
-- Method called on a function return value (factory pattern).
local w = CreateWorker()
w:Run()
-- Method called on a narrowed return value from a local function.
-- Covers the pattern: factory returns nil|Class, caller guards then calls.
local store = {
    cache = {}, ---@type table<string, Processor>
}
local function GetProcessor(key)
    local obj = store.cache[key]
    if not obj then
        return nil
    end
    local valid = obj:IsValid()
    if not valid then
        return nil
    end
    return obj
end
local p = GetProcessor("x")
if not p then return end
p:Execute()
