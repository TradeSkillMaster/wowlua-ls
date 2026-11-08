---@diagnostic disable: unused-local
---@param boss Boss
---@param name string
local function readBoss(boss, name)
    local enabled = boss.Options.Enabled
    --    ^ hover: (local) enabled: boolean
    local option = boss.Options[name .. "Color"]
    --    ^ hover: (local) option: number | boolean
end
