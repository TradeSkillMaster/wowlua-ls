---@diagnostic disable: unused-local
-- Accesses namespace fields via bare access (no @type annotation).
-- @field annotations from the @class should be visible cross-file.
local _, ns = ...
local n = ns.name
--    ^ hover: (local) n: string  def: local
local c = ns.count
--    ^ hover: (local) c: number  def: local
local r = ns.runtime
--    ^ hover: (local) r: string  def: local
