---@diagnostic disable: unused-local
-- AddonV: mirror of AddonU with a differently-typed same-named `Util` field.
---@class V_NS
local _, ns = ...
---@class V_Util
local Util = {}
ns.Util = Util
-- ^ hover: (field) Util: V_Util
function Util:VMethod() end
