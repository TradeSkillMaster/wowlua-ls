-- Cross-file `@class` field harvesting when the placeholder field is a *funcall*
-- self-field (`self.x = SomeCall()`) written in a file that does NOT declare the class.
-- Unlike a typed/bare self-field (recorded per field in `field_paths`), a funcall
-- self-field becomes an `ExternalGlobalKind::TableField` *global* — so its assigning file
-- reaches the harvest only because `build_on_stubs::finish` also indexes every
-- `ws_globals` TableField writer of a workspace class. This file only declares the
-- classes; `FNF_Owner`'s fields are assigned in funcall_field_methods.lua. See
-- funcall_field_methods.lua / funcall_field_user.lua.

local addonName, ns = ...

---@class FNF_Widget
local Widget = {}
ns.FNF_Widget = Widget
function Widget:Render() end

---@class FNF_Owner
local Owner = {}
ns.FNF_Owner = Owner
