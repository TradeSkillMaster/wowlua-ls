-- Test: redundant-condition `type()` guard cases (requires stubs for `type`)
---@diagnostic disable: unused-local, unused-function, empty-block

-- `type(n) == "number"` where n is a number → always true
---@type number
local n
if type(n) == "number" then end
--    ^ diag: redundant-condition

-- `type(n) == "string"` where n is a number → always false
---@type number
local n2
if type(n2) == "string" then end
--    ^ diag: redundant-condition

-- `type(n) ~= "number"` where n is a number → always false
---@type number
local n3
if type(n3) ~= "number" then end
--    ^ diag: redundant-condition

-- Reversed operand order: `"number" == type(s)` where s is a string → false
---@type string
local s
if "number" == type(s) then end
--    ^ diag: redundant-condition

-- Mixed union → NOT flagged (could be either type)
---@type string|number
local u
if type(u) == "string" then end

-- Non-type-name literal: `type()` can only return one of the 8 Lua type names,
-- so comparing against "widget" is always false (resolved via the disjoint-type
-- path against the `type` stub's literal-union return).
---@type number
local n4
if type(n4) == "widget" then end
--    ^ diag: redundant-condition
