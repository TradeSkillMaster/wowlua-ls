---@diagnostic disable: unused-local
local _, ns = ... ---@class LocalTableShapeNS

local data = ns.shapeData
local constants = data.constants

-- Nested table fields of the separately-assigned local must resolve cross-file
-- (before the fix, `data.constants` was a bare `table` and these were undefined).
local _a = constants.classes.MONK
--                           ^ hover: (field) MONK: number
local _b = constants.tiers.lfr
--                         ^ hover: (field) lfr: number
-- A scalar field written directly on the local keeps its type
local _c = constants.conquestItemModID
--                   ^ hover: (field) conquestItemModID: number
-- An array field filled via index writes stays `table`: the element writes
-- (`constants.items[1] = "foo"`) must NOT be mis-captured as a scalar write to
-- the `items` field itself.
local _d = constants.items
--                   ^ hover: (field) items: table
