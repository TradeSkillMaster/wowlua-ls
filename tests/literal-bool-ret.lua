-- Tests for literal boolean return type narrowing on union discriminators.
-- When a union type A | B has a method where A:Method() returns literal `false`
-- and B:Method() returns literal `true`, the LS narrows the union in branches.

---@class AuctionRow
---@field rowId number
---@field buyout number
local AuctionRow = {}

---@return false
function AuctionRow:IsSubRow() return false end

---@class AuctionSubRow
---@field rowId number
---@field parentRowId number
local AuctionSubRow = {}

---@return true
function AuctionSubRow:IsSubRow() return true end

-- ── Then-branch narrowing ──────────────────────────────────────────────────

---@param row AuctionRow | AuctionSubRow
local function test_then_branch(row)
    if row:IsSubRow() then
        local r = row
        --    ^ hover: (local) r: AuctionSubRow
    end
end

-- ── Else-branch narrowing ──────────────────────────────────────────────────

---@param row AuctionRow | AuctionSubRow
local function test_else_branch(row)
    if row:IsSubRow() then
        local r = row
        --    ^ hover: (local) r: AuctionSubRow
    else
        local r = row
        --    ^ hover: (local) r: AuctionRow
    end
end

-- ── Early-exit narrowing ───────────────────────────────────────────────────

---@param row AuctionRow | AuctionSubRow
local function test_early_exit(row)
    if not row:IsSubRow() then return end
    local r = row
    --    ^ hover: (local) r: AuctionSubRow
end

-- ── assert() narrowing ─────────────────────────────────────────────────────

---@param row AuctionRow | AuctionSubRow
local function test_assert(row)
    assert(row:IsSubRow())
    local r = row
    --    ^ hover: (local) r: AuctionSubRow
end

-- ── assert(x and x:Method()) with nil union ────────────────────────────────

---@param row (AuctionRow | AuctionSubRow)?
local function test_assert_compound(row)
    assert(row and row:IsSubRow())
    local r = row
    --    ^ hover: (local) r: AuctionSubRow
end

-- ── Shared field accessible without narrowing ──────────────────────────────

---@param row AuctionRow | AuctionSubRow
local function test_shared_field(row)
    local id = row.rowId
    --             ^ hover: (field) rowId: number
end

-- ── 3-member union: two return true, one returns false ─────────────────────

---@class BaseItem
---@field name string
local BaseItem = {}

---@return false
function BaseItem:IsEnhanced() return false end

---@class MagicItem
---@field enchantLevel number
local MagicItem = {}

---@return true
function MagicItem:IsEnhanced() return true end

---@class RareItem
---@field rarity string
local RareItem = {}

---@return true
function RareItem:IsEnhanced() return true end

---@param item BaseItem | MagicItem | RareItem
local function test_three_member_then(item)
    if item:IsEnhanced() then
        local x = item
        --    ^ hover: (local) x: MagicItem | RareItem
    else
        local y = item
        --    ^ hover: (local) y: BaseItem
    end
end

-- ── No narrowing when return is generic boolean ────────────────────────────

---@class NodeA
---@field val number
local NodeA = {}

---@return boolean
function NodeA:IsLeaf() return false end

---@class NodeB
---@field data string
local NodeB = {}

---@return true
function NodeB:IsLeaf() return true end

-- Should NOT narrow: NodeA returns generic boolean, not literal false
---@param node NodeA | NodeB
local function test_no_narrow_generic_bool(node)
    if node:IsLeaf() then
        local v = node
        --    ^ hover: (local) v: NodeA | NodeB
    end
end

-- ── `not` inversion in else: `if not x:Method() then ... else ... end` ─────

---@param row AuctionRow | AuctionSubRow
local function test_not_inversion(row)
    if not row:IsSubRow() then
        local r = row
        --    ^ hover: (local) r: AuctionRow
    else
        local r = row
        --    ^ hover: (local) r: AuctionSubRow
    end
end

-- ── Method missing on one union member: no narrowing, no crash ─────────────

---@class HasCheck
---@field extra number
local HasCheck = {}

---@return true
function HasCheck:IsValid() return true end

---@class MissingCheck
---@field label string

-- MissingCheck does NOT define IsValid — should not narrow

---@param obj HasCheck | MissingCheck
local function test_missing_method(obj)
    if obj:IsValid() then
        local v = obj
        --    ^ hover: (local) v: HasCheck | MissingCheck
    end
end
