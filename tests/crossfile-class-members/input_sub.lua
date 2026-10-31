-- `input_base.lua`'s fields read from a subclass and another class in this file,
-- as conditions, write targets, keys and values.
---@diagnostic disable: unused-local
local NumberInput = DefineClass("NumberInput", "InputBase")

function NumberInput:Apply(value)
    if self._target then
    --         ^ hover: (field) _target: table?
        self._target[self._key] = value
        --    ^ hover: (field) _target: table?
        --                 ^ hover: (field) _key: string?
    end
end

function NumberInput:Read()
    local target = self._target
    --    ^ hover: (local) target: table?
    return target
end

---@param store InputStore
local function dump(store)
    local data = store._data
    --    ^ hover: (local) data: table
    local options = store._options
    --    ^ hover: (local) options: any
end
