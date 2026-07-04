-- Open string-enum alias defined in the file itself: `@alias Name string` plus
-- `---|"literal"` continuation lines. The alias's resolved type stays bare
-- `string` (so any string is accepted — no type-mismatch), while the enumerated
-- values are preserved and offered as completions inside a string argument typed
-- with the alias.
--
-- This file intentionally does NOT disable type-mismatch, so exhaustive
-- diagnostic checking actually verifies the "any string is accepted" property
-- (a regression that turned the alias into a closed literal union would surface
-- an unasserted type-mismatch here and fail the test).

---@alias SEAUnit string
---|"player"
---|"target"
---|"focus"

---@param u SEAUnit
local function seaTake(u) end

-- Completion offers the enumerated values inside the string argument.
seaTake("")
--       ^ comp: player, target, focus

-- Any string is still accepted: a value that is not one of the enumerated tokens
-- must NOT produce type-mismatch (its absence is asserted by exhaustive checking).
seaTake("some_dynamic_unit")

-- ...but a genuinely wrong argument type still errors, proving the check is live.
seaTake(123)
--      ^ diag: type-mismatch

-- The alias resolves to a plain `string` for type/hover purposes, and a value
-- of the alias type is freely assignable back into an alias-typed parameter.
---@type SEAUnit
local seaVar
seaTake(seaVar)
--      ^ hover: (local) seaVar: string
