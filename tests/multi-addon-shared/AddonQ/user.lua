---@diagnostic disable: unused-local
-- AddonQ sees its own field and the shared field, but NOT AddonP's fields.
local _, ns = ...
local b = ns.qUnique
--    ^ hover: (local) b: string  def: local
-- Shared field (defined in both addons) must be visible here too — regression
-- for the name-collapse that dropped it in all-but-one addon.
local sh = ns.shared
--    ^ hover: (local) sh: table  def: local
-- AddonQ's own runtime write of the colliding name resolves normally.
local col = ns.collide
--    ^ hover: (local) col: boolean  def: local
-- AddonP's @field declaration must NOT leak (regression for @field pollution).
local sec = ns.secretField
--    ^ hover: (local) sec: ?  def: local
-- AddonP's runtime field and method must NOT leak.
local p = ns.pUnique
--    ^ hover: (local) p: ?  def: local
local m = ns.pMethod
--    ^ hover: (local) m: ?  def: local
