---@diagnostic disable: create-global
-- A settings record a constructor builds, which `opts_add.lua` fills under keys
-- known only at run time and `opts_store.lua` replaces.
---@class OptMod
local proto = GetPrototype("OptMod")
local mt = { __index = proto }

---@return OptMod
function NewOptMod()
    ---@class OptMod
    local obj = setmetatable({
        Options = { Enabled = true },
        Durations = { Enabled = true },
        Items = { name = "x" },
    }, mt)
    return obj
end
