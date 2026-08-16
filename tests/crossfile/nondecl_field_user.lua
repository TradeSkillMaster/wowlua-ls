---@diagnostic disable: unused-local
-- Reads `NDF_Owner`'s fields, which are assigned only in the NON-declaring
-- nondecl_field_methods.lua. Each coarse `any` field is upgraded on read to the
-- definition-site type harvested from the assigning file. See nondecl_field_defs.lua /
-- nondecl_field_methods.lua.

local addonName, ns = ...

---@type NDF_Owner
local o = ns.NDF_Owner

-- Assigned via `self.widget = <NDF_Widget upvalue>` in a non-declaring file: coarse
-- `any` upgraded to its class (RHS-aware harvest through the external class table).
local w = o.widget
--    ^ hover: (local) w: NDF_Widget
-- Because the field is precisely `NDF_Widget`, its real method resolves; a bogus one
-- would fire `undefined-field` (caught by the exhaustive harness).
o.widget:Render()

-- Arithmetic RHS: coarse `any` upgraded to `number`.
local c = o.count
--    ^ hover: (local) c: number

-- Assigned `NDF_Widget` in one method and cleared to nil in another (both in the
-- assigning file): the union carries the field's nilability through the external path.
local p = o.opt
--    ^ hover: (local) p: NDF_Widget?

-- A co-located sibling class whose field is assigned in the same non-declaring method
-- file: harvested via whole-file warming when `NDF_Owner` is read.
---@type NDF_Helper
local h = ns.NDF_Helper
local t = h.tag
--    ^ hover: (local) t: number
