---@param v string
local function Make(v)
    --- @class RedeclBox
    local box = { value = v }
    local upper = box.value:upper()
    --                ^ hover: (field) value: string
    return box, upper
end

-- A child of a parameterized generic class keeps its constructor entry rather than
-- reading the parent's `first` in other files, which there is its unbound `T`
-- (reader_child.lua).
---@param v string
local function MakeChild(v)
    --- @class RedeclPairChild : RedeclPair<string>
    local child = { first = v }
    return child
end

return Make, MakeChild
