---@diagnostic disable: unused-local
-- AddonB should see its own namespace fields
local _, ns = ...
local b = ns.addonBField
--    ^ hover: (local) b: boolean  def: local
local n = ns.addonBName
--    ^ hover: (local) n: string  def: local
-- AddonA's fields should NOT be visible (resolve to ?)
local a = ns.addonAField
--    ^ hover: (local) a: ?  def: local
local c = ns.addonACount
--    ^ hover: (local) c: ?  def: local
