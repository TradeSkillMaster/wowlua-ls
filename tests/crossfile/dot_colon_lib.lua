-- Cross-file test: dot-defined methods called with colon syntax
-- Defines a class with __static accessor and dot-defined methods
local addonName, ns = ...

---@class DotColonClass
---@accessor __static
---@field _STATE_SCHEMA string
local DCC = {}
ns.DCC = DCC

---@return string
function DCC.__static._ExtendStateSchema(cls)
    return cls._STATE_SCHEMA
end

---@return string
function DCC.__static._AddActionScripts(cls, name)
    return cls._STATE_SCHEMA
end
