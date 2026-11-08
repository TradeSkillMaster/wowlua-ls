---@diagnostic disable: unused-local, duplicate-set-field, create-global
-- Replaces the settings record, and reads it as the files writing it do: their
-- writes, not the elements `opts_add.lua` writes under run-time keys.
---@class OptMod
local proto = GetPrototype("OptMod")

---@param mod OptMod
---@param saved table
function ResetOptions(mod, saved)
    local defaults = {}
    for name in pairs(saved) do
        defaults[name] = true
    end
    mod["Options"] = {}
    mod["Options"] = defaults
    mod["Options"] = saved.current
end

---@param mod OptMod
function CopyOptions(mod, target)
    for option, value in pairs(mod.Options) do
    --          ^ hover: (local) value: boolean
        target[option] = value
    end
    local enabled = mod.Options.Enabled
    --    ^ hover: (local) enabled: boolean
end

---@type OptEntry[]
local entries = {}

---@param mod OptMod
function RegisterOptMod(mod)
    ---@class OptEntry
    local entry = { mod = mod, kind = "boss" }
    entries[#entries + 1] = entry
end

-- A field of an entry's value read off the class's copies here.
function AnyEnabled()
    for _, v in ipairs(entries) do
        if v.mod.Options.Enabled then
        --               ^ hover: (field) Enabled: boolean
            return true
        end
    end
end
