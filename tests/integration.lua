---@diagnostic disable: create-global, undefined-global
-- wowlua_ls integration test
-- Annotations on the line below code use caret to mark test column
-- Format: --  caret hover: TYPE  def: local|external|None

local x = 5
--    ^ hover: (local) x: number = 5  def: local

local neg = -1
--    ^ hover: (local) neg: number = -1  def: local

local neg100 = -100
--    ^ hover: (local) neg100: number = -100  def: local

local y = x + 2
--    ^ hover: (local) y: number  def: local

local s = "hello"
--    ^ hover: (local) s: string = "hello"  def: local

local b = true
--    ^ hover: (local) b: true  def: local

local n = nil
--    ^ hover: (local) n: nil  def: local

local function AddTwo(val)
    return val + 2
end

local result = AddTwo(x)
--    ^ hover: (local) result: number  def: local

local f = AddTwo
--    ^ hover: (local) function f(val: number)  def: local

local function GetPair()
    return 11, 22
end
local a, b2 = GetPair()
--    ^ hover: (local) a: number  def: local

-- ── Unannotated function with every `return` in a nested branch ──────
-- The FunctionRet symbols live in the if/else scopes, not the function
-- body scope, so the direct scope-lookup misses them. The fallback over
-- `func.rets` unions each slot's resolved type.
local cond = true
local function NestedReturns()
    if cond then
        return 1
    else
        return 2
    end
end
local nr = NestedReturns()
--    ^ hover: (local) nr: number  def: local

do
    local inner = 99
    --    ^ hover: (local) inner: number = 99  def: local
    local sum = inner + x
    --    ^ hover: (local) sum: number  def: local
end

-- WoW addon varargs: local addonName, ns = ...
local addonName, ns = ...
--    ^ hover: (local) addonName: string  def: local
ns.version = 1
ns.title = "MyAddon"
local ver = ns.version
--    ^ hover: (local) ver: number  def: local
local title = ns.title
--    ^ hover: (local) title: string  def: local

-- ── And/or with nullable union produces boolean, not true ────────────
---@type number?
local maybeNum = nil
local ternary = maybeNum and true or false
--    ^ hover: (local) ternary: boolean  def: local

-- ── `not` on unresolved operand still produces boolean ──────────────
-- Prevents `not x and func() or nil` from collapsing to nil when x is unresolved.
local function _notUnresolved(unknownParam)
    local notResult = not unknownParam
    --    ^ hover: (local) notResult: boolean  def: local
    ---@return string
    local function getStr() return "" end
    local andOrResult = not unknownParam and getStr() or nil
    --    ^ hover: (local) andOrResult: string?  def: local
end
local _ = _notUnresolved

-- ── Dotted method with unresolved intermediate should not leak to root table ──
local MyObj = {}
MyObj.knownField = 1
function MyObj.__private:_Helper()
end
local kf = MyObj.knownField
--    ^ hover: (local) kf: number  def: local
local hp = MyObj._Helper
--    ^ hover: (local) hp: ?  def: local

-- ── Right-associative ^ operator ──
local pow = 2 ^ 3 ^ 4
--    ^ hover: (local) pow: number  def: local
local concat = "a" .. "b" .. "c"
--    ^ hover: (local) concat: string  def: local

-- ── Nil-init with branch reassignment ──
local nilInit = nil
if x > 3 then
    nilInit = "yes"
elseif x > 1 then
    nilInit = "maybe"
else
    nilInit = "no"
end
local useNilInit = nilInit
--    ^ hover: (local) useNilInit: string  def: local

-- ── Nil arg should not propagate nil type to function params ──
local nilArgTbl = { x = nil }
local nilArgResult = nilArgTbl.nilArgFunc(nilArgTbl.x, "hello")
--    ^ hover: (local) nilArgResult: ?  def: local
function nilArgTbl.nilArgFunc(a, b)
--                            ^ hover: (param) a: ?  def: local
    return b
end

-- ── Unannotated param: return type is unknown ──
local function multiParam(a) return a end
multiParam(1)
multiParam("hi")
local mpResult = multiParam(true)
--    ^ hover: (local) mpResult: ?  def: local

-- ── Unannotated param hover shows ? (no call-site inference) ──
local function inferredHover(x, y)
--                           ^ hover: (param) x: ?  def: local
--                              ^ hover: (param) y: ?  def: local
    return x, y
