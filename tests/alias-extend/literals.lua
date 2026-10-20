---@diagnostic disable: unused-local, unused-function
-- Workspace aliases extended across files; checked with and without the stubs.

---@type XaMode
local mode = "a"
--    ^ hover: (local) mode: "a" | "b"

---@param u XaUnit
local function unit(u) end
unit("")
--    ^ comp: x, y

---@param u XoUnit
local function opaqueUnit(u) end
opaqueUnit("")
--          ^ comp: x, y
