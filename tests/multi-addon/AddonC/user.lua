---@diagnostic disable: unused-local
-- AddonC should see its own namespace fields (runtime + @field)
---@class AddonCNS
local _, ns = ...
local d = ns.coreData
--    ^ hover: (local) d: string  def: local
local v = ns.coreVersion
--    ^ hover: (local) v: number  def: local
local m = ns.configMode
--    ^ hover: (local) m: boolean  def: local
-- AddonD's @class fields should NOT be visible
local x = ns.debugEnabled
--    ^ hover: (local) x: ?  def: local
local y = ns.pluginName
--    ^ hover: (local) y: ?  def: local
