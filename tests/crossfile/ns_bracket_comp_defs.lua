-- Cross-file test: bracket assignment completions on addon namespace table<K,V>
local _, private = ...

---@class CCNPCData
---@field questID number
---@field npcID number
---@field classification string

---@type table<number, CCNPCData>
private.NPCs = {}

---@class CCZoneInfo
---@field mapID number
---@field name string

---@type table<number, CCZoneInfo>
private.Data.Zones = {}
