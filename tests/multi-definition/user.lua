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

-- A method present in the built-in stubs AND redefined by a workspace file
-- (framexml_mixin.lua) lists both definition sites — stub and workspace —
-- just like a multiply-defined global/@class. This is the `library` overlap
-- case: go-to-definition on the method offers every site, not only the stub.
---@type CallbackRegistryMixin
local reg = nil
reg:GenerateCallbackEvents({})
--  ^ defs: 2

-- Same overlap case, but for a method on a stub *namespace* table rather than a
-- @class: `Settings` is a plain scope-0 global table in the built-in stubs, and
-- framexml_namespace.lua redefines Settings.RegisterVerticalLayoutCategory.
-- Go-to-definition must offer both the stub and the workspace site.
Settings.RegisterVerticalLayoutCategory("x")
--       ^ defs: 2
