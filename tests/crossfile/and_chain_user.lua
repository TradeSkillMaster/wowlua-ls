---@diagnostic disable: unused-local
-- Cross-file test: and-chaining on addon namespace fields (usage)
local _, ns = ...

-- `ns.GetSpellInfo` should be the function type from `C_Spell.GetSpellInfo`,
-- not a union of `table | fun()`.
local info = ns.GetSpellInfo(123)
--    ^ hover: (local) info: SpellInfo  def: local
ns.GetSpellInfo(123)
-- ^ hover: (field) function GetSpellInfo(spellIdentifier: SpellIdentifier)\n  -> spellInfo: SpellInfo
