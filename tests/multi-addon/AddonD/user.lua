---@diagnostic disable: unused-local
-- AddonD should see its own namespace fields (runtime + @field)
---@class AddonDNS
local _, ns = ...
local p = ns.pluginName
--    ^ hover: (local) p: string  def: local
local v = ns.pluginVersion
--    ^ hover: (local) v: number  def: local
local d = ns.debugEnabled
--    ^ hover: (local) d: boolean  def: local
-- AddonC's @class fields should NOT be visible
local x = ns.configMode
--    ^ hover: (local) x: ?  def: local
local y = ns.coreData
--    ^ hover: (local) y: ?  def: local
