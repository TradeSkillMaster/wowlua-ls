-- `[k] = v` maps written to a field other files read: one whose entries are of
-- different kinds reads `table` in its own file as in other files, one of records
-- of differing shapes their union. A class field written an empty table and an
-- instance keeps the union here.
---@diagnostic disable: unused-local, create-global
local _, ns = ...

-- Entries whose literals already differ.
ns.Mixed = { "a", [5] = 5 }
local mixed = ns.Mixed
--    ^ hover: (local) mixed: table
local mixedFirst = ns.Mixed[1]
--    ^ hover: (local) mixedFirst: any

-- Records of differing shapes are entries of one kind: the map keeps their union.
ns.Shapes = {
    [1] = { name = "a" },
    [2] = { name = "b", size = 2 },
}
local shapes = ns.Shapes
--    ^ hover: (local) shapes: ({name: string} | {name: string, size: number})[]

-- A map in a field of a table only this file sees keeps the union.
local Local = {}
Local.Mixed = { [1] = "a", [2] = 5 }
local localFirst = Local.Mixed[1]
--    ^ hover: (local) localFirst: string | number

-- A map in a field of a global table this file defines.
ShelfData = {}
ShelfData.Mixed = { [101] = "first", [102] = 5 }
local shelf = ShelfData.Mixed
--    ^ hover: (local) shelf: table

-- Entries that agree until a later write of another type, through a local the
-- field holds: on a class, from a method, and on the namespace.
---@class ShelfHolder
local Holder = {}
local held = { "a", [5] = "b" }
held[6] = 5
Holder.items = held
local items = Holder.items
--    ^ hover: (local) items: table

function Holder:Load()
    local loaded = { "a", [5] = "b" }
    loaded[6] = 5
    self.loaded = loaded
end

local queued = { "a", [5] = "b" }
queued[6] = 5
ns.Queue = queued

---@class ShelfItem
local ShelfItem = {}

---@class ShelfCache
local ShelfCache = {}
ns.ShelfCache = ShelfCache

---@type ShelfItem
local item

function ShelfCache:Reset()
    local empty = {}
    self.entry = empty
end

function ShelfCache:Fill()
    self.entry = item
end

local entry = ShelfCache.entry
--    ^ hover: (local) entry: table | ShelfItem
