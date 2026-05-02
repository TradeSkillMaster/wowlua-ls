-- Test: undefined-doc-class diagnostic
-- This diagnostic fires ONLY in `@class Foo: <parent>` inheritance position.
-- All other undefined type references (in @param, @return, @type, @field, @alias)
-- fire `undefined-doc-name` instead — see tests/undefined-doc-name.lua.

local function _consume(...) end

---@class KnownClass
---@field value number

-- ── @class parent references ─────────────────────────────────────────────

---@class ChildOfKnown : KnownClass
---@field extra string
-- ^ diag: none

---@class ChildOfUnknown : MissingParent
-- ^ diag: undefined-doc-class

-- ── Built-in types as @class parents should not trigger ─────────────────

---@class UserdataChild : userdata
-- ^ diag: none

---@class UnknownChild : unknown
-- ^ diag: none

-- ── Inline table type parent should not trigger ──────────────────────

---@class OrderedTableTest<K, V>: { [integer]: V, [K]: V }

---@type OrderedTableTest<string, number>
local _orderedTable = {}
_consume(_orderedTable)
-- ^ diag: none

-- ── Parameterized table<K,V> parent should not trigger ─────────────

---@class DictClass : table<string, number>
-- ^ diag: none

---@class TypoDict : tabel<string, number>
-- ^ diag: undefined-doc-class

-- ── Suppression ──────────────────────────────────────────────────────

---@diagnostic disable: undefined-doc-class
---@class SuppressedChild : MissingSuppressedParent
-- ^ diag: none
---@diagnostic enable: undefined-doc-class
