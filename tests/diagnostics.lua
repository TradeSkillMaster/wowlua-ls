-- Test: semantic diagnostics (@deprecated, @nodiscard, @diagnostic suppression)

---@deprecated
local function oldFunc()
  return 1
end

---@nodiscard
local function mustUse()
  return 42
end

-- Should warn: deprecated
oldFunc()
-- ^ diag: deprecated

-- Should warn: discard-returns
mustUse()
-- ^ diag: discard-returns

-- Should NOT warn: return value used
local x = mustUse()
-- ^ diag: none

-- Should warn: deprecated (return value used but still deprecated)
local y = oldFunc()
--        ^ diag: deprecated

-- Should NOT warn: suppressed by disable-next-line
---@diagnostic disable-next-line: deprecated
oldFunc()
-- ^ diag: none

-- Should NOT warn: suppressed by disable-next-line (all codes)
---@diagnostic disable-next-line
mustUse()
-- ^ diag: none

-- Should NOT warn: inside disable range
---@diagnostic disable: deprecated
oldFunc()
-- ^ diag: none
oldFunc()
-- ^ diag: none
---@diagnostic enable: deprecated

-- Should warn again: outside disable range
oldFunc()
-- ^ diag: deprecated

-- Should NOT warn: suppressed by disable-line on same line
oldFunc() ---@diagnostic disable-line: deprecated
-- ^ diag: none

-- ── Type mismatch diagnostics ──────────────────────────────────────────────

---@param x number
---@param y string
local function typed(x, y) return x end

-- Should warn: string where number expected
typed("hello", "world")
--    ^ diag: type-mismatch

-- Should NOT warn: correct types
typed(42, "ok")
--    ^ diag: none

-- Should warn: boolean where number expected
typed(true, "ok")
--    ^ diag: type-mismatch

-- Should warn: second arg wrong type too
typed(42, 99)
--        ^ diag: type-mismatch

-- Should NOT warn: nil is fine for optional params
---@param a number|nil
local function optParam(a) end
optParam(nil)
--       ^ diag: none

-- Should warn: string is not number|nil
optParam("nope")
--       ^ diag: type-mismatch

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: type-mismatch
typed("hello", "world")
--    ^ diag: none

-- ── Return type mismatch ────────────────────────────────────────────────────

---@return number
local function retNum() return "hello" end
--                             ^ diag: return-mismatch

---@return number
local function retNumOk() return 42 end
--                               ^ diag: none

---@return string|number
local function retUnion() return "hello" end
--                               ^ diag: none

---@return string
local function retNil() return nil end
--                             ^ diag: return-mismatch

-- Suppression works
---@return number
---@diagnostic disable-next-line: return-mismatch
local function retSuppressed() return "hello" end
-- ^ diag: none
