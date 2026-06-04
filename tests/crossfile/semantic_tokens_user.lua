---@diagnostic disable: unused-local
-- Field / method access is intentionally left to the editor's built-in Lua
-- grammar — the LS emits no semantic token for these cases.

---@type Widget
local w = nil

w:Show()
-- ^ tok: none

local child = w:Child()
--              ^ tok: none

local s = w.label
--          ^ tok: none
