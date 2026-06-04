---@diagnostic disable: unused-local
-- Uses types from the external library directory.

---@type ExtWidget
local w = { id = 1, label = "ok" }
--    ^ hover: (local) w: ExtWidget

local s = FormatWidget(w)
--    ^ hover: (local) s: string

---@diagnostic enable: unused-local
local unused = 123
-- ^ diag: unused-local
