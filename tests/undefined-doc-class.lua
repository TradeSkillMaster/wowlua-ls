-- Test: undefined-doc-class diagnostic
local function _consume(...) end

---@class KnownClass
---@field value number

---@alias KnownAlias string | number

-- ── @class parent references ─────────────────────────────────────────────

---@class ChildOfKnown : KnownClass
---@field extra string
-- ^ diag: none

---@class ChildOfUnknown : MissingParent
-- ^ diag: undefined-doc-class

-- ── @field type references ───────────────────────────────────────────────

---@class FieldTestClass
---@field good KnownClass
-- ^ diag: none
---@field bad MissingFieldType
-- ^ diag: undefined-doc-class
---@field alias_ref KnownAlias
-- ^ diag: none

-- ── @alias type references ───────────────────────────────────────────────

---@alias GoodAlias KnownClass
-- ^ diag: none

---@alias BadAlias MissingAliasTarget
-- ^ diag: undefined-doc-class

-- ── @param type references ───────────────────────────────────────────────

---@param x KnownClass
---@param y MissingParamType
local function _paramTest(x, y) _consume(x, y) end
-- ^ diag: undefined-doc-class

-- ── @return type references ──────────────────────────────────────────────

---@return MissingReturnType
local function _returnTest() end
-- ^ diag: undefined-doc-class

---@return number
local function _goodReturn() return 1 end
-- ^ diag: none

-- ── @type on variables ──────────────────────────────────────────────────

---@type MissingVarType
local _badVar = nil
-- ^ diag: undefined-doc-class

---@type KnownClass
local _goodVar = {}
-- ^ diag: none

-- ── Built-in types should not trigger ────────────────────────────────────

---@param a number
---@param b string
---@param c boolean
---@param d table
---@param e function
---@param f nil
---@param g any
local function _builtinTest(a, b, c, d, e, f, g) _consume(a, b, c, d, e, f, g) end
-- ^ diag: none

-- ── Union types ──────────────────────────────────────────────────────────

---@class UnionTestClass
---@field ok KnownClass | string
-- ^ diag: none
---@field mixed KnownClass | MissingInUnion
-- ^ diag: undefined-doc-class

-- ── Array types ──────────────────────────────────────────────────────────

---@class ArrayTestClass
---@field ok KnownClass[]
-- ^ diag: none
---@field bad MissingArrayElem[]
-- ^ diag: undefined-doc-class

-- ── Generic constraints ──────────────────────────────────────────────────

---@generic T : KnownClass
---@param x T
local function _goodGeneric(x) _consume(x) end
-- ^ diag: none

---@generic T : MissingConstraint
---@param x T
local function _badGeneric(x) _consume(x) end
-- ^ diag: undefined-doc-class

-- ── Generic type variables should not trigger ────────────────────────────

---@generic T
---@param x T
---@return T
local function _identity(x) return x end
-- ^ diag: none

-- ── Suppression ──────────────────────────────────────────────────────────

---@diagnostic disable: undefined-doc-class
---@type MissingSuppressed
local _suppressedVar = nil
-- ^ diag: none
---@diagnostic enable: undefined-doc-class
