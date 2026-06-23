---@diagnostic disable: unused-local
-- Uses globals/types from a sibling library directory referenced via a
-- relative `../shared` path. Types and globals should be visible, and
-- user-file diagnostics should still work normally.

---@type SharedHelper
local h = { id = 1 }
--    ^ hover: (local) h: SharedHelper

local result = SharedFormat(h)
--    ^ hover: (local) result: number

local v = SharedLib.Value()
--    ^ hover: (local) v: number

---@diagnostic enable: unused-local
local unused = 123
-- ^ diag: unused-local
