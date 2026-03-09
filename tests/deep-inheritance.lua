-- Test: deep class inheritance
-- Hierarchy: Object -> ScriptRegion -> Region -> Frame -> Button

---@type Button
local btn = nil
--    ^ hover: (global) btn: Button {  def: local

-- Also test @field inheritance
---@type Frame
local f = nil
--    ^ hover: (global) f: Frame {  def: local
