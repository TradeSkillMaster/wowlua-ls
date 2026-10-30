---@diagnostic disable: unused-local
---@param mod BossModProto
local function Pull(mod)
    local state = mod.combatState
    --    ^ hover: (local) state: CombatState {
end

---@param warning BossWarning
local function WarningKey(warning)
    return warning.option
    --             ^ hover: (field) option: string
end
