---@diagnostic disable: unused-local
---@class U_NS
local _, ns = ...
-- Regression: `ns.Util` must resolve to THIS addon's `U_Util`, not AddonV's
-- same-named `V_Util`. A leak here previously typed it `V_Util`, so the
-- following `:UMethod()` call read as `undefined-field`.
local util = ns.Util
--    ^ hover: (local) util: U_Util  def: local
util:UMethod()
-- A namespace method with a `@return` must keep its function field type — the
-- per-addon isolation pass must NOT re-type the field to its return type
-- (`U_Thing`), which `Method`-kind globals carry in `returns` rather than as the
-- field type.
local getThing = ns.GetThing
--    ^ hover: (local) function getThing(self)
-- Calling it still yields the declared return type.
local thing = ns:GetThing()
--    ^ hover: (local) thing: U_Thing  def: local
