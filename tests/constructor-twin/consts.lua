---@diagnostic disable: unused-local
-- The workspace scan registers `NS.CONST` as an anonymous sub-table whose call
-- and reference entries it can only type `any`. Reads in the file that writes
-- the constructor take those entries from the local constructor instead (the
-- scan table stays the receiver, so a literal entry still reads through it).
local localWidget = MakeThing()
NS.CONST = {
    A = MakeThing(),
    B = localWidget,
    C = 5,
    E = MakeThing() --[[@as Special]],
}
local t = NS.CONST
local a = NS.CONST.A
--    ^ hover: (local) a: Widget
local b = NS.CONST.B
--    ^ hover: (local) b: Widget
local c = NS.CONST.C
--    ^ hover: (local) c: number
local ta = t.A
--    ^ hover: (local) ta: Widget
local e = NS.CONST.E
--    ^ hover: (local) e: Special

_G.useConstructorTwin = { t, a, b, c, ta, e }
