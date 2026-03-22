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

-- ── userdata and thread should not trigger ────────────────────────────────

---@param ud userdata
---@param co thread
local function _userdataThreadTest(ud, co) _consume(ud, co) end
-- ^ diag: none

---@type userdata
local _udVar = nil
-- ^ diag: none

---@type thread
local _thVar = nil
-- ^ diag: none

-- ── Boolean literal types (true/false) should not trigger ────────────────

---@param x table<string, true>
local function _boolLiteralParam(x) _consume(x) end
-- ^ diag: none

---@type table<string, true>
local _boolLiteralField = {}
-- ^ diag: none

---@type false|string
local _falseLiteral = false
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

-- ── Generic constraints (not checked — commonly reference cross-file types) ──

---@generic T : KnownClass
---@param x T
local function _goodGeneric(x) _consume(x) end
-- ^ diag: none

---@generic T : MissingConstraint
---@param x T
local function _badGeneric(x) _consume(x) end
-- ^ diag: none

-- Constraint type used in @param should also not trigger
---@generic T : KnownClass
---@param x T
---@param y KnownClass
local function _constraintInParam(x, y) _consume(x, y) end
-- ^ diag: none

-- ── Generic type variables should not trigger ────────────────────────────

---@generic T
---@param x T
---@return T
local function _identity(x) return x end
-- ^ diag: none

-- ── Union with fun() return types should not create empty class names ───

---@class IterTestClass

---@diagnostic disable: return-mismatch
---@return IterTestClass|fun(): number, string, number @Iterator with fields
local function _unionFunReturn() return nil end
-- ^ diag: none

---@return fun(): number, string|IterTestClass
local function _unionFunReturn2() return nil end
-- ^ diag: none
---@diagnostic enable: return-mismatch

-- ── fun() return type with description should not leak into type ────────

---@param cb fun(x: string): string? Function description text
local function _funRetDesc(cb) _consume(cb) end
-- ^ diag: none

---@diagnostic disable: return-mismatch
---@return fun(x: string): string? Return description text
local function _funRetDescReturn() return nil end
-- ^ diag: none
---@diagnostic enable: return-mismatch

-- ── Vararg return type (...) in fun() should not trigger ─────────────────

---@alias VarargFunc fun(obj?: any, key: any): ...
-- ^ diag: none

---@alias VarargFunc2 fun(key: any, ...): ...
-- ^ diag: none

---@param func fun(obj?: any, key: any): ...
local function _varargFunParam(func) _consume(func) end
-- ^ diag: none

-- ── Parenthesized types ──────────────────────────────────────────────────

---@param x (string|number)
local function _parenUnionParam(x) _consume(x) end
-- ^ diag: none

---@field parenVal (KnownClass|nil)
---@class ParenFieldClass
-- ^ diag: none

---@param cb (fun(): string)
local function _parenFunParam(cb) _consume(cb) end
-- ^ diag: none

-- ── Inline table types ──────────────────────────────────────────────────

---@param opts {compressed: true}
local function _inlineTableParam(opts) _consume(opts) end
-- ^ diag: none

---@param opts {key: string, value: number}
local function _inlineTableParam2(opts) _consume(opts) end
-- ^ diag: none

---@type {[string]: number}
local _inlineTableVar = {}
-- ^ diag: none

-- ── Suppression ──────────────────────────────────────────────────────────

---@diagnostic disable: undefined-doc-class
---@type MissingSuppressed
local _suppressedVar = nil
-- ^ diag: none
---@diagnostic enable: undefined-doc-class
