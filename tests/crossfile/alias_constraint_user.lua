---@diagnostic disable: unused-local
-- Consumes the constrained alias ACWrap<T: ACAnimal> defined in
-- alias_constraint_defs.lua. The bound must be enforced cross-file.

-- Should NOT warn: ACDog is a subclass of ACAnimal.
---@type ACWrap<ACDog>
local acGood

-- Should WARN: ACRock is unrelated to ACAnimal.
---@type ACWrap<ACRock>
local acBad
-- ^ diag: generic-constraint-mismatch
