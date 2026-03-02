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

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: undefined-global
_consume(totallyFakeGlobal)
-- ^ diag: none
