-- Cross-file test: callable class via setmetatable __call through table field

---@class CallableWidget
local CallableWidget = {}
CallableWidget.__index = CallableWidget

setmetatable(CallableWidget, {
    __call = function(self, id)
        return setmetatable({}, self)
    end
})

---@class WidgetRegistry
local WidgetRegistry = {}
WidgetRegistry.Widget = CallableWidget
