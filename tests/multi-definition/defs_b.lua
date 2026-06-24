---@diagnostic disable: unused-local, unused-function, duplicate-doc-alias

-- The second partial @class declaration for SharedClass.
---@class SharedClass
---@field b number
local SharedClassB = {}

-- The second declaration of the SharedAlias type alias.
---@alias SharedAlias number

-- The second definition of the SharedGlobal global function.
---Shared global function (definition B).
function SharedGlobal()
end

-- The second assignment of the SharedVar global variable.
SharedVar = 2
