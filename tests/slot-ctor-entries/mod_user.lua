---@diagnostic disable: unused-local
---@param mod Mod
local function readMod(mod)
    -- The entry's own value, with the members files write onto it.
    local vb = mod.vb
    --    ^ hover: (local) vb: { bossLeft: number, phase: number }
    local phase = mod.vb.phase
    --    ^ hover: (local) phase: number
    -- A parameter the constructor's function converts.
    local id = mod.id
    --    ^ hover: (local) id: string
    local locale = mod.locale
    --    ^ hover: (local) locale: ModLocale {\n  name: string\n}
    local sort = mod.sort
    --    ^ hover: (local) sort: string[]
    local flag = mod.flag
    --    ^ hover: (local) flag: boolean?
end
