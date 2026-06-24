---@diagnostic disable: unused-local, unused-function, duplicate-doc-alias

-- A partial @class declaration split across files (defs_a + defs_b).
---@class SharedClass
---@field a number
local SharedClassA = {}

-- A type alias declared in both files.
---@alias SharedAlias number

-- A global function defined in two files.
---Shared global function (definition A).
function SharedGlobal()
end

-- A global variable assigned in two files.
SharedVar = 1

-- A global function defined in only this file.
function OnlyOnce()
end
