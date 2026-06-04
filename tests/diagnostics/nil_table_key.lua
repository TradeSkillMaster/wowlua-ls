-- Test: nil-table-key diagnostic

-- ── @class parents ──────────────────────────────────────────────────────────

---@class NilKeyClass : table<string?, number>
-- ^ diag: nil-table-key

---@class GoodKeyClass : table<string, number>

---@class NilKeyUnion : table<string|nil, number>
-- ^ diag: nil-table-key

---@class BareNilKey : table<nil, number>
-- ^ diag: nil-table-key

-- ── @field types ────────────────────────────────────────────────────────────

---@class FieldTest
---@field items table<string?, number>
-- ^ diag: nil-table-key
---@field good table<string, number>
local _fieldTest

-- ── @alias ──────────────────────────────────────────────────────────────────

---@alias NilKeyMap table<string?, number>
-- ^ diag: nil-table-key

---@alias GoodMap table<string, number>

-- ── @type on variables ──────────────────────────────────────────────────────

---@type table<nil, number>
local _nilKeyVar
-- ^ diag: nil-table-key

---@type table<string|nil, number>
local _nilUnionKeyVar
-- ^ diag: nil-table-key

---@type table<string, number>
local _goodKeyVar

-- Value type nil is fine
---@type table<string, number?>
local _nilValueVar

-- ── @param ──────────────────────────────────────────────────────────────────

---@param t table<nil, string>
local function _paramNilKey(t) return t end
-- ^ diag: nil-table-key

---@param t table<string, number>
local function _paramGoodKey(t) return t end

-- ── @return ─────────────────────────────────────────────────────────────────

---@return table<string?, number>
local function _returnNilKey() return {} end
-- ^ diag: nil-table-key

---@return table<string, number>
local function _returnGoodKey() return {} end

-- ── Nested table types ──────────────────────────────────────────────────────

---@type table<string, table<nil, number>>
local _nestedNilKey
-- ^ diag: nil-table-key

---@type table<string, table<string, number>>
local _nestedGoodKey

-- ── @overload ───────────────────────────────────────────────────────────────

---@overload fun(t: table<nil, string>): boolean
local function _overloadNilKey() end
-- ^ diag: nil-table-key

---@overload fun(t: table<string, number>): boolean
local function _overloadGoodKey() end

-- ── @diagnostic suppression ─────────────────────────────────────────────────

---@diagnostic disable-next-line: nil-table-key
---@type table<nil, number>
local _suppressedVar
