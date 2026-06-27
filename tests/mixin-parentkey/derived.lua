---@diagnostic disable: unused-local, create-global, inject-field

-- `Derived = CreateFromMixins(Base)`: the derived global becomes its own class,
-- inheriting the base, and `self` in derived methods resolves to DerivedViewMixin
-- (not the base) — so its own fields, the inherited base members, and the XML
-- `parentKey` children all resolve.
DerivedViewMixin = CreateFromMixins(BaseViewMixin)

-- Re-anchored to its own class table (inherits the base), not the bare
-- CreateFromMixins return type (which would be `BaseViewMixin`).
local _d = DerivedViewMixin
--    ^ hover: (local) _d: DerivedViewMixin {

function DerivedViewMixin:OnShow()
  -- Inherited base method resolves and binds to the derived receiver.
  local c, live = self:Refresh()
  --                   ^ hover: (method) function DerivedViewMixin:Refresh()

  -- parentKey child from the XML template (mixin="DerivedViewMixin") landed on
  -- the mixin class and resolves on self.
  local container = self.Container
  --    ^ hover: (local) container: Frame

  -- An untracked runtime field on the open mixin must NOT false-positive as
  -- undefined-field (mixins receive runtime fields dynamically).
  self.runtimeOnly = 5
  local r = self.runtimeOnly

  -- A nested access rooted at the open mixin is equally permissive.
  local deep = self.Container.SomeRuntimeChild
end

-- Multi-mixin: `CreateFromMixins(A, B)` inherits every base.
MultiMixin = CreateFromMixins(BaseViewMixin, ExtraMixin)

function MultiMixin:Use()
  -- Methods from both bases resolve, and untracked fields stay permissive.
  self:Refresh()
  self:Extend()
  local x = self.whatever
end
