---@meta _
-- Annotate CallbackRegistryMixin:GenerateCallbackEvents with @generates-events so
-- that calling it on a mixin/class synthesizes the enum-like `Event` table the
-- callback registry creates at runtime. For example:
--
--   ScrollBoxListMixin:GenerateCallbackEvents({ "OnDataProviderReassigned", ... })
--
-- populates `ScrollBoxListMixin.Event = { OnDataProviderReassigned = "...", ... }`,
-- which addons reference as `ScrollBoxListMixin.Event.OnDataProviderReassigned`.
-- Upstream Ketho stubs declare the method but lack the side-effect annotation, so
-- those `.Event.*` accesses falsely report `undefined-field`.

---@generates-events 1 Event
---@param events string[]
function CallbackRegistryMixin:GenerateCallbackEvents(events) end

-- The callback-registry consumer methods take an event name as their first
-- argument. `@callback-event-arg 1` lets the language server complete the
-- registered event names inside the string and flag unknown ones
-- (`unknown-callback-event`, off by default).

---@callback-event-arg 1
---@param event string
---@param func function
---@param owner? any
---@return any
function CallbackRegistryMixin:RegisterCallback(event, func, owner, ...) end

---@callback-event-arg 1
---@param event string
---@param func function
---@param owner? any
---@return table
function CallbackRegistryMixin:RegisterCallbackWithHandle(event, func, owner, ...) end

---@callback-event-arg 1
---@param event string
---@param owner any
function CallbackRegistryMixin:UnregisterCallback(event, owner) end

---@callback-event-arg 1
---@param event string
---@param ... any
function CallbackRegistryMixin:TriggerEvent(event, ...) end
