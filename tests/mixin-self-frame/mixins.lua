---@diagnostic disable: unused-local, unused-function
-- A mixin table wired to a frame via XML is conventionally a plain global with
-- no Lua `@class`. In the file that *defines* the mixin, `self` inside its
-- methods used to be just a structural bag of its collected self-fields, so
-- passing `self` to a `Frame`-typed parameter reported a spurious type-mismatch.
-- The fix flags such tables `open_mixin` (like CreateFromMixins), so `self` is
-- accepted wherever a frame/table is expected while staying permissive for the
-- frame/runtime fields a mixin receives dynamically.

---@param f Frame
local function NeedsFrame(f) end

-- (1) Named frame, `mixin="NamedMixin"` attribute.
NamedMixin = {}

function NamedMixin:Setup()
  self.count = 1
  -- `self` must satisfy the `Frame` parameter — no type-mismatch.
  NeedsFrame(self)
  -- A collected self-field still resolves.
  local n = self.count
  --             ^ hover: (field) count: number
  -- A frame/runtime field the mixin receives dynamically is permissive
  -- (no undefined-field), exactly like a CreateFromMixins mixin.
  local p = self.dynamicallyAdded
  return n, p
end

-- (2) Unnamed (parentKey-only) child frame, `mixin="UnnamedMixin"` attribute.
UnnamedMixin = {}

function UnnamedMixin:Setup()
  self.value = "x"
  NeedsFrame(self)
  local v = self.value
  --             ^ hover: (field) value: string
  return v
end

-- (3) Mixin applied via an inline `Mixin(self, InlineMixin)` <OnLoad> script.
InlineMixin = {}

function InlineMixin:Setup()
  self.label = true
  NeedsFrame(self)
  local l = self.label
  --             ^ hover: (field) label: true
  return l
end
