-- Cross-file references test: this file defines globals and a @class used in references_user.lua

---@class RefCrossClass
---@field name string
local RefCrossClass = {}

function RefCrossClass:Greet()
    return "hi " .. self.name
end

function GlobalRefFn(x)
    return x + 1
end

_G.RefCrossClass = RefCrossClass

-- Two @class types sharing a method name (`Shared`), consumed via a union-typed
-- receiver in references_user.lua. The chain resolver returns only the FIRST
-- union member, so find-references on the SECOND member's method (RefUnionB)
-- must still reach the union call site via union-member matching.
---@class RefUnionA
local RefUnionA = {}
function RefUnionA:Shared()
    return 1
end
_G.RefUnionA = RefUnionA

---@class RefUnionB
local RefUnionB = {}
function RefUnionB:Shared()
    return 2
end
_G.RefUnionB = RefUnionB
