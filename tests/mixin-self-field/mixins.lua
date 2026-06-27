---@diagnostic disable: create-global
-- Plain global mixin tables (the common WoW mixin pattern), applied to frames
-- via `Mixin(frame, ThisMixin)`. They are not `@class` declarations — exercising
-- the addon-ns / mixin-table path that the funcall self-field scanner skips.
TestWidgetMixin = {}
function TestWidgetMixin:Render() end
function TestWidgetMixin:Cancel() end

TestExtraMixin = {}
function TestExtraMixin:Extra() end

-- A `@class` mixin, exercising the cross-file funcall self-field path.
---@class TestClassMixin
TestClassMixin = {}
function TestClassMixin:ClassMethod() end
