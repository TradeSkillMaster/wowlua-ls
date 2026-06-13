-- Cross-file references test: this file consumes the globals/@class from references_defs.lua

local r = GlobalRefFn(5)
local s = GlobalRefFn(10)

---@type RefCrossClass
local obj = nil

if obj then
    print(obj:Greet())
    print(obj.name)
end

-- Union-typed receiver: u:Shared() must count as a reference to BOTH
-- RefUnionA:Shared and RefUnionB:Shared, not just the first union member.
---@param u RefUnionA|RefUnionB
local function useUnion(u)
    u:Shared()
end
