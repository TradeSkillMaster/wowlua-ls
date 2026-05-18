-- Calling a callable class (setmetatable + __call) through a table field
-- should not trigger cannot-call or redundant-parameter

---@class WidgetRegistry
local _, WidgetRegistry = ...

-- Direct access to callable class through field: no cannot-call
local _w = WidgetRegistry.Widget(1)
--                              ^ diag: none
