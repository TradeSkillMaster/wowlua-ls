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

-- A constrained type param does not make its own name undefined in the body, and
-- a valid constraint type resolves cleanly.
---@alias ConstrainedAlias<T: KnownClass> T[]

-- The constraint type name itself is validated.
---@alias BadConstraintAlias<T: MissingConstraint> T[]
-- ^ diag: undefined-doc-name

-- ── @field type references ───────────────────────────────────────────────

---@class FieldTestClass
---@field good KnownClass
---@field bad MissingFieldType
-- ^ diag: undefined-doc-name
---@field alias_ref KnownAlias

-- ── @alias type references ───────────────────────────────────────────────

---@alias GoodAlias KnownClass

---@alias BadAlias MissingAliasTarget
-- ^ diag: undefined-doc-name

-- ── @param type references ───────────────────────────────────────────────

---@param x KnownClass
---@param y MissingParamType
local function _paramTest(x, y) _consume(x, y) end
-- ^ diag: undefined-doc-name

---@param x string
local function _paramPrim(x) _consume(x) end

---@param x KnownAlias
local function _paramAlias(x) _consume(x) end

-- ── @return type references ──────────────────────────────────────────────

---@return MissingReturnType
local function _returnTest() end
-- ^ diag: undefined-doc-name

---@return number
local function _goodReturn() return 1 end

-- ── @type on variables ──────────────────────────────────────────────────

---@type MissingVarType
local _badVar = nil
-- ^ diag: undefined-doc-name

---@type KnownClass
local _goodVar = {}

-- ── Parameterized unknown names ──────────────────────────────────────────

---@param x MissingParamed<number>
local function _paramedBad(x) _consume(x) end
-- ^ diag: undefined-doc-name

---@param x KnownParamAlias<number>
local function _paramedGood(x) _consume(x) end

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

-- ── Nested references inside inline table shapes ─────────────────────────

---@param opts { f: MissingInShape }
local function _shapeBad(opts) _consume(opts) end
-- ^ diag: undefined-doc-name

---@param opts { f: string, g: number }
local function _shapeGood(opts) _consume(opts) end

-- ── Built-in types should not trigger ────────────────────────────────────

---@param a number
---@param b string
---@param c boolean
---@param d table
---@param e function
---@param f nil
---@param g any
local function _builtinTest(a, b, c, d, e, f, g) _consume(a, b, c, d, e, f, g) end

---@param ud userdata
---@param co thread
local function _userdataThreadTest(ud, co) _consume(ud, co) end

---@type unknown
local _unknownVar = nil

-- ── Boolean / string literal types should not trigger ────────────────────

---@param x table<string, true>
local function _boolLiteralParam(x) _consume(x) end

---@type false|string
local _falseLiteral = false

-- ── Union types ──────────────────────────────────────────────────────────

---@class UnionTestClass
---@field ok KnownClass | string
---@field mixed KnownClass | MissingInUnion
-- ^ diag: undefined-doc-name

-- ── Array types ──────────────────────────────────────────────────────────

---@class ArrayTestClass
---@field ok KnownClass[]
---@field bad MissingArrayElem[]
-- ^ diag: undefined-doc-name

-- ── @generic type parameters in scope should not trigger ─────────────────

---@generic T
---@param x T
---@return T
local function _identity(x) return x end

---@generic T : KnownClass
---@param x T
local function _goodGenericConstraint(x) _consume(x) end

-- Generic constraints themselves are NOT checked (intentional — they commonly
-- reference cross-file types that aren't fully resolvable at check time).
---@generic T : MissingConstraint
---@param x T
local function _missingGenericConstraint(x) _consume(x) end

-- ── Class type params inside @class body ─────────────────────────────────

---@class IndexedLookup<K, V>
---@field [K] V

---@type IndexedLookup<string, number>
local _reader = {}
_consume(_reader)

---@class GenericPair<A, B>
---@field first A
---@field second B

---@type GenericPair<string, number>
local _pair = {}
_consume(_pair)

-- ── Union with fun() return types should not trigger ─────────────────────

---@class IterTestClass

---@diagnostic disable: return-mismatch
---@return IterTestClass|fun(): number, string, number @Iterator with fields
local function _unionFunReturn() return nil end

---@return fun(): number, string|IterTestClass
local function _unionFunReturn2() return nil end
---@diagnostic enable: return-mismatch

-- ── fun() return type with description should not leak into type ────────

---@param cb fun(x: string): string? Function description text
local function _funRetDesc(cb) _consume(cb) end

-- ── Vararg return type (...) should not trigger ──────────────────────────

---@alias VarargFunc fun(obj?: any, key: any): ...

---@param func fun(obj?: any, key: any): ...
local function _varargFunParam(func) _consume(func) end

-- ── Parenthesized types ──────────────────────────────────────────────────

---@param x (string|number)
local function _parenUnionParam(x) _consume(x) end

---@param cb (fun(): string)
local function _parenFunParam(cb) _consume(cb) end

-- ── Inline table types ──────────────────────────────────────────────────

---@type {[string]: number}
local _inlineTableVar = {}

-- ── Inline @type on field assignments ────────────────────────────────

local _inlineObj = {}
_inlineObj.bad = {} ---@type MissingInlineField
-- ^ diag: undefined-doc-name
_inlineObj.good = {} ---@type KnownClass

-- ── Inline @type on local variable expressions ─────────────────────

local _inlineBadLocal = {} ---@type MissingInlineLocal
-- ^ diag: undefined-doc-name

local _inlineGoodLocal = {} ---@type KnownClass

-- ── Numeric keys in table shapes ─────────────────────────────────────

---@alias NumericKeyTuple {[1]: string, [2]: number, [3]: number?, [4]: number?}

---@type {[1]: string, [2]: boolean}
local _numericKeyTable = {}

-- ── Space after --- variant (--- @param) ─────────────────────────────
-- Regression: `--- @param` (space after `---`) used to fall through the
-- comment-range lookup and emit the diagnostic spanning the whole function
-- body.  The fix is in comment_is_tag() which handles both `---@` and `--- @`.

--- @param x KnownClass
--- @param y MissingSpaceParamType
local function _spaceParamTest(x, y) _consume(x, y) end
-- ^ diag: undefined-doc-name

--- @return MissingSpaceReturnType
local function _spaceReturnTest() end
-- ^ diag: undefined-doc-name

-- ── Suppression ──────────────────────────────────────────────────────

---@diagnostic disable: undefined-doc-name
---@type MissingSuppressed
local _suppressedVar = nil
---@diagnostic enable: undefined-doc-name
