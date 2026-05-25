---@diagnostic disable: undefined-global
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

-- ── Field-chain boolean discrimination ───────────────────────────────
-- When a field chain ends in a union type and the method call discriminates,
-- the field chain should be narrowed in the then-branch.

---@class BoolRetState
---@field selectedRow AuctionRow | AuctionSubRow
local BoolRetState = {}

---@class BoolRetContainer
---@field _state BoolRetState
local BoolRetContainer = {}

---@param subRow AuctionSubRow
local function expectSubRow(subRow) end

function BoolRetContainer:test_field_chain_discrimination()
    if self._state.selectedRow and self._state.selectedRow:IsSubRow() then
        expectSubRow(self._state.selectedRow)
        -- ^ diag: none
    end
end

-- Early-exit: `if not self._state.selectedRow:IsSubRow() then return end`
-- After the guard, the field should be narrowed to AuctionSubRow.
function BoolRetContainer:test_field_chain_early_exit()
    if not self._state.selectedRow then return end
    if not self._state.selectedRow:IsSubRow() then return end
    expectSubRow(self._state.selectedRow)
    -- ^ diag: none
end

-- ── Assert narrowing on field-access-derived union ──────────────────────
-- When a local is assigned from a field access (e.g. state.selectedAuction),
-- the union type from the @field annotation should still support
-- literal boolean discrimination via assert().

---@class BoolRetHolder
---@field selectedRow AuctionRow | AuctionSubRow
local BoolRetHolder = {}

---@param holder BoolRetHolder
local function test_assert_field_access(holder)
    local row = holder.selectedRow
    assert(row:IsSubRow())
    local r = row
    --    ^ hover: (local) r: AuctionSubRow
end

-- Same pattern with if-then narrowing on a field-access-derived local
---@param holder BoolRetHolder
local function test_if_field_access(holder)
    local row = holder.selectedRow
    if row:IsSubRow() then
        local r = row
        --    ^ hover: (local) r: AuctionSubRow
    else
        local r = row
        --    ^ hover: (local) r: AuctionRow
    end
end

-- Early-exit on a field-access-derived local
---@param holder BoolRetHolder
local function test_early_exit_field_access(holder)
    local row = holder.selectedRow
    if not row:IsSubRow() then return end
    local r = row
    --    ^ hover: (local) r: AuctionSubRow
end
