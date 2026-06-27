---@diagnostic disable: unused-local
-- `Mixin(tbl.field, M)` on a function-local table (not `self`): the per-file
-- augmentation narrows the field within the same function, mirroring the
-- plain-local `Mixin(f, M)` case but on a field target.
local function makeLocal()
  local obj = {}
  obj.W = CreateFrame("Frame")
  Mixin(obj.W, TestWidgetMixin)
  obj.W:Render()
  obj.W:Cancel()
  obj.W:Hide()
  -- A local that *copies* the augmented field sees the intersection too — the
  -- augmentation refreshes such a local's already-resolved type, matching how the
  -- plain-local `Mixin(f, M)` form lets a later `local c = f` resolve.
  local copy = obj.W
  copy:Render()
  copy:Cancel()
end

-- Negative control: a non-mixin'd local field keeps its bare type, so an unknown
-- method still errors (the augmentation doesn't blanket-suppress field checks).
local function plainLocal()
  local obj = {}
  obj.Plain = CreateFrame("Frame")
  obj.Plain:NotAMethod()
  --         ^ diag: undefined-field
end
