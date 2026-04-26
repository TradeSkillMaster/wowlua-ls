-- Cross-file test: uses @type to reference class defined on addon namespace.
-- Fields assigned to addon namespace in ns_class_defs.lua should be visible
-- through the @class table when accessed via @type annotation.
---@type AddonAPI
local ns = select(2, ...)
local ver = ns:GetVersion()
--    ^ hover: (global) ver: number  def: local
ns.config:Reset()
--          ^ hover: (method) function Reset()  def: external
local dbg = ns.config.debug
--    ^ hover: (global) dbg: boolean  def: local
-- @field annotation takes precedence: declared as string, runtime is number
local v = ns.version
--    ^ hover: (global) v: string  def: local
