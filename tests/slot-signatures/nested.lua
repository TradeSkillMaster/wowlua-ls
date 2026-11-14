---@diagnostic disable: create-global
-- Methods defined through a field a nested `---@class` declares the class's
-- constructor of.
---@class SigCoreT
SigCore = {}
---@class SigTestT
SigCore.Test = {}

---@class SigTestT
local test = SigCore.Test

---@class SigWarper
test.Warper = {
    frames = {},
}

function test.Warper:Start()
    return self.frames
end

-- A function field declared with a typed vararg.
---@class SigReg
---@field Fire fun(self: SigReg, name: string, ...: unknown)
SigRegistry = {}

---@type SigReg
local reg = SigRegistry
reg:Fire("x", 1)
