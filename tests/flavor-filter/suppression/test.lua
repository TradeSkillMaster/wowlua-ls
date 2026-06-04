-- Inline diagnostic suppression silences wrong-flavor-api.

-- Fires normally.
AbbreviateLargeNumbers(1)
-- ^ diag: wrong-flavor-api

---@diagnostic disable-next-line: wrong-flavor-api
AbbreviateLargeNumbers(2)

-- Also suppressible via its LuaLS-style alias? For now just the exact code.
AbbreviateLargeNumbers(3) ---@diagnostic disable-line: wrong-flavor-api
