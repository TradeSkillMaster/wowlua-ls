-- Test: redundant-condition suppressed for flavor-restricted globals.
-- Project targets retail + classic, so globals restricted to one flavor may
-- be nil at runtime.
---@diagnostic disable: unused-local, unused-function, empty-block, wrong-flavor-api

-- PlayerGetTimerunningSeasonID is retail-only. In a classic build it won't exist,
-- so nil-checking it is valid — not a redundant condition.
if PlayerGetTimerunningSeasonID then end

if not PlayerGetTimerunningSeasonID then end

-- Equality comparison with nil (goes through eval_equality path).
if PlayerGetTimerunningSeasonID == nil then end

if PlayerGetTimerunningSeasonID ~= nil then end

-- type() guard (goes through eval_type_guard path).
if type(PlayerGetTimerunningSeasonID) == "function" then end

-- CreateFrame is available in all flavors, so nil-checking it IS redundant.
if CreateFrame then end
-- ^ diag: redundant-condition

if not CreateFrame then end
-- ^ diag: redundant-condition
