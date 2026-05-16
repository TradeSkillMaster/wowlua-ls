---@class BItem
---@field name string

---@class BStash
---@field gems BItem[]
---@field ores BItem[]
---@field herbs BItem[]

---@type BStash
local stash = {}

---@return "gems" | "ores" | "herbs"
local function getCategory()
    return "gems"
end

-- Literal key: resolves to annotated field type
local litResult = stash["gems"]
--       ^ hover: (local) litResult: BItem[]  def: local

-- Dynamic key with string literal union: resolves via key-aware field lookup
-- All literals match defined fields, so result is non-nil.
local dynResult = stash[getCategory()]
--       ^ hover: (local) dynResult: BItem[]  def: local

-- Anonymous table with same-typed fields: all keys match, no nil
local bag = {
    slot1 = {},
    slot2 = {},
    slot3 = {},
}
---@return "slot1" | "slot2" | "slot3"
local function getSlot()
    return "slot1"
end
local item = bag[getSlot()]
--    ^ hover: (local) item: table  def: local

-- Key union where some literals don't match a field: still non-nil since
-- a string literal union key implies the table is designed for those keys.
---@return "gems" | "ores" | "herbs" | "unknown"
local function getMaybeCategory()
    return "gems"
end
local maybeResult = stash[getMaybeCategory()]
--       ^ hover: (local) maybeResult: BItem[]  def: local
