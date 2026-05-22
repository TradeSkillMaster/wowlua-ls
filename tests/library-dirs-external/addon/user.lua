-- Uses types from the external library directory.

---@type ExtWidget
local w = { id = 1, label = "ok" }
--    ^ hover: (local) w: ExtWidget

local s = FormatWidget(w)
--    ^ hover: (local) s: string

local unused = 123
-- ^ diag: unused-local
