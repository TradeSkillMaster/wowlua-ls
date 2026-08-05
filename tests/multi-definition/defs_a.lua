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

-- A global declared explicitly via `_G.X` in two files. Its definition site is
-- the `X` name token, not the whole `_G.X = ...` statement (definition A).
_G.SharedExplicitGlobal = {}

-- A global function defined in only this file.
function OnlyOnce()
end