end
inferredHover("hello", 1)
inferredHover(42, nil)
inferredHover(nil)

-- ── Param hover should not leak type from reassignment in body ──
local function paramReassign(p)
--                           ^ hover: (param) p: ?  def: local
    p = { x = 1 }
end

-- ── Param type in function signature should not leak type-guard narrowing ──
local function typeGuardParam(val)
--                            ^ hover: (param) val: ?  def: local
    if type(val) == "table" then
        return val
    end
end
local tgpResult = typeGuardParam({})
--    ^ hover: (local) tgpResult: ?  def: local

-- ── Inverse type guard: else branch strips matched type from union ──
---@param val string|number
local function inverseTypeGuardHover(val)
    if type(val) == "string" then
        local _ = val
--                ^ hover: (param) val: string  def: local
    else
        local _ = val
--                ^ hover: (param) val: number  def: local
    end
end

-- ── Inverse type guard: early exit strips matched type ──
---@param val string|number
local function inverseTypeGuardEarlyExit(val)
    if type(val) == "string" then return end
    local _ = val
--            ^ hover: (param) val: number  def: local
end

-- ── Implicit nil return: bare `return` unions nil into inferred type ──
local function bareAndValue(cond)
--             ^ hover: (local) function bareAndValue(cond)\n-> true?  def: local
    if not cond then return end
    return true
end
local bavResult = bareAndValue(true)
--    ^ hover: (local) bavResult: true?  def: local

-- ── Implicit nil return (fall-through): hover shows unioned nil ──
local function fallThrough(cond)
--             ^ hover: (local) function fallThrough(cond)\n-> number?  def: local
    if cond then return 42 end
end

-- ── Only bare returns (no value): bare return returns zero values, not nil ──
local function onlyBare(cond)
--             ^ hover: (local) function onlyBare(cond)  def: local
    if cond then return end
    return
end
local obResult = onlyBare(true)
--    ^ hover: (local) obResult: nil  def: local

-- ── Void function (no return statements): no return type shown ──
local function voidFunc(x)
--             ^ hover: (local) function voidFunc(x)  def: local
    print(x)
end

-- ── Unconditional return: no implicit nil, inferred type stays narrow ──
local function alwaysReturns()
--             ^ hover: (local) function alwaysReturns()\n-> number  def: local
    return 7
end
local arResult = alwaysReturns()
--    ^ hover: (local) arResult: number  def: local

-- ── `@return` annotation wins over implicit-nil union ──
-- With a user-supplied annotation, fall-through should not widen `-> number`
-- to `-> number | nil`. Soundness is handled by the `missing-return` diagnostic.
---@param x any
---@return number
---@diagnostic disable-next-line: missing-return
local function annotatedBare(x)
--             ^ hover: (local) function annotatedBare(x: any)\n-> number  def: local
    if x then return 1 end
end

-- ── Or then-branch narrowing: union of each term's effect ──
---@param value? number|string
local function orThenNarrow(value)
    if value == nil or type(value) == "number" then
        local _ = value
--                ^ hover: (param) value: number?  def: local
    else
        local _ = value
--                ^ hover: (param) value: string  def: local
    end
end

-- ── Or then-branch narrowing: multiple type guards ──
---@param value number|string|boolean
local function orThenMultiType(value)
    if type(value) == "number" or type(value) == "string" then
        local _ = value
--                ^ hover: (param) value: number | string  def: local
    end
end

-- ── Type guard else-branch strips table from union with array type ──
---@param val string|string[]
local function typeGuardTableElse(val)
    if type(val) == "table" then
        local _ = val
--                ^ hover: (param) val: string[]  def: local
    else
        local _ = val
--                ^ hover: (param) val: string  def: local
    end
end

-- ── Type guard else-branch strips string from union with class type ──
---@class PublisherSchemaBase
---@param val string|PublisherSchemaBase
local function typeGuardStringElse(val)
    if type(val) == "string" then
        local _ = val
--                ^ hover: (param) val: string  def: local
    else
        local _ = val
--                ^ hover: (param) val: PublisherSchemaBase  def: local
    end
end

-- ── Type guard early-exit strips table from union with array type ──
---@param val string|string[]
local function typeGuardTableEarlyExit(val)
    if type(val) == "table" then return end
    local _ = val
