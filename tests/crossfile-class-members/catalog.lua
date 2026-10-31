-- A module's data tables: `[k] = v` maps typed by their entries when they agree,
-- a getter returning one of them, and a map declared with alias names.
---@diagnostic disable: unused-local
local Shelf = select(2, ...).Shelf
local Catalog = Shelf:Init("Catalog")
local DATA = {}

Catalog.Names = {
    [101] = "first",
    [102] = "second",
}

Catalog.Groups = {
    [201] = { "a", "b" },
    [202] = { "c", "d" },
}

Catalog.Hidden = {
    [301] = true,
    [302] = true,
}

-- Entries of different types leave a map untyped.
local MODE_A, MODE_B = "a", "b"
Catalog.Defaults = {
    [MODE_A] = false,
    [MODE_B] = 5,
}

function Catalog.Get(full)
    if full then
        return DATA.Full
    end
    return DATA.Empty
end

DATA.Empty = {}
DATA.Full = {
    ["x"] = { weight = 1 },
}

-- A union of an empty table and a class stays untyped in other files: there a
-- bare `table` would swallow the class.
local ENTRY_MT = { __index = {} }
function Catalog.NewEntry(name)
    if name == "" then
        return {}
    end
    ---@class CatalogEntry
    local entry = setmetatable({ name = name }, ENTRY_MT)
    return entry
end

---@alias CatalogSlot
---|'"left"'
---|'"right"'

---@alias CatalogKey
---|'"l"'
---|'"r"'

---@type table<CatalogSlot, CatalogKey>
Catalog.SLOTS = {
    left = "l",
    right = "r",
}

local names = Catalog.Names
--    ^ hover: (local) names: string[]
local defaults = Catalog.Defaults
--    ^ hover: (local) defaults: table
local slots = Catalog.SLOTS
--                    ^ hover: (field) SLOTS: table<CatalogSlot, CatalogKey>
