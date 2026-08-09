-- Inline diagnostic suppression silences wrong-flavor-api.

-- Fires normally.
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

---@diagnostic disable-next-line: wrong-flavor-api
PlayerGetTimerunningSeasonID()

-- Also suppressible via its LuaLS-style alias? For now just the exact code.
PlayerGetTimerunningSeasonID() ---@diagnostic disable-line: wrong-flavor-api
