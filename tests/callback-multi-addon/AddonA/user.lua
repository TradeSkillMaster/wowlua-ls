---@diagnostic disable: unused-local, create-global
local _, addonTable = ...

local function f()
    addonTable.CallbackRegistry:RegisterCallback("AlphaEvent", function() end)

    -- BetaEvent belongs to AddonB; addon-scoped keys keep it unknown here.
    addonTable.CallbackRegistry:RegisterCallback("BetaEvent", function() end)
    --                                           ^ diag: unknown-callback-event
end
