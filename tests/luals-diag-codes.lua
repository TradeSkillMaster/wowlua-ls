---@diagnostic disable: unused-local
-- LuaLS-only diagnostic codes (codes LuaLS defines but wowlua_ls does not) are
-- accepted in `---@diagnostic` directives without an `unknown-diag-code` warning,
-- so suppressions written for a project that also runs LuaLS don't generate noise.
--
-- Exhaustive diagnostic checking fails this test if any accepted code below is
-- wrongly flagged as unknown.

-- Single LuaLS-only codes — none should produce unknown-diag-code.
---@diagnostic disable-next-line: lowercase-global
local _a = 1
---@diagnostic disable-next-line: await-in-sync
local _b = 1
---@diagnostic disable-next-line: cast-type-mismatch
local _c = 1
---@diagnostic disable-next-line: unused-label
local _d = 1
---@diagnostic disable-next-line: global-element
local _d2 = 1

-- Several LuaLS-only codes in one directive are all accepted.
---@diagnostic disable-next-line: global-in-nil-env, spell-check, not-yieldable
local _e = 1

-- Our own codes and a LuaLS alias remain known (no unknown-diag-code).
---@diagnostic disable-next-line: undefined-global, param-type-mismatch
local _h = 1

-- A genuinely unknown code is still flagged.
---@diagnostic disable-next-line: totally-not-a-real-code
local _f = 1
-- ^ diag: unknown-diag-code

-- An unknown code mixed with an accepted LuaLS-only code: only the unknown one
-- fires (the LuaLS code is silently accepted).
---@diagnostic disable-next-line: lowercase-global, another-fake-code
local _g = 1
-- ^ diag: unknown-diag-code

-- A trailing (embedded) directive on an annotation comment line gets the same
-- code validation as a standalone directive — a typo'd code is still flagged.
---@type number ---@diagnostic disable-line: clas-shadows-builtin
local _i = 1
-- ^ diag: unknown-diag-code

-- A valid code in a trailing directive is accepted (nothing fires; exhaustive
-- checking would fail if a spurious diagnostic appeared here).
---@type number ---@diagnostic disable-line: undefined-global
local _j = 1

-- A trailing directive missing its ':' is flagged, just like a standalone one.
---@type number ---@diagnostic disable-line another-fake-code
local _k = 1
-- ^ diag: malformed-annotation
