---@diagnostic disable: unused-local

-- A file re-declaring the class sees the fields the deep writes reached.
---@class DeepFieldRoot
local Root = DeepFieldRoot

local scale = Root.Config.scale
--    ^ hover: (local) scale: number
local name = Root.Util.name
--    ^ hover: (local) name: string