--            ^ hover: (param) val: string  def: local
end

-- ── Type guard elseif branches get inverse narrowing from first condition ──
---@param val string|string[]
local function typeGuardElseif(val)
    if type(val) == "table" then
        local _ = val
--                ^ hover: (param) val: string[]  def: local
    elseif val == "hello" then
        local _ = val
--                ^ hover: (param) val: "hello"  def: local
    else
        local _ = val
--                ^ hover: (param) val: string  def: local
    end
end

-- ── Or-condition else-branch strips multiple types ──
---@class OrTestPublisher
---@param val string|number|OrTestPublisher
local function typeGuardOrElse(val)
    if type(val) == "string" or type(val) == "number" then
        local _ = val
--                ^ hover: (param) val: string | number  def: local
    else
        local _ = val
--                ^ hover: (param) val: OrTestPublisher  def: local
    end
end

-- ── Or-condition with three terms (nested or) narrows correctly ──
---@class ThreeTermPublisher
---@param val string|number|nil|ThreeTermPublisher
local function threeTermOrGuard(val)
    if type(val) == "string" or type(val) == "number" or val == nil then
        local _ = val
--                ^ hover: (param) val: string | number?  def: local
    else
        local _ = val
--                ^ hover: (param) val: ThreeTermPublisher  def: local
    end
end

-- ── Caller hover on function with narrowed params should not show narrowed type ──
---@param x number
local function callerOfGuardParam(x)
    typeGuardParam(x)
--  ^ hover: (local) function typeGuardParam(val)  def: local
end

-- ── Function-level varargs should not get file-level WoW type ──
local function varargFunc(action, ...)
    local idx = ...
--        ^ hover: (local) idx: ?  def: local
    local a, b = ...
--        ^ hover: (local) a: ?  def: local
--           ^ hover: (local) b: ?  def: local
    return idx
end

-- ── Field hover should not be shadowed by same-named global ──
local function GetText() return "global" end
local Inbox = {}
---@param index number
---@return string
function Inbox.GetText(index) return "inbox" end
local gt = Inbox.GetText(1)
--               ^ hover: (field) function GetText(index: number)  def: local
--    ^ hover: (local) gt: string  def: local

-- ── Branch-local variable type: reassignment in sibling branch should not leak ──
local branchVar = 5
if branchVar < 0 then
    branchVar = "negative"
elseif branchVar >= 1 then
    local branchUse = branchVar
    --    ^ hover: (local) branchUse: number  def: local
    branchVar = "positive"
else
    local branchUse2 = branchVar
    --    ^ hover: (local) branchUse2: number  def: local
    branchVar = "zero"
end

-- ── For-loop variable hover at definition site ──
---@type table<string, number>
local forTbl
for forKey, forVal in pairs(forTbl) do
--  ^ hover: (local) forKey: string  def: local
    local useKey = forKey
    --             ^ hover: (local) forKey: string  def: local
    local useVal = forVal
    --             ^ hover: (local) forVal: number  def: local
end
for forIdx = 1, 10 do
--  ^ hover: (local) forIdx: number  def: local
    local useIdx = forIdx
    --             ^ hover: (local) forIdx: number  def: local
end

-- ── Numeric for inside function (only statement) — regression: header was <none> ──
-- When the for-loop is the only statement in a function body, the function body
-- block has the same text range as the ForCountLoop node. The loop variable must
-- still resolve to `number` at its declaration site in the header.
local function processForRevItems(forRevItems)
    for forRevIdx = #forRevItems, 1, -1 do
    --  ^ hover: (local) forRevIdx: number  def: local
        local useRevIdx = forRevIdx
        --                ^ hover: (local) forRevIdx: number  def: local
    end
end

-- ── For-in inside function (only statement) — same range-collision fix ──
---@param forInParam table<string, number>
local function processForIn(forInParam)
    for forInKey, forInVal in pairs(forInParam) do
    --  ^ hover: (local) forInKey: string  def: local
        local _use = forInKey
    end
end

-- ── for-in with `next, tbl` (multi-expression generic for protocol) ──
---@generic K, V
---@param t table<K, V>
---@param index? K
---@return K key
---@return V value
---@diagnostic disable-next-line: missing-return
local function myNext(t, index) end

