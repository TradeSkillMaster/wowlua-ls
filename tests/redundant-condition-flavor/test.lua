-- Test: redundant-condition suppressed for flavor-restricted globals.
-- Project targets retail + classic, so globals restricted to one flavor may
-- be nil at runtime.
---@diagnostic disable: unused-local, unused-function, empty-block, wrong-flavor-api

-- AbbreviateLargeNumbers is retail-only. In a classic build it won't exist,
-- so nil-checking it is valid — not a redundant condition.
if AbbreviateLargeNumbers then end

if not AbbreviateLargeNumbers then end

-- Equality comparison with nil (goes through eval_equality path).
if AbbreviateLargeNumbers == nil then end

if AbbreviateLargeNumbers ~= nil then end

-- type() guard (goes through eval_type_guard path).
if type(AbbreviateLargeNumbers) == "function" then end

-- CreateFrame is available in all flavors, so nil-checking it IS redundant.
if CreateFrame then end
-- ^ diag: redundant-condition

if not CreateFrame then end
-- ^ diag: redundant-condition
