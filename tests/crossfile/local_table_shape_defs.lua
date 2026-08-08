local _, ns = ... ---@class LocalTableShapeNS

-- Regression: a plain local data table (`local constants = {}`) built up via
-- scattered `constants.X = ...` writes (here inside a `do ... end`) and assigned to
-- a @class field (`data.constants = constants`) must carry its accumulated shape
-- cross-file, not degrade to a bare `table`. Enum-style values referencing local
-- scalar constants (`MONK`, `TIER_LFR`) resolve to their scalar kind.

---@class LocalTableShapeData
local data = {}
ns.shapeData = data

local MONK = 10
local WARLOCK = 9
local TIER_LFR = 1

local constants = {}
data.constants = constants
do
    constants.tiers = { lfr = TIER_LFR, heroic = 3 }
    constants.classes = { MONK = MONK, WARLOCK = WARLOCK }
    constants.conquestItemModID = 159
    -- An array field initialized empty then filled via index writes must stay
    -- typed `table` — the element writes must NOT be mis-captured as scalar
    -- writes to the `items` field itself.
    constants.items = {}
    constants.items[1] = "foo"
    constants.items[2] = "bar"
end
