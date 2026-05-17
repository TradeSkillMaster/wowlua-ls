---@type MixinA
local obj = nil

-- Unannotated duplicate should NOT create a spurious overload
obj:IsValid()
--   ^ hover: (method) function MixinA:IsValid()  def: external

obj:GetCount("test")
--   ^ hover: (method) function MixinA:GetCount(name: string)  def: external
