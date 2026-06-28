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

-- A trailing inline `disable-line` on the same comment line suppresses the
-- warning (and must not itself trigger a `malformed-annotation` for the @class).
---@class Button ---@diagnostic disable-line: class-shadows-builtin
---@field btn string

-- `disable-next-line` on the line above the @class also suppresses it.
---@diagnostic disable-next-line: class-shadows-builtin
---@class Texture
---@field tex string

-- A same-line disable-line for an *unrelated* code does not suppress it.
---@class StatusBar ---@diagnostic disable-line: unused-local
-- ^ diag: class-shadows-builtin
---@field bar string
