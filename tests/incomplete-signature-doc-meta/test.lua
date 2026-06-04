---@meta

-- Test: @meta file with partial annotations → no fire (all diagnostics suppressed)
---@param x number
local function _partialInMeta(x, y)
--                            ^ hover: (param) x: number
    return x + y
end
