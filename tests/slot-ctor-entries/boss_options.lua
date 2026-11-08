---@class Boss
local bossProto = {}

---@param name string
---@param default boolean?
function bossProto:AddOption(name, default)
    self.Options[name] = default == nil or default
    self.Options[name .. "Color"] = 0
end
