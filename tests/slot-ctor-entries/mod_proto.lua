---@diagnostic disable: create-global
-- A prototype registry: `GetPrototype("Mod")` is the class `Mod`, which this file
-- and `mod_core.lua` declare.
local protos = {}

---@generic T
---@param name `T`
---@return T
function GetPrototype(name)
    protos[name] = protos[name] or {}
    return protos[name]
end

---@class Mod
local modProto = GetPrototype("Mod")

function modProto:Stop()
    self.vb.bossLeft = 2
end
