---@diagnostic disable: unused-local
-- AddonP sees its own runtime fields, @field declarations, and the shared field.
local _, ns = ...
local a = ns.pUnique
--    ^ hover: (local) a: number  def: local
local sec = ns.secretField
--    ^ hover: (local) sec: string  def: local
local sh = ns.shared
--    ^ hover: (local) sh: table  def: local
-- A @field-only declaration whose name collides with another addon's runtime
-- write must survive the cross-addon-leak strip (regression).
local col = ns.collide
--    ^ hover: (local) col: boolean  def: local
-- AddonQ's unique field must NOT be visible.
local q = ns.qUnique
--    ^ hover: (local) q: ?  def: local
