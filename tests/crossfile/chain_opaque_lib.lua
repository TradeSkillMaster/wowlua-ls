-- Opaque reference pattern on addon namespace.
-- Mimics TSM: LibTSMComponent has :Register() (defclass), :From() returning
-- LibTSMComponentReference, and LibTSMComponentReference has :Include()
-- with generic @return T. The component is placed on the addon namespace.
local _, ns = ...

---@class ChainComponentOpaque
ns.ChainComponentOpaque = {}

---@generic T: ChainComponentOpaque
---@defclass T
---@param name `T`
---@return T
function ns.ChainComponentOpaque:Register(name)
    return {}
end

---@param name string
---@return ChainComponentRef
function ns.ChainComponentOpaque:From(name)
    return {}
end

---@class ChainComponentRef
local REFERENCE_METHODS = {}

---@generic T
---@param path `T`
---@return T
function REFERENCE_METHODS:Include(path)
    return {}
end

-- Create a component on the addon namespace (like ADDON_TABLE.LibTSMApp = ...)
ns.ChainOpaqueApp = ns.ChainComponentOpaque:Register("ChainOpaqueApp")
