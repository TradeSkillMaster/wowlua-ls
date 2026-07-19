-- Regression: a value assigned from a chained-receiver call whose method name
-- collides with a WoW API global must NOT inherit the global's return type.
-- `LibStub("AceLocale-3.0"):GetLocale(name)` is an AceLocale *method* (returns
-- the locale table); the same-named global `GetLocale()` returns the
-- client-locale *string*. The cross-file coarse scan drops the chained receiver,
-- so the value never leaks `string` (previously `string | table<string,
-- string>`). Covers all three scan sites that build a callee chain from the RHS:
-- a namespace field, a bare global, and two-step local forwarding.
---@diagnostic disable: unused-local, create-global
local name, ns = ...

-- (a) namespace field
ns.L = LibStub("AceLocale-3.0"):GetLocale(name)
-- ^ hover: (field) L: table

-- (b) bare global — its deferred resolver recovers the precise per-file type
--     once the bogus same-named-global chain is gone.
GlobalLoc = LibStub("AceLocale-3.0"):GetLocale(name)
local rd = GlobalLoc
--    ^ hover: (local) rd: table<string, string>

-- (c) two-step forwarding: a local origin captured in `local_call_origins`, then
--     a field assignment from that local.
local loc = LibStub("AceLocale-3.0"):GetLocale(name)
ns.G = loc
-- ^ hover: (field) G: table
