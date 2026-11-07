---@class ClassWriterRunner
local runner = ClassWriterRunner

function runner:Run()
    local warper = runner.Warper:New()
    self.warper = warper
end
