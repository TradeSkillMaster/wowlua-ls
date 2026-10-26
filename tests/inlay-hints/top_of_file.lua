local function getCount() return 42 end
--                       ^ hint: -> number
---@diagnostic disable: unused-local
-- A definition at the very top of the file gets its hints, not an annotation's
-- `fun(...)`, which starts at offset 0 too.
---@alias TopOfFileCb fun(x: number): string
local count = getCount()