for nextKey, nextVal in myNext, forTbl do
--  ^ hover: (local) nextKey: string  def: local
    local _useNextKey = nextKey
    --                  ^ hover: (local) nextKey: string  def: local
    local _useNextVal = nextVal
    --                  ^ hover: (local) nextVal: number  def: local
end

-- Non-generic iterator with state expression: concrete return types resolve directly
---@param t table
---@param index? number
---@return string name
---@return number count
---@diagnostic disable-next-line: missing-return
local function concreteIter(t, index) end

for ciName, ciCount in concreteIter, forTbl do
--  ^ hover: (local) ciName: string  def: local
    local _useCiCount = ciCount
    --                  ^ hover: (local) ciCount: number  def: local
end

-- Three-expression form (iter, state, init) should not break
for triKey, triVal in myNext, forTbl, nil do
--  ^ hover: (local) triKey: string  def: local
    local _useTriVal = triVal
    --                 ^ hover: (local) triVal: number  def: local
end

-- Untyped table with multi-expression iterator falls back gracefully
local plainTbl = {}
for ptKey, ptVal in myNext, plainTbl do
    local _usePt = ptKey
    --             ^ hover: (local) ptKey: ?  def: local
end

-- ── Branch merge: nil-initialized variable assigned in all branches ──
-- When a variable is initialized to nil and then assigned in every branch
-- of an if/elseif/else, the merged type should reflect the branch types.
local function _branchMergeTest(cond1, cond2, cond3)
    -- Simple if/else
    local x1 = nil
    if cond1 then
        x1 = 5
    else
        x1 = 10
    end
    local _r1 = x1
    --          ^ hover: (local) x1: number

    -- If/elseif/else with nested if/else in else branch (inner if/else is
    -- the LAST statement in the else block — tests that merges are processed
    -- even when the block is about to pop)
    local x2 = nil
    if cond1 then
        x2 = 5
    elseif cond2 then
        x2 = 1
    else
        if cond3 then
            x2 = 10
        else
            x2 = 20
        end
    end
    local _r2 = x2
    --          ^ hover: (local) x2: number

    -- Mixed types across branches: union should include all branch types
    local x3 = nil
    if cond1 then
        x3 = 5
    else
        if cond2 then
            x3 = "hello"
        else
            x3 = true
        end
    end
    local _r3 = x3
    --          ^ hover: (local) x3: number | string | true
end

-- ── For-in over branch-merged union table ──
-- When a variable is assigned different table types in if/else branches,
-- pairs() iteration should yield the union of both value types.
local function _forInUnionTableTest(cond)
    local tbl = nil
    if cond then
        tbl = {1, 2, 3}
    else
        tbl = {"a", "b", "c"}
    end
    for _, branchVal in pairs(tbl) do
        local _use = branchVal
        --           ^ hover: (local) branchVal: number | string
    end
    -- Key should be number (both branches use positional integer keys)
    for branchKey, _ in pairs(tbl) do
        local _k = branchKey
        --         ^ hover: (local) branchKey: number
    end
end

