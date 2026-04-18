-- Test: undefined-doc-name diagnostic
-- Fires when an annotation references a type name that isn't declared anywhere
-- (not a class, not an alias, not a primitive, not a parameterized alias, and
-- not a generic type parameter in scope).
--
-- The class-parent inheritance position (`@class Foo: Parent`) emits
-- `undefined-doc-class` instead — see tests/undefined-doc-class.lua.

local function _consume(...) end

---@class KnownClass
---@field value number

---@alias KnownAlias string | number

---@alias KnownParamAlias<T> T[]

-- ── @field type references ───────────────────────────────────────────────

---@class FieldTestClass
---@field good KnownClass
-- ^ diag: none
---@field bad MissingFieldType
-- ^ diag: undefined-doc-name
---@field alias_ref KnownAlias
-- ^ diag: none

-- ── @alias type references ───────────────────────────────────────────────

---@alias GoodAlias KnownClass
-- ^ diag: none

---@alias BadAlias MissingAliasTarget
-- ^ diag: undefined-doc-name

-- ── @param type references ───────────────────────────────────────────────

---@param x KnownClass
---@param y MissingParamType
local function _paramTest(x, y) _consume(x, y) end
-- ^ diag: undefined-doc-name

---@param x string
local function _paramPrim(x) _consume(x) end
-- ^ diag: none

---@param x KnownAlias
local function _paramAlias(x) _consume(x) end
-- ^ diag: none

-- ── @return type references ──────────────────────────────────────────────

---@return MissingReturnType
local function _returnTest() end
-- ^ diag: undefined-doc-name

---@return number
local function _goodReturn() return 1 end
-- ^ diag: none

-- ── @type on variables ──────────────────────────────────────────────────

---@type MissingVarType
local _badVar = nil
-- ^ diag: undefined-doc-name

---@type KnownClass
local _goodVar = {}
-- ^ diag: none

-- ── Parameterized unknown names ──────────────────────────────────────────

---@param x MissingParamed<number>
local function _paramedBad(x) _consume(x) end
-- ^ diag: undefined-doc-name

---@param x KnownParamAlias<number>
local function _paramedGood(x) _consume(x) end
-- ^ diag: none

-- ── Nested references inside fun() ───────────────────────────────────────

---@param cb fun(y: MissingInFun): number
local function _funParamNested(cb) _consume(cb) end
-- ^ diag: undefined-doc-name

---@diagnostic disable: return-mismatch
---@return fun(): MissingInFunRet
local function _funRetNested() return nil end
-- ^ diag: undefined-doc-name
---@diagnostic enable: return-mismatch

---@param cb fun(x: string): string
local function _funGood(cb) _consume(cb) end
-- ^ diag: none

-- ── Nested references inside inline table shapes ─────────────────────────

---@param opts { f: MissingInShape }
local function _shapeBad(opts) _consume(opts) end
-- ^ diag: undefined-doc-name

---@param opts { f: string, g: number }
local function _shapeGood(opts) _consume(opts) end
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

---@param ud userdata
---@param co thread
local function _userdataThreadTest(ud, co) _consume(ud, co) end
-- ^ diag: none

---@type unknown
local _unknownVar = nil
-- ^ diag: none

-- ── Boolean / string literal types should not trigger ────────────────────

---@param x table<string, true>
local function _boolLiteralParam(x) _consume(x) end
-- ^ diag: none

---@type false|string
local _falseLiteral = false
-- ^ diag: none

-- ── Union types ──────────────────────────────────────────────────────────

---@class UnionTestClass
---@field ok KnownClass | string
-- ^ diag: none
---@field mixed KnownClass | MissingInUnion
-- ^ diag: undefined-doc-name

-- ── Array types ──────────────────────────────────────────────────────────

---@class ArrayTestClass
---@field ok KnownClass[]
-- ^ diag: none
---@field bad MissingArrayElem[]
-- ^ diag: undefined-doc-name

-- ── @generic type parameters in scope should not trigger ─────────────────

---@generic T
---@param x T
---@return T
local function _identity(x) return x end
-- ^ diag: none

---@generic T : KnownClass
---@param x T
local function _goodGenericConstraint(x) _consume(x) end
-- ^ diag: none

-- Generic constraints themselves are NOT checked (intentional — they commonly
-- reference cross-file types that aren't fully resolvable at check time).
---@generic T : MissingConstraint
---@param x T
local function _missingGenericConstraint(x) _consume(x) end
-- ^ diag: none

-- ── Class type params inside @class body ─────────────────────────────────

---@class SmartMapReader<K, V>
---@field [K] V

---@type SmartMapReader<string, number>
local _reader = {}
_consume(_reader)
-- ^ diag: none

---@class GenericPair<A, B>
---@field first A
---@field second B

---@type GenericPair<string, number>
local _pair = {}
_consume(_pair)
-- ^ diag: none

-- ── Union with fun() return types should not trigger ─────────────────────

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

-- ── Vararg return type (...) should not trigger ──────────────────────────

---@alias VarargFunc fun(obj?: any, key: any): ...
-- ^ diag: none

---@param func fun(obj?: any, key: any): ...
local function _varargFunParam(func) _consume(func) end
-- ^ diag: none

-- ── Parenthesized types ──────────────────────────────────────────────────

---@param x (string|number)
local function _parenUnionParam(x) _consume(x) end
-- ^ diag: none

---@param cb (fun(): string)
local function _parenFunParam(cb) _consume(cb) end
-- ^ diag: none

-- ── Inline table types ──────────────────────────────────────────────────

---@type {[string]: number}
local _inlineTableVar = {}
-- ^ diag: none

-- ── Inline @type on field assignments ────────────────────────────────

local _inlineObj = {}
_inlineObj.bad = {} ---@type MissingInlineField
-- ^ diag: undefined-doc-name
_inlineObj.good = {} ---@type KnownClass
-- ^ diag: none

-- ── Inline @type on local variable expressions ─────────────────────

local _inlineBadLocal = {} ---@type MissingInlineLocal
-- ^ diag: undefined-doc-name

local _inlineGoodLocal = {} ---@type KnownClass
-- ^ diag: none

-- ── Suppression ──────────────────────────────────────────────────────

---@diagnostic disable: undefined-doc-name
---@type MissingSuppressed
local _suppressedVar = nil
-- ^ diag: none
---@diagnostic enable: undefined-doc-name
