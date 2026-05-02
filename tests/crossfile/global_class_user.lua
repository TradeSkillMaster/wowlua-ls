-- Cross-file test: @class on a global assignment should merge with cross-file class definition

---@class MixinItem
MixinItemMixin = {}

-- Methods on the global should be associated with the class
---@return number
function MixinItemMixin:GetValue()
    return self.value
end

-- Fields from cross-file @class declaration should be accessible
local n = MixinItemMixin.name
--    ^ hover: (local) n: string  def: local

local v = MixinItemMixin.value
--    ^ hover: (local) v: number  def: local

-- Cross-file method should be accessible
MixinItemMixin:GetName()
--             ^ hover: (method) function MixinItem:GetName()  def: external

-- Method defined in this file should work
MixinItemMixin:GetValue()
--             ^ hover: (method) function MixinItem:GetValue()  def: local
