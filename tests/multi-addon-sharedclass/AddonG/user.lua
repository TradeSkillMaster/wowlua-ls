---@diagnostic disable: unused-local
-- AddonG and AddonH both name their ns @class "SharedNS", so it is one shared
-- (partial) class: each sees its own AND the co-claimant's field. The strip must
-- run once against the union of claimants (not once per root, which would erase
-- fields), while a third unrelated addon's field is still stripped as foreign.
local _, ns = ...
local g = ns.gField
--    ^ hover: (local) g: number  def: local
local h = ns.hField
--    ^ hover: (local) h: string  def: local
local i = ns.iField
--    ^ hover: (local) i: ?  def: local
