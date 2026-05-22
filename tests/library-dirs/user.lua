-- Uses types from library directory. Types should be visible,
-- and user-file diagnostics should still work normally.

---@type LibHelper
local h = { name = "test", value = 1 }
--    ^ hover: (local) h: LibHelper

local result = FormatHelper(h)
--    ^ hover: (local) result: string

local unused = 123
-- ^ diag: unused-local
