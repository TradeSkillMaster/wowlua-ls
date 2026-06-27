---@diagnostic disable: unused-local, unused-function

-- A workspace @class that reuses a stub class name should trigger a warning.
---@class Frame
-- ^ diag: class-shadows-builtin
---@field myField string
---@field myOther number

-- The local definition should shadow the stub — constructors matching
-- the local shape should NOT trigger missing-fields.
---@type Frame
local f = { myField = "hello", myOther = 42 }

-- @diagnostic disable suppresses the warning for intentional cases.
---@diagnostic disable: class-shadows-builtin
---@class GameTooltip
---@field tip string
---@diagnostic enable: class-shadows-builtin
