-- Cross-file `@class` field harvesting when the field is assigned in a file that does
-- NOT declare the class. `NDF_Owner`'s runtime fields are written by methods in
-- nondecl_field_methods.lua (a file with no `@class NDF_Owner`), so `self` there is the
-- *external* class table and the writes are only reachable by re-analyzing the assigning
-- file via the class's assigning-file index + external-table matching. This file only
-- declares the classes; it assigns none of `NDF_Owner`'s fields. See
-- nondecl_field_methods.lua / nondecl_field_user.lua.

local addonName, ns = ...

---@class NDF_Widget
local Widget = {}
ns.NDF_Widget = Widget
function Widget:Render() end

---@class NDF_Owner
local Owner = {}
ns.NDF_Owner = Owner