-- ── Table constructor key/value type inference ──
local bracketStrMap = { ["foo"] = "bar", ["baz"] = "qux" }
--    ^ hover: (local) bracketStrMap: table<string, string>  def: local
local bracketNumMap = { ["a"] = 1, ["b"] = 2 }
--    ^ hover: (local) bracketNumMap: table<string, number>  def: local
local bracketNumKeyArr = { [1] = "one", [2] = "two" }
--    ^ hover: (local) bracketNumKeyArr: string[]  def: local
local positionalArr = { "hello", "world" }
--    ^ hover: (local) positionalArr: string[]  def: local
local bracketIdx = bracketStrMap["foo"]
--    ^ hover: (local) bracketIdx: string  def: local
-- Phase 2 deferred inference: keys/values are variables, not literals
local _bKey1 = "x"
local _bKey2 = "y"
local _bVal1 = 10
local _bVal2 = 20
local bracketVarMap = { [_bKey1] = _bVal1, [_bKey2] = _bVal2 }
--    ^ hover: (local) bracketVarMap: table<string, number>  def: local
-- String-literal bracket keys produce named fields (like `a = v`)
local bracketNamedAccess = bracketStrMap.foo
--    ^ hover: (local) bracketNamedAccess: string  def: local
local mixedTable = { ["a"] = 1, x = "hello" }
local _mixedA = mixedTable.a
--    ^ hover: (local) _mixedA: number  def: local
local _mixedX = mixedTable.x
--    ^ hover: (local) _mixedX: string  def: local
-- Phase 2 deferred inference for positional arrays with variable values
local _arrVal1 = "foo"
local _arrVal2 = "bar"
local positionalVarArr = { _arrVal1, _arrVal2 }
--    ^ hover: (local) positionalVarArr: string[]  def: local
-- Dynamic bracket assignment infers table value_type
local dynBracketTbl = {}
dynBracketTbl[1] = "hello"
dynBracketTbl[2] = "world"
local dynBracketVal = dynBracketTbl[1]
--    ^ hover: (local) dynBracketVal: string  def: local
-- Dynamic bracket assignment with number values
local dynNumTbl = {}
dynNumTbl[1] = 10
dynNumTbl[2] = 20
local dynNumVal = dynNumTbl[1]
--    ^ hover: (local) dynNumVal: number  def: local
-- Dynamic bracket assignment inside a do block
local doBlockResult = nil
local doBlockTbl = {}
do
    doBlockTbl[0] = 42
    doBlockResult = doBlockTbl[0]
end
local doBlockCheck = doBlockResult
--    ^ hover: (local) doBlockCheck: number  def: local
-- Dynamic bracket assignment inside a for loop within a do block
local forLoopResult = nil
local forLoopTbl = {}
do
    for i = 0, 9 do
        forLoopTbl[i] = i * 2
    end
    forLoopResult = forLoopTbl[5]
end
local forLoopCheck = forLoopResult
--    ^ hover: (local) forLoopCheck: number  def: local

-- Multiple return nil early-exits should merge into one return slot (not duplicate)
local multiRetNs = {}
function multiRetNs.helper(a, b, c)
    if not a then
        return nil
    end
    if not b then
        return nil
    end
    if not c then
        return nil
    end
    local x = 42
    return x > 0 and x or nil
end
local multiRetResult = multiRetNs.helper(1, 2, 3)
--    ^ hover: (local) multiRetResult: number?  def: local

-- ── do-block upvalue propagation ──────────────────────────────────────
-- Reassignments inside a do-block should be visible in function bodies
-- defined after the do-block (the do-block executes unconditionally).

local doBlockVar = nil
do
    doBlockVar = 42
end
function doBlockConsumer()
    local captured = doBlockVar
    --    ^ hover: (local) captured: number  def: local
end

-- do-block with non-nil table assignment
---@diagnostic disable-next-line: redefined-local
local doBlockTbl = nil
do
    doBlockTbl = { name = "hello" }
end
function doBlockTblConsumer()
    local captured = doBlockTbl.name
    --    ^ hover: (local) captured: string  def: local
end

-- nested do-blocks
local nestedDoVar = nil
do
    do
        nestedDoVar = "inner"
    end
end
function nestedDoConsumer()
    local captured = nestedDoVar
    --    ^ hover: (local) captured: string  def: local
end

-- sequential do-blocks: second overwrites first
local seqDoVar = nil
do
    seqDoVar = 42
end
do
    seqDoVar = "overwritten"
end
function seqDoConsumer()
    local captured = seqDoVar
    --    ^ hover: (local) captured: string  def: local
end

-- multiple variables reassigned in one do-block
local doMultiA = nil
local doMultiB = nil
do
    doMultiA = 100
    doMultiB = "hello"
end
function doMultiConsumer()
    local a = doMultiA
    --    ^ hover: (local) a: number  def: local
    local b = doMultiB
    --    ^ hover: (local) b: string  def: local
end

-- do-block local should NOT leak to outer scope
do
    local doLocalOnly = 99
end
function doLocalLeakTest()
    local captured = doLocalOnly
    --    ^ hover: (local) captured: ?  def: local
end

-- ── Indirect _G access as global resolution ──────────────────────────────────

---@param a number
---@return string
function myGlobalFunc(a) return tostring(a) end

-- Indirect _G: local aliasing _G resolves user-defined globals
local _gRef = _G
local _gFunc = _gRef.myGlobalFunc
--    ^ hover: (local) function _gFunc(a: number)\n-> string

