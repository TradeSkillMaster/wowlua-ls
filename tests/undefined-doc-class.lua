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

-- ── Primitive types as @class parents ────────────────────────────────

---@class PrimNum : number
-- ^ diag: invalid-class-parent

---@class PrimStr : string
-- ^ diag: invalid-class-parent

---@class PrimBool : boolean
-- ^ diag: invalid-class-parent

---@class PrimNil : nil
-- ^ diag: invalid-class-parent

---@class PrimFun : function
-- ^ diag: invalid-class-parent

---@class PrimTrue : true
-- ^ diag: invalid-class-parent

---@class PrimFalse : false
-- ^ diag: invalid-class-parent

---@class PrimThread : thread
-- ^ diag: invalid-class-parent

---@class PrimInt : integer
-- ^ diag: invalid-class-parent

---@class PrimBool2 : bool
-- ^ diag: invalid-class-parent

---@class PrimFun2 : fun
-- ^ diag: invalid-class-parent

-- ── String literal, number literal, union, and fun() parents ────────

---@class LitStr : "foo"
-- ^ diag: invalid-class-parent

---@class LitNum : 42
-- ^ diag: invalid-class-parent

---@class LitUnion : 1 | 2 | 3
-- ^ diag: invalid-class-parent

---@class FunSig : fun(x: number): string
-- ^ diag: invalid-class-parent

-- ── Aliases resolving to primitive types ─────────────────────────────

---@alias MyNumber number
---@class AliasNum : MyNumber
-- ^ diag: invalid-class-parent

---@alias MyStr string
---@class AliasStr : MyStr
-- ^ diag: invalid-class-parent

-- Alias to a class should be fine
---@alias MyClass KnownClass
---@class AliasClass : MyClass
-- ^ diag: none

-- ── Suppression ──────────────────────────────────────────────────────

---@diagnostic disable: undefined-doc-class
---@class SuppressedChild : MissingSuppressedParent
-- ^ diag: none
---@diagnostic enable: undefined-doc-class

---@diagnostic disable: invalid-class-parent
---@class SuppressedPrim : number
-- ^ diag: none
---@diagnostic enable: invalid-class-parent
