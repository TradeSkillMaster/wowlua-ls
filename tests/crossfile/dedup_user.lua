---@diagnostic disable: unused-local
---@type MixinA
local obj = nil

-- Unannotated duplicate should NOT create a spurious overload
obj:IsValid()
--   ^ hover: (method) function MixinA:IsValid()  def: external

obj:GetCount("test")
--   ^ hover: (method) function MixinA:GetCount(name: string)  def: external

-- Duplicate with body-derived nil should NOT create a spurious `-> nil` overload
local id = obj:GetId()
--              ^ hover: (method) function MixinA:GetId()  def: external
--    ^ hover: (local) id: number
