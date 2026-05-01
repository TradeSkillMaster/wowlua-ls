-- Cross-file overlay test: uses fields defined in overlay_defs.lua
---@type MyAddon
local addon = MyAddon
local v = addon.version
--    ^ hover: (local) v: number  def: local
local n = addon.name
--    ^ hover: (local) n: string  def: local
addon:Run()
--    ^ hover: (method) function MyAddon:Run()  def: external

-- Function-call-result field should not produce undefined-field
local _w = addon.widget
--               ^ diag: none  def: external

-- Method defined on @type-annotated local (overlay_ext.lua) should be visible
addon:ExtraMethod()
--    ^ hover: (method) function MyAddon:ExtraMethod()  def: external
local _ef = addon.extraField
--                ^ diag: none  def: external

local c = GLOBAL_REGISTRY.count
--    ^ hover: (local) c: number  def: local
local l = GLOBAL_REGISTRY.label
--    ^ hover: (local) l: string  def: local
GLOBAL_REGISTRY:Reset()
--              ^ hover: (method) function Reset()  def: external
