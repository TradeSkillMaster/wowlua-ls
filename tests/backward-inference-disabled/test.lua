---@diagnostic disable: unused-function
-- Test: backward inference disabled via .wowluarc.json config
-- With `backward_param_types: false`, unannotated params stay untyped.

local function addOne(x)
--                    ^ hover: (param) x: ?
    return x + 1
end

---@param tag string
local function logTag(tag) end

local function forwardTag(t)
--                        ^ hover: (param) t: ?
    logTag(t)
end
