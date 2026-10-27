-- A generic class's `@field value T` can't bind on a bare redeclaration, which
-- declares no type parameters: the constructor's entry keeps its value's type.
-- generic_ctor.lua redeclares the class from another file.

---@class RedeclBox<T>
---@field value T
local RedeclBox = {}

---@param v string
local function Make(v)
    --- @class RedeclBox
    local box = { value = v }
    local upper = box.value:upper()
    --                ^ hover: (field) value: string
    return box, upper
end

---@class RedeclPair<T>
---@field first T
local RedeclPair = {}

return RedeclBox, Make, RedeclPair
