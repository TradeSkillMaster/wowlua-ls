---@meta _
-- Override EventRegistry to declare inheritance from CallbackRegistryMixin
-- and type the event parameter as FrameEvent on callback registration methods.
-- Upstream Ketho stubs use `EventRegistry = CreateFromMixins(CallbackRegistryMixin)`
-- without a @class annotation, and CallbackRegistryMixin:RegisterCallback lacks
-- a typed event parameter.

---@class EventRegistry : CallbackRegistryMixin

---@param event FrameEvent
---@param func function
---@param owner? any
---@return any
function EventRegistry:RegisterCallback(event, func, owner, ...) end

---@param event FrameEvent
---@param func function
---@param owner? any
---@return table
function EventRegistry:RegisterCallbackWithHandle(event, func, owner, ...) end
