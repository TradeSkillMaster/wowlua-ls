-- Regression: fields/methods on a workspace-scanned class must NOT carry the
-- `defaultLibrary` modifier (that modifier is reserved for WoW API stubs).
-- This was broken when cross-file classes landed in EXT space and every EXT
-- table lookup blindly applied the modifier.

---@type Widget
local w = nil

w:Show()
-- ^ tok: method

local child = w:Child()
--              ^ tok: method

local s = w.label
--          ^ tok: property
