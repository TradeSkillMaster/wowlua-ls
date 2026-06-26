---@diagnostic disable: unused-local, create-global

-- Workspace-only mixin (the stubs can't know these custom events): exercises the
-- `@generates-events` synthesis end to end in the test harness, independent of any
-- pre-existing stub `.Event` table.

---@class WidgetRegistryMixin
WidgetRegistryMixin = {}

---@generates-events 1 Event
---@param events string[]
function WidgetRegistryMixin:GenerateCallbackEvents(events) end

---@class MyWidget : WidgetRegistryMixin
MyWidget = {}

MyWidget:GenerateCallbackEvents({ "OnCustomFoo", "OnCustomBar" })

local a = MyWidget.Event.OnCustomFoo
--                       ^ hover: (field) OnCustomFoo: string
local b = MyWidget.Event.OnCustomBar
--                       ^ hover: (field) OnCustomBar: string
