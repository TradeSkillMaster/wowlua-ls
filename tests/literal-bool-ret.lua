---@diagnostic disable: undefined-global, unused-function, unused-local
-- Tests for literal boolean return type narrowing on union discriminators.
-- When a union type A | B has a method where A:Method() returns literal `false`
-- and B:Method() returns literal `true`, the LS narrows the union in branches.
-- Also covers direct `x == false` / `x == true` equality narrowing.

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
    end
end

-- Early-exit: `if not self._state.selectedRow:IsSubRow() then return end`
-- After the guard, the field should be narrowed to AuctionSubRow.
function BoolRetContainer:test_field_chain_early_exit()
    if not self._state.selectedRow then return end
    if not self._state.selectedRow:IsSubRow() then return end
    expectSubRow(self._state.selectedRow)
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

-- ── `x == false` / `x == true` equality narrowing ──────────────────────────
-- The `false | T` failure-return idiom: comparing against a boolean literal
-- narrows the union the same way a string literal comparison does. `boolean`
-- has exactly two inhabitants, so the tested literal is exact on both sides.

---@return false|number
local function parseId() return 1 end

---@return true|string
local function loadName() return "n" end

---@param n number
local function takesNumber(n) end

local function test_bool_eq_branches()
    local id = parseId()
    if id == false then
        local f = id
        --    ^ hover: (local) f: false
    else
        local n = id
        --    ^ hover: (local) n: number
    end
end

local function test_bool_neq_branches()
    local id = parseId()
    if id ~= false then
        local n = id
        --    ^ hover: (local) n: number
    else
        local f = id
        --    ^ hover: (local) f: false
    end
end

local function test_true_literal_branches()
    local name = loadName()
    if name == true then
        local t = name
        --    ^ hover: (local) t: true
    else
        local s = name
        --    ^ hover: (local) s: string
    end
end

-- Early exit, both operand orders (`false == x` reads as a Yoda comparison).
local function test_bool_early_exit()
    local id = parseId()
    if id == false then return end
    local n = id
    --    ^ hover: (local) n: number
end

local function test_bool_early_exit_reversed()
    local id = parseId()
    if false == id then return end
    local n = id
    --    ^ hover: (local) n: number
end

-- The strip only applies past the guard: a read *before* it still sees `false`.
local function test_bool_early_exit_not_retroactive()
    local id = parseId()
    takesNumber(id)
    --          ^ diag: type-mismatch
    if id == false then return end
    takesNumber(id)
end

---@param b false
local function takesFalse(b) end

-- Same for the `~=` direction, which filters to the literal past the guard.
local function test_bool_neq_early_exit_not_retroactive()
    local id = parseId()
    takesFalse(id)
    --         ^ diag: type-mismatch
    if id ~= false then return end
    takesFalse(id)
end

-- A plain `boolean` narrows to the tested literal in the then-branch. The strip
-- leaves the else-branch alone: `boolean` is not the literal, so nothing is
-- removed (see `extract_literal_eq_sides`).
---@param flag boolean
local function test_plain_boolean(flag)
    if flag == false then
        local f = flag
        --    ^ hover: (local) f: false
    else
        local t = flag
        --    ^ hover: (local) t: boolean
    end
end

-- The exiting branch may read the guarded symbol (log the failure value, then
-- return). That in-branch read pushes a narrowed version, which must stay inside
-- the branch instead of overriding the post-guard type.
---@param msg string
local function logMessage(msg) end

local function test_bool_early_exit_with_branch_read()
    local id = parseId()
    if id == false then
        logMessage("bad id: " .. tostring(id))
        return
    end
    local n = id
    --    ^ hover: (local) n: number
    takesNumber(id)
end

-- ── Field chains ───────────────────────────────────────────────────────────

---@class BoolEqHolder
---@field id false|number
local BoolEqHolder = {}

---@param h BoolEqHolder
local function test_field_bool_early_exit(h)
    takesNumber(h.id)
    --          ^ diag: type-mismatch
    if h.id == false then return end
    takesNumber(h.id)
end

-- The `~=` direction filters the field to the literal, and likewise only past
-- the guard.
---@param h BoolEqHolder
local function test_field_bool_neq_early_exit(h)
    takesFalse(h.id)
    --         ^ diag: type-mismatch
    if h.id ~= false then return end
    takesFalse(h.id)
end
