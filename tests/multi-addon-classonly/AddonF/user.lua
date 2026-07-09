---@diagnostic disable: unused-local
local _, ns = ...
local f = ns.fRuntime
--    ^ hover: (local) f: number  def: local
-- AddonE's @field declarations must NOT leak in.
local c = ns.configOnly
--    ^ hover: (local) c: ?  def: local
