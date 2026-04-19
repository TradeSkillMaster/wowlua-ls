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
