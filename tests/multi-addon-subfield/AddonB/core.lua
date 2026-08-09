---@diagnostic disable: unused-local, unused-function
-- The sibling addon root. Its differently-typed `ns.Widget` and its `ns.db.bOnly`
-- sub-field must stay out of AddonA's namespace — see AddonA/core.lua.
local _, ns = ...

--- @class B_Widget
--- @field b string
local Widget = {}
ns.Widget = Widget

ns.db = {}
ns.db.bOnly = true
