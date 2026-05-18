-- Cross-file test: access @enum field defined on vararg-destructured @class local
local name, VarargAddon = ...
---@class VarargAddonClass
VarargAddon = LibStub("AceAddon-3.0"):GetAddon(name)

local diff = VarargAddon.Difficulty
--    ^ hover: (local) diff: VarargDifficulty  def: local  diag: unused-local
