-- A list or map returned from another file carries its element type inline:
-- it binds `V[]` / `table<K, V>` generics and indexes like one of this file's.
---@diagnostic disable: unused-local, missing-parameter
local _, ns = ...

for _, item in ipairs(ns.GetAll()) do
    local name = item.name
    --    ^ hover: (local) name: string
end

local list = ns.GetAll()
local first = list[1]
--    ^ hover: (local) first: CrossItem
local firstName = list[1].name
--                        ^ hover: (field) name: string  comp: Describe, name
local count = #list
--    ^ hover: (local) count: number
local popped = tremove(list)
--    ^ hover: (local) popped: CrossItem
table.sort(list, function(a, b) return a.name < b.name end)
--                        ^ hover: (param) a: CrossItem

for key, value in pairs(ns.GetByName()) do
--  ^ hover: (local) key: string
    local valueName = value.name
    --    ^ hover: (local) valueName: string
end

-- A list the other file built with inserts.
for _, collected in ipairs(ns.Collect()) do
    --  ^ hover: (local) collected: CrossItem
end

-- An element read may find nothing, so a fallback isn't redundant.
local firstOrNew = list[1] or {}
local poppedOrNew = tremove(list) or {}
