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
local dynResult = stash[getCategory()]
--       ^ hover: (local) dynResult: BItem[]?  def: local

-- Anonymous table with same-typed fields: deduplicates via types_equivalent
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
--    ^ hover: (local) item: table?  def: local
