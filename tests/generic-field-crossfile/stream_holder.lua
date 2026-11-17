local XFieldHolder = DefineXFieldClass("XFieldHolder")

function XFieldHolder:__init()
    self.stream = GetXFieldStream(function() return "a" end)
end
