-- A class field one of the class's files writes with the return of a function
-- another of its files defines: the class harvest reads that return.
---@class ClassWriterRunner
local runner = ClassWriterRunner

---@class ClassWriterWarper
runner.Warper = {}
local mt = { __index = runner.Warper }

function runner.Warper:New()
    ---@class ClassWriterWarper
    local obj = setmetatable({ factor = 1 }, mt)
    return obj
end

function runner.Warper:Stop() end
