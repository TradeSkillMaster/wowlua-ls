-- Test: undefined-global diagnostic (requires stubs)
local function _consume(...) end

-- Should warn: typo in WoW API name
_consume(CretaeFrame)
--       ^ diag: undefined-global

-- Should NOT warn: real WoW API global
_consume(CreateFrame)
--       ^ diag: none

-- Should warn: non-existent global
_consume(nonExistentGlobal123)
--       ^ diag: undefined-global

-- Should NOT warn: real WoW global (FrameXML stub)
_consume(WOW_PROJECT_ID)
--       ^ diag: none

-- Should NOT warn: _G is a built-in Lua global
_consume(_G)
--       ^ diag: none

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: undefined-global
_consume(totallyFakeGlobal)
-- ^ diag: none

-- Should NOT warn: field access on grouped expression (not a global)
local t1 = { hex = "red" }
local t2 = { hex = "blue" }
local _color = (t1 or t2).hex
--                         ^ diag: none
