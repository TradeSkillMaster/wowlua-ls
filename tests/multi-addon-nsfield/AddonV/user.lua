---@diagnostic disable: unused-local
---@class V_NS
local _, ns = ...
-- Regression (opposite direction): `ns.Util` here must resolve to `V_Util`,
-- not AddonU's `U_Util`.
local util = ns.Util
--    ^ hover: (local) util: V_Util  def: local
util:VMethod()
