---@meta

-- Test: @meta file with partial annotations → no fire (all diagnostics suppressed)
---@param x number
local function _partialInMeta(x, y)
--                            ^ diag: none
--                               ^ diag: none
    return x + y
end
