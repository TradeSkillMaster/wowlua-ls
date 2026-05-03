-- Cross-file test: and-chaining on addon namespace fields
local _, ns = ...

-- Field assigned via `and` chain should infer the RHS type, not a union.
ns.GetSpellInfo = C_Spell and C_Spell.GetSpellInfo
