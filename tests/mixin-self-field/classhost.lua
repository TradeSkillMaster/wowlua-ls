---@diagnostic disable: create-global
-- A `@class` mixin: the funcall self-field scanner registers `self.Widget`
-- cross-file, so the mixin intersection rides the cross-file field type and the
-- mixin methods resolve both same-file (here) and cross-file (see reader.lua).
---@class PanelClassMixin
PanelClassMixin = {}

function PanelClassMixin:OnLoad()
  self.Widget = CreateFrame("Frame", nil, self)
  Mixin(self.Widget, TestWidgetMixin)
  self.Widget:Render()
  self.Widget:Hide()
end

function PanelClassMixin:Refresh()
  self.Widget:Cancel()
  -- Negative control: a non-mixin'd field still reports unknown methods.
  self.Plain = CreateFrame("Frame", nil, self)
  self.Plain:NotAMethod()
  --         ^ diag: undefined-field
end
