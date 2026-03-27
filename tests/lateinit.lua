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
-- ^ diag: none

obj._clause:Build()
-- ^ diag: none

-- ── No field-type-mismatch on nil assignment to lateinit field ─────────

obj._db = nil
-- ^ diag: none

obj._clause = nil
-- ^ diag: none

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
--                    ^ diag: none

-- ── No field-type-mismatch when nil assigned to inline lateinit ─────────

InlinePooled.data = nil
-- ^ diag: none

-- ── field-type-mismatch still fires for wrong type ──────────────────────

InlinePooled.count = "hello"
-- ^ diag: field-type-mismatch