-- _G dot write creates a global that can be read back
_G.TestGlobalValue = 123
local _gReadback = _G.TestGlobalValue
--    ^ hover: (local) _gReadback: number

-- function _G.Foo() creates a top-level global function
---@param x number
---@return number
function _G.GFuncViaG(x) return x + 1 end
local _gFuncResult = GFuncViaG(5)
--    ^ hover: (local) _gFuncResult: number

-- function _G.Table:Method() creates a method on a global table
_G.GTableViaG = {}
---@param name string
---@return boolean
function _G.GTableViaG:Check(name) return true end
local _gMethodResult = GTableViaG:Check("test")
--    ^ hover: (local) _gMethodResult: boolean

-- Table with bracket-keyed entries whose values are arrays of mixed type:
-- structural dedup should collapse identical anonymous tables into one type,
-- and the array element union should be parenthesized.
local bracketArrayMap = { ["alpha"] = { "a", "b", 1 }, ["beta"] = { "c", "d", 2 }, ["gamma"] = { "e", "f", 3 } }
--    ^ hover: (local) bracketArrayMap: table<string, (string | number)[]>
local _bamEntry = bracketArrayMap["alpha"]
--    ^ hover: (local) _bamEntry: (string | number)[]

-- Subtable values: all entries have identical structure, should collapse to one type
local subtableArr = { [1] = { id = 1, t = {} }, [2] = { id = 3, t = {} }, [3] = { id = 2, t = {} }, [4] = { id = 4, t = {} } }
--    ^ hover: (local) subtableArr: {id: number, t: table}[]  def: local
local subtableMap = { ["a"] = { id = 1, t = {} }, ["b"] = { id = 2, t = {} } }
--    ^ hover: (local) subtableMap: table<string, {id: number, t: table}>  def: local
-- Nested subtables: inline fields expand to depth 4
local nestedArr = { [1] = { info = { name = "a", tags = { "x" } } }, [2] = { info = { name = "b", tags = { "y" } } } }
--    ^ hover: (local) nestedArr: {info: {name: string, tags: string[]}}[]  def: local
-- Element access extracts the value_type with full field structure
local nestedEntry = nestedArr[1]
--    ^ hover: (local) nestedEntry: {
local nestedInfo = nestedEntry.info
--    ^ hover: (local) nestedInfo: {
local nestedName = nestedInfo.name
--    ^ hover: (local) nestedName: string  def: local
-- Positional arrays of subtables
local posSubArr = { { x = 1, y = 2 }, { x = 3, y = 4 } }
--    ^ hover: (local) posSubArr: {x: number, y: number}[]  def: local
-- Mixed primitive and subtable fields
local mixedFieldArr = { [1] = { count = 5, label = "a" }, [2] = { count = 8, label = "b" } }
--    ^ hover: (local) mixedFieldArr: {count: number, label: string}[]  def: local

-- ── Method self type for dotted base (A.B:C) ──────────────────────────────
-- self should be the sub-table (A.B), not the root (A)
local MethodSelfRoot = {}
MethodSelfRoot.Sub = {}
function MethodSelfRoot.Sub:DoStuff()
    self.foo = 1
end
function MethodSelfRoot.Sub:Check()
    self.foo
--  ^    hover: (param) self: {
--       ^ hover: (field) foo: number
end

-- Simple 2-name case still works: function Obj:Method()
local SimpleSelfObj = {}
function SimpleSelfObj:SetVal()
    self.val = 1
end
function SimpleSelfObj:GetVal()
    self.val
--  ^   hover: (param) self: {
--       ^ hover: (field) val: number
end

-- Self-referential anonymous table: hovering on the table should not recursively
-- expand self-type through colon methods (fun(self: {Func: fun(self: ...)}))
local SelfRefAnon = {}
function SelfRefAnon:Run() end
local _selfRefCopy = SelfRefAnon
--    ^ hover: (local) _selfRefCopy: {\nRun: fun(self: table)\n}

-- owner_class resolution through dotted path: @return ClassName → @return self
---@class DottedOwnerWidget
---@field value number
local DottedOwnerWidget = {}
local DottedOwnerNs = {}
DottedOwnerNs.Widget = DottedOwnerWidget
---@return DottedOwnerWidget
function DottedOwnerNs.Widget:Clone()
    return self
end
---@type DottedOwnerWidget
local _dow
local _dowClone = _dow:Clone()
--    ^ hover: (local) _dowClone: DottedOwnerWidget

-- ── Bracket assignment nil filtering ─────────────────────────────────
-- Writing nil to a list slot clears it — nil should not appear in the
-- inferred element type.

-- Annotated list: annotation is authoritative, bracket assignments don't override
-- (no type-mismatch diagnostic for bracket index assignments yet)
---@type number[]
local annotNumArr = {}
annotNumArr[1] = nil
annotNumArr[2] = ""
local annotNumArrVal = annotNumArr[1]
--    ^ hover: (local) annotNumArrVal: number  def: local

-- Unannotated list: nil assignments should not add nil to the element type
local inferArr = {}
inferArr[1] = 0
inferArr[2] = nil
local inferArrVal = inferArr[1]
--    ^ hover: (local) inferArrVal: number  def: local

-- Already-resolved list (Phase 1 literals): nil bracket writes excluded
local mixedArr = {1, 2, 3}
mixedArr[4] = nil
local mixedArrVal = mixedArr[1]
--    ^ hover: (local) mixedArrVal: number  def: local

-- local x = x: nested shadowing — inner RHS resolves to outer local
local outerShadow = 42
do
    local outerShadow = outerShadow + 1
    --    ^ hover: (local) outerShadow: number  def: local
    --                     ^ hover: (local) outerShadow: number = 42  def: local
end

-- Regression: keyword tokens must suppress scope completions.
-- Typing `if expr then` was offering symbols matching "t*" and Enter replaced "then".
local keywordTest = true
if keywordTest then
    --             ^ comp: none
end
--^ comp: none

-- Regression: partial keyword prefix must offer only the required keyword in
-- unambiguous positions (after if/elseif/while condition, only `then`/`do` fits).
-- Typing `if expr th` was completing to external globals like THE_ALLIANCE.
local partialKwCond = true
if partialKwCond th
--                 ^ comp: then
local whileDoTest = 1
while whileDoTest > 0 d
--                     ^ comp: do
local forInTbl = {}
for _, _ in pairs(forInTbl) d
--                            ^ comp: do

-- Bracket access on a table whose fields are all the same type should return that type as nilable.
-- Dynamic key on all-string fields → string?
local LABEL_MAP = { alpha = "first", beta = "second", gamma = "third" }
---@param key string
local function testBracketFieldInference(key)
    local dynResult = LABEL_MAP[key]
    --    ^ hover: (local) dynResult: string?
    local litResult = LABEL_MAP["alpha"]
    --    ^ hover: (local) litResult: string
end

-- Loop-carried variable type inference: variables assigned in an if-branch
-- inside a loop should be non-nil in subsequent elseif/else branches (since
-- they were assigned in a previous iteration).
local function loopCarriedType(items)
    local count = nil
    for i = 1, 10 do
        if not count then
            count = i or 0
        elseif i < count then
            local useCount = count
            --    ^ hover: (local) useCount: number
        end
    end
    -- While loop variant
    local state = nil
    local idx = 0
    while idx < 10 do
        idx = idx + 1
        if not state then
            state = "init"
        else
            local useState = state
            --    ^ hover: (local) useState: string
        end
    end
    -- Repeat loop variant
    local phase = nil
    repeat
        if not phase then
            phase = 1
        else
            local usePhase = phase
            --    ^ hover: (local) usePhase: number
        end
    until phase == 1
    -- Closures in loops should not be affected (preserve nil at definition time)
    local captured = nil
    for j = 1, 5 do
        local fn_inside = function() return captured end
        --                                  ^ hover: (local) captured: nil
        if not captured then
            captured = j
        end
    end
end

-- ── Boolean truthiness guard narrowing ───────────────────────────────
-- Inside `if flag then`, a bare `boolean` narrows to `true` (the only
-- truthy boolean value in Lua). `boolean?` also narrows to `true`.

---@param flag boolean
---@param optFlag boolean?
local function boolGuardNarrow(flag, optFlag)
    if flag then
        local x = flag
        --    ^ hover: (local) x: true
    end
    if optFlag then
        local y = optFlag
        --    ^ hover: (local) y: true
    end
end
_consume(boolGuardNarrow)
