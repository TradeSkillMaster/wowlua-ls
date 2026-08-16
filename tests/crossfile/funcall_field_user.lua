---@diagnostic disable: unused-local
-- Reads `FNF_Owner`'s fields, which are assigned only via funcall self-fields in the
-- NON-declaring funcall_field_methods.lua. Each coarse placeholder field is upgraded on
-- read to the definition-site type harvested from the assigning file — reachable only
-- because that file is indexed via the `ws_globals` TableField pass. See
-- funcall_field_defs.lua / funcall_field_methods.lua.

local addonName, ns = ...

---@type FNF_Owner
local o = ns.FNF_Owner

-- Assigned via `self.widget = MakeWidget()` (a funcall self-field) in a non-declaring
-- file: coarse placeholder upgraded to its class.
local w = o.widget
--    ^ hover: (local) w: FNF_Widget
-- Because the field is precisely `FNF_Widget`, its real method resolves; a bogus one
-- would fire `undefined-field` (caught by the exhaustive harness).
o.widget:Render()

-- Assigned via `self.count = ComputeCount()` (arithmetic return): coarse placeholder
-- upgraded to `number`.
local c = o.count
--    ^ hover: (local) c: number
