---@diagnostic disable: unused-local
local _, ns = ...
local i = ns.iField
--    ^ hover: (local) i: boolean  def: local
-- The shared class's fields must NOT leak into an unrelated addon.
local g = ns.gField
--    ^ hover: (local) g: ?  def: local
