-- Regression: a value assigned from the `Lib("Name"):Method(...)` idiom recovers
-- the method's precise return type cross-file — WITHOUT the trailing method name
-- mis-resolving as a same-named WoW API global.
-- `LibStub("AceLocale-3.0"):GetLocale(name)` is an AceLocale *method* returning
-- the locale table (`table<string, string>`); the same-named global `GetLocale()`
-- returns the client-locale *string*. The coarse scan emits the resolvable chain
-- `[AceLocale-3.0, GetLocale]` (receiver string = class, then method) so the field
-- resolves to `table<string, string>` and never leaks `string`. Covers all three
-- scan sites that build a callee chain from the RHS: a namespace field, a bare
-- global, and two-step local forwarding.
---@diagnostic disable: unused-local, create-global, undefined-field
local name, ns = ...

-- (a) namespace field
ns.L = LibStub("AceLocale-3.0"):GetLocale(name)
-- ^ hover: (field) L: table<string, string>

-- (b) bare global — its deferred resolver recovers the precise per-file type.
GlobalLoc = LibStub("AceLocale-3.0"):GetLocale(name)
local rd = GlobalLoc
--    ^ hover: (local) rd: table<string, string>

-- (c) two-step forwarding: a local origin captured in `local_call_origins`, then
--     a field assignment from that local.
local loc = LibStub("AceLocale-3.0"):GetLocale(name)
ns.G = loc
-- ^ hover: (field) G: table<string, string>

-- (d) the method's RETURN wins, not the receiver/library class: AceGUI:Create
--     returns AceGUIWidget (a distinct class), so the field is AceGUIWidget — never
--     AceGUI-3.0 (the library named by the receiver string).
ns.widget = LibStub("AceGUI-3.0"):Create("Frame")
-- ^ hover: (field) widget: AceGUIWidget {

-- (e) the idiom is tightly shaped: a `.field`/`[k]` navigation between the getter
--     and the method (`Lib("X").field:M()`) is NOT `Lib("X"):M()` — the method runs
--     on `x.field`, not on the library. `.somefield` is undefined on AceLocale-3.0,
--     so the call is genuinely unresolvable: the field is `any` (honest unknown),
--     never mis-resolved to `GetLocale`'s `table<string, string>`.
ns.viafield = LibStub("AceLocale-3.0").somefield:GetLocale(name)
-- ^ hover: (field) viafield: any
