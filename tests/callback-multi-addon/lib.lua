---@diagnostic disable: unused-local, create-global

---@class CallbackRegistryMixin
CallbackRegistryMixin = {}

---@generates-events 1 Event
---@param events string[]
function CallbackRegistryMixin:GenerateCallbackEvents(events) end

---@callback-event-arg 1
---@param event string
---@param func function
function CallbackRegistryMixin:RegisterCallback(event, func) end
