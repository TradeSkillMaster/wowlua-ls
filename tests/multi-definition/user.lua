---@diagnostic disable: unused-local, unused-function

-- A global function defined in two files yields two definition sites.
SharedGlobal()
-- ^ defs: 2

-- A global variable assigned in two files yields two definition sites.
local v = SharedVar
--        ^ defs: 2

-- A global function defined in only one file still yields exactly one site.
OnlyOnce()
-- ^ defs: 1  def: external

-- Go-to-definition on a @class name in an annotation lists every partial
-- declaration (one per file).
---@type SharedClass
--       ^ defs: 2
local obj = nil

-- Go-to-definition on an @alias name lists every declaration.
---@alias LocalAlias SharedAlias
--                   ^ defs: 2

-- A variable typed as the partial class navigates to both declarations via
-- go-to-type-definition.
local function readObj()
    return obj
--         ^ typedefs: 2
end
