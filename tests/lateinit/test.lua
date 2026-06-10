local function _consume(...) end

---@class LateinitDB
---@field Query fun(self: LateinitDB): string

---@class LateinitClause
---@field Build fun(self: LateinitClause): string

-- ============================================================================
-- @field with T! — lateinit field declarations
-- ============================================================================

---@class PooledQuery
---@field public _db LateinitDB!
---@field public _clause LateinitClause!
---@field public _name string
---@field public _optional string?

-- ── Hover shows T! for lateinit fields ─────────────────────────────────

---@type PooledQuery
local obj = {}
local _ = obj._db
--             ^ hover: (field) _db: LateinitDB!

-- ── No need-check-nil on lateinit field access ─────────────────────────

obj._db:Query()

obj._clause:Build()

-- ── No field-type-mismatch on nil assignment to lateinit field ─────────

obj._db = nil

obj._clause = nil

-- ── field-type-mismatch still fires for wrong non-nil type on lateinit ─

obj._db = 42
-- ^ diag: field-type-mismatch

-- ── Non-lateinit field still works normally ─────────────────────────────

local _ = obj._name
--             ^ hover: (field) _name: string

-- ── Optional field still warns on access ────────────────────────────────

obj._optional:upper()
--  ^ diag: need-check-nil

-- ============================================================================
-- Inline ---@type T! on field init
-- ============================================================================

---@class InlinePooled
local InlinePooled = {}
InlinePooled.data = nil ---@type string!
InlinePooled.count = nil ---@type number!

-- ── Hover shows T! for inline lateinit ──────────────────────────────────

local _ = InlinePooled.data
--                     ^ hover: (field) data: string!

-- ── No need-check-nil on inline lateinit field ──────────────────────────

_consume(InlinePooled.data:upper())

-- ── No field-type-mismatch when nil assigned to inline lateinit ─────────

InlinePooled.data = nil

-- ── field-type-mismatch still fires for wrong type ──────────────────────

InlinePooled.count = "hello"
-- ^ diag: field-type-mismatch

-- ── No field-type-mismatch when T|nil assigned to lateinit T! field ──

---@type LateinitDB?
local maybeDb = nil
obj._db = maybeDb

-- ============================================================================
-- Inline ---@type T! inside table constructor (regression: was false positive)
-- ============================================================================

local tbl = {
    result = nil, ---@type boolean!
    name = "test",
}

local _ = tbl.result
--            ^ hover: (field) result: boolean!

tbl.result = nil

tbl.result = true

tbl.result = "wrong"
-- ^ diag: field-type-mismatch

-- ============================================================================
-- No missing-fields for lateinit fields in constructors
-- ============================================================================

---@type PooledQuery
local pq = { _name = "test" } ---@diagnostic disable-line: unused-local

-- ── assign-type-mismatch path: table literal as function arg ────────────

---@param q PooledQuery
local function useQuery(q) _consume(q) end

useQuery({ _name = "test" })
