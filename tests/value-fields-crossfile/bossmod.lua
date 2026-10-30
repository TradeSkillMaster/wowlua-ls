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

-- A field two constructors of one class write a concatenation, one of a value that
-- may be `false` (an error if it is): the concatenation is a string, so the field is.
---@class BossWarning
local warningProto = {}
local warningMt = { __index = warningProto }

function warningProto:OptionKey()
    return self.option
    --          ^ hover: (field) option: string
end

---@param name string|boolean?
---@param version any
function proto:NewWarning(name, version)
    ---@class BossWarning
    local obj = setmetatable({}, warningMt)
    local optionId = name or name ~= false and "Warning"
    if optionId then
        obj.option = optionId .. (version or "")
    end
    return obj
end

---@param spellId number
function proto:NewSpecWarning(spellId)
    ---@class BossWarning
    local obj = setmetatable({}, warningMt)
    obj.option = "SpecWarn" .. spellId
    return obj
end
