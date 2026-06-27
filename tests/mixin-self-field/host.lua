local _, addonTable = ...
addonTable.UI = {}

-- An addon-namespace mixin table (no `@class`), like a frame mixin promoted only
-- by `Mixin()`. Its `self.field = CreateFrame(...)` writes are typed per-file, so
-- the mixin augmentation must happen in the per-file engine to be visible from
-- sibling methods (the case the cross-file funcall scanner can't reach).
addonTable.UI.PanelMixin = {}

function addonTable.UI.PanelMixin:OnLoad()
  self.Child = CreateFrame("Frame", nil, self)
  Mixin(self.Child, TestWidgetMixin)
  self.Child:Render()
  -- ^ same-method read of a mixin method resolves (no undefined-field)

  -- Multiple mixins on one field.
  self.Multi = CreateFrame("Frame", nil, self)
  Mixin(self.Multi, TestWidgetMixin, TestExtraMixin)

  -- Mixin call nested inside control flow is still detected.
  self.Cond = CreateFrame("Frame", nil, self)
  if self.Cond then
    Mixin(self.Cond, TestWidgetMixin)
  end

  -- A field that is NOT mixin'd keeps its bare Frame type.
  self.Plain = CreateFrame("Frame", nil, self)
end

function addonTable.UI.PanelMixin:Layout()
  -- Cross-method reads: the mixin methods resolve because the field's registered
  -- type carries the intersection (Frame & mixin), not just the bare Frame.
  self.Child:Cancel()
  self.Child:Render()
  self.Multi:Render()
  self.Multi:Extra()
  self.Cond:Render()
  -- Base Frame methods still resolve through the intersection.
  self.Child:Hide()
  self.Multi:Show()
  -- A non-mixin method on a non-mixin'd field still errors (no blanket suppression).
  self.Plain:NotAMethod()
  -- ^ diag: undefined-field
end
