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

-- Should warn: discard-returns
mustUse()

-- Should NOT warn: return value used
local x = mustUse()

-- Should warn: deprecated (return value used but still deprecated)
local y = oldFunc()

-- Should NOT warn: suppressed by disable-next-line
---@diagnostic disable-next-line: deprecated
oldFunc()

-- Should NOT warn: suppressed by disable-next-line (all codes)
---@diagnostic disable-next-line
mustUse()

-- Should NOT warn: inside disable range
---@diagnostic disable: deprecated
oldFunc()
oldFunc()
---@diagnostic enable: deprecated

-- Should warn again: outside disable range
oldFunc()

-- Should NOT warn: suppressed by disable-line on same line
oldFunc() ---@diagnostic disable-line: deprecated
