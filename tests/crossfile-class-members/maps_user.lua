-- `maps.lua`'s maps and class field read from another file.
---@diagnostic disable: unused-local
local _, ns = ...

local mixed = ns.Mixed
--    ^ hover: (local) mixed: table
local shapes = ns.Shapes
--    ^ hover: (local) shapes: ({ name: string } | { name: string, size: number })[]
local shelf = ShelfData.Mixed
--    ^ hover: (local) shelf: table
local queue = ns.Queue
--    ^ hover: (local) queue: table
---@type ShelfHolder
local holder
local items = holder.items
--    ^ hover: (local) items: table
local loaded = holder.loaded
--    ^ hover: (local) loaded: table

-- `table | ShelfItem` crosses files as in its file: the empty table as a table
-- of no fields, which doesn't swallow the class.
---@type ShelfCache
local cache
local entry = cache.entry
--    ^ hover: (local) entry: table | ShelfItem
