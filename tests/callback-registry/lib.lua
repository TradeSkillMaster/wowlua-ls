---@diagnostic disable: unused-local, create-global

-- Minimal CallbackRegistryMixin definition carrying the producer/consumer
-- annotations. (In a real project these come from the bundled WoW API stubs; the
-- test defines them in-workspace so the scan picks up the methods without stubs.)

---@class CallbackRegistryMixin
CallbackRegistryMixin = {}

---@generates-events 1 Event
---@param events string[]
function CallbackRegistryMixin:GenerateCallbackEvents(events) end

---@callback-event-arg 1
---@param event string
---@param func function
function CallbackRegistryMixin:RegisterCallback(event, func) end

---@callback-event-arg 1
---@param event string
function CallbackRegistryMixin:TriggerEvent(event, ...) end
