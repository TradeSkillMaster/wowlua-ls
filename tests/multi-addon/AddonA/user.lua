---@diagnostic disable: unused-local
-- AddonA should see its own namespace fields
local _, ns = ...
local a = ns.addonAField
--    ^ hover: (local) a: string  def: local
local c = ns.addonACount
--    ^ hover: (local) c: number  def: local
-- AddonB's fields should NOT be visible (resolve to ?)
local b = ns.addonBField
--    ^ hover: (local) b: ?  def: local
local n = ns.addonBName
--    ^ hover: (local) n: ?  def: local
