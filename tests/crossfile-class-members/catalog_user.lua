-- `catalog.lua`'s module read from another file: each member has the type it has
-- where it is defined.
---@diagnostic disable: unused-local
local Shelf = select(2, ...).Shelf
local Catalog = Shelf:Include("Catalog")

local names = Catalog.Names
--    ^ hover: (local) names: string[]
local name = Catalog.Names[101]
--    ^ hover: (local) name: string
local groups = Catalog.Groups
--    ^ hover: (local) groups: string[][]
local hidden = Catalog.Hidden
--    ^ hover: (local) hidden: boolean[]
local defaults = Catalog.Defaults
--    ^ hover: (local) defaults: table
local data = Catalog.Get(true)
--    ^ hover: (local) data: table<string, { weight: number }> | table
local entry = Catalog.NewEntry("x")
--    ^ hover: (local) entry: any
local slots = Catalog.SLOTS
--                    ^ hover: (field) SLOTS: table<CatalogSlot, CatalogKey>
local left = Catalog.SLOTS.left
--    ^ hover: (local) left: "l" | "r"
