---@diagnostic disable: unused-local
-- A pure @class/@field addon (no runtime writes, so no globals) must still get
-- an isolated ns table instead of falling back to the polluted combined table.
local _, ns = ...
local c = ns.configOnly
--    ^ hover: (local) c: boolean  def: local
local l = ns.labelOnly
--    ^ hover: (local) l: string  def: local
-- AddonF's runtime field must NOT leak in.
local f = ns.fRuntime
--    ^ hover: (local) f: ?  def: local
