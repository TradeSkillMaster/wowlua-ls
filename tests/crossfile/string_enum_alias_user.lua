-- Cross-file: verifies that EnumType | StringAlias is assignable to EnumType
-- when all StringAlias members are valid enum member values.

-- Union of the enum type and an alias whose values are all enum members: no mismatch
---@type CrossFileColor | CrossFilePrimary
local colorOrPrimary
TakeCrossFileColor(colorOrPrimary)
--                 ^ hover: (local) colorOrPrimary: CrossFileColor | "red" | "green"

-- String literal that is a valid enum member value: no mismatch (structural subtype)
---@type "red"
local redStr
TakeCrossFileColor(redStr)

-- String literal that is NOT a declared enum value: the LS intentionally accepts it
-- (member-value checking for string enums is a known limitation of String(_) broadening).
---@type "not_a_member"
local wrongStr
TakeCrossFileColor(wrongStr)
