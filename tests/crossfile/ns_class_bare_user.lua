---@diagnostic disable: unused-local
-- Cross-file test: accesses namespace fields WITHOUT @type annotation.
-- Fields assigned to addon namespace in ns_class_defs.lua should be visible
-- through the addon namespace table directly (no @type needed).
local _, ns = ...
local ver = ns:GetVersion()
--    ^ hover: (local) ver: number  def: local
local t = ns.title
--    ^ hover: (local) t: string  def: local
local dbg = ns.config.debug
--    ^ hover: (local) dbg: boolean  def: local
-- Runtime assignment from file_a.lua takes priority over @field annotation
local v = ns.version
--    ^ hover: (local) v: number  def: local
