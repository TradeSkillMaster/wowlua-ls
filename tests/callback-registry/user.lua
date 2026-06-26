---@diagnostic disable: unused-local, create-global

-- Consuming the registry by string-literal event name (the dominant addon pattern).
-- Event names complete from the registry's declared set, and an unknown name is
-- flagged by `unknown-callback-event`.
local _, addonTable = ...

local function setup()
    addonTable.CallbackRegistry:RegisterCallback("BagOpened", function() end)
    --                                            ^ comp: BagClosed, BagOpened, SettingChanged

    addonTable.CallbackRegistry:TriggerEvent("SettingChanged")

    addonTable.CallbackRegistry:RegisterCallback("DoesNotExist", function() end)
    --                                           ^ diag: unknown-callback-event
end

-- A `self:` receiver is never a stable cross-file key, so it is dropped: events
-- registered on `self` aren't validated and can't collide across classes. Without
-- the self-skip, `OtherEvent` below would false-positive against `CompEvent`.
local Comp = {}
function Comp:OnLoad()
    self:GenerateCallbackEvents({ "CompEvent" })
end
function Comp:Use()
    self:RegisterCallback("OtherEvent", function() end)
end
