---@diagnostic disable: unused-local
-- Test: deep class inheritance
-- Hierarchy: Object -> ScriptRegion -> Region -> Frame -> Button

---@type Button
local btn = nil
--    ^ hover: (local) btn: Button {  def: local

-- Also test @field inheritance
---@type Frame
local f = nil
--    ^ hover: (local) f: Frame {  def: local

-- A parent field typed exactly `nil` (`@field slot nil`) that a subclass
-- re-declares as a bare-`table` placeholder must NOT surface as the degenerate
-- `never` type. `strip_nil` maps the parent's nil-only type to the empty union,
-- and the field-access parent-class fallback (which fires because the child's
-- own field resolves to a `table` placeholder) used to push that empty union
-- through as the result. The child's own `table` must win instead.
---@class NilSlotParent
---@field slot nil
local NilSlotParent = {}

---@class NilSlotChild: NilSlotParent
---@field slot table
local NilSlotChild = {}

local slotVal = NilSlotChild.slot
--    ^ hover: (local) slotVal: table
