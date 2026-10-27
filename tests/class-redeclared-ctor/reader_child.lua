-- The workspace build types a child's constructor entry from its file
-- (generic_ctor.lua) rather than reading the parent's unbound `T`; the cold build
-- leaves it `any`.

---@param child RedeclPairChild
local function readChild(child)
    return child.first
    --           ^ hover: (field) first: string
end

return readChild
