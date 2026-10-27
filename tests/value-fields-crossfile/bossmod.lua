---@diagnostic disable: undefined-field
-- A class declared a second time in the file (a parameter retyped `---@class`):
-- what one declaration's methods write reaches the field through the others.
local _, private = ...
---@class BossModProto
local proto = private:GetPrototype("BossModProto")

---@param mod BossModProto
function private:EndCombat(mod)
    ---@class BossModProto
    mod = mod
    if not mod.combatState then return end
    --         ^ hover: (field) combatState: CombatState {
end

---@param mod BossModProto
function private:StartCombat(mod)
    ---@class BossModProto
    mod = mod
    return mod.combatState
end

function proto:RegisterCombat(kind)
    ---@class CombatState
    local info = { kind = kind }
    self.combatState = info
end

function proto:Check()
    return self.combatState
    --          ^ hover: (field) combatState: CombatState {
end
