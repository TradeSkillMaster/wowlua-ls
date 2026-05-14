---@alias Callback fun(x: number): string

---@param cb Callback
---@return string
local function invoke(cb)
    return cb(42)
end

local result = invoke(function(x)
    return tostring(x)
end)
