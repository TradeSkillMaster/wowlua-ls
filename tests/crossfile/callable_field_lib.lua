-- Cross-file test: callable class via setmetatable __call through table field

---@class CallableWidget
local CallableWidget = {}
CallableWidget.__index = CallableWidget

setmetatable(CallableWidget, {
    __call = function(self, id)
        return setmetatable({}, self)
    end
})

-- Callable class whose local variable name differs from the @class name.
-- The setmetatable __call must still be detected cross-file via the variable.
---@class RenamedWidget
local Action = {}
Action.__index = Action

setmetatable(Action, {
    __call = function(self, id)
        return setmetatable({}, self)
    end
})

---@class WidgetRegistry
local WidgetRegistry = {}
WidgetRegistry.Widget = CallableWidget
WidgetRegistry.Renamed = Action
