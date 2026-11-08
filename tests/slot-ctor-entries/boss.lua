---@diagnostic disable: create-global
-- A constructor entry's record other files of the class fill under keys known
-- only at run time: a read under such a key is one of those values.
---@class Boss
local bossProto = {}

---@return Boss
function NewBoss()
    ---@class Boss
    local obj = setmetatable({
        Options = { Enabled = true },
    }, { __index = bossProto })
    return obj
end
