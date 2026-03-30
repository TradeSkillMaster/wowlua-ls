-- wowlua_ls integration test
-- Annotations on the line below code use caret to mark test column
-- Format: --  caret hover: TYPE  def: local|external|None

local x = 5
--    ^ hover: (global) x: number = 5  def: local

local y = x + 2
--    ^ hover: (global) y: number  def: local

local s = "hello"
--    ^ hover: (global) s: string = "hello"  def: local

local b = true
--    ^ hover: (global) b: true  def: local

local n = nil
--    ^ hover: (global) n: nil  def: local

local function AddTwo(val)
    return val + 2
end

local result = AddTwo(x)
--    ^ hover: (global) result: number  def: local

local f = AddTwo
--    ^ hover: (global) function f(val)  def: local

local function GetPair()
    return 11, 22
end
local a, b2 = GetPair()
--    ^ hover: (global) a: number  def: local

do
    local inner = 99
    --    ^ hover: (local) inner: number  def: local
    local sum = inner + x
    --    ^ hover: (local) sum: number  def: local
end

-- WoW addon varargs: local addonName, ns = ...
local addonName, ns = ...
--    ^ hover: (global) addonName: string  def: local
ns.version = 1
ns.title = "MyAddon"
local ver = ns.version
--    ^ hover: (global) ver: number  def: local
local title = ns.title
--    ^ hover: (global) title: string  def: local

-- ── And/or with nullable union produces boolean, not true ────────────
---@type number?
local maybeNum = nil
local ternary = maybeNum and true or false
--    ^ hover: (global) ternary: boolean  def: local

-- ── Dotted method with unresolved intermediate should not leak to root table ──
local MyObj = {}
MyObj.knownField = 1
function MyObj.__private:_Helper()
end
local kf = MyObj.knownField
--    ^ hover: (global) kf: number  def: local
local hp = MyObj._Helper
--    ^ hover: (global) hp: ?  def: local

-- ── Right-associative ^ operator ──
local pow = 2 ^ 3 ^ 4
--    ^ hover: (global) pow: number  def: local
local concat = "a" .. "b" .. "c"
--    ^ hover: (global) concat: string  def: local

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
--    ^ hover: (global) useNilInit: string  def: local

-- ── Nil arg should not propagate nil type to function params ──
local nilArgTbl = { x = nil }
local nilArgResult = nilArgTbl.nilArgFunc(nilArgTbl.x, "hello")
--    ^ hover: (global) nilArgResult: ?  def: local
function nilArgTbl.nilArgFunc(a, b)
--                             ^ hover: (param) a: ?  def: local
    return b
end

-- ── Unannotated param: return type is unknown ──
local function multiParam(a) return a end
multiParam(1)
multiParam("hi")
local mpResult = multiParam(true)
--    ^ hover: (global) mpResult: ?  def: local

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
--    ^ hover: (global) tgpResult: ?  def: local

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
---@class ReactivePublisherSchemaBase
---@param val string|ReactivePublisherSchemaBase
local function typeGuardStringElse(val)
    if type(val) == "string" then
        local _ = val
--                ^ hover: (param) val: string  def: local
    else
        local _ = val
--                ^ hover: (param) val: ReactivePublisherSchemaBase  def: local
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
--                ^ hover: (param) val: string  def: local
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
--  ^ hover: (global) function typeGuardParam(val)  def: local
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
--    ^ hover: (global) gt: string  def: local

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

-- ── Table constructor key/value type inference ──
local bracketStrMap = { ["foo"] = "bar", ["baz"] = "qux" }
--    ^ hover: (global) bracketStrMap: table<string, string>  def: local
local bracketNumMap = { ["a"] = 1, ["b"] = 2 }
--    ^ hover: (global) bracketNumMap: table<string, number>  def: local
local bracketNumKeyArr = { [1] = "one", [2] = "two" }
--    ^ hover: (global) bracketNumKeyArr: string[]  def: local
local positionalArr = { "hello", "world" }
--    ^ hover: (global) positionalArr: string[]  def: local
local bracketIdx = bracketStrMap["foo"]
--    ^ hover: (global) bracketIdx: string  def: local
-- Phase 2 deferred inference: keys/values are variables, not literals
local _bKey1 = "x"
local _bKey2 = "y"
local _bVal1 = 10
local _bVal2 = 20
local bracketVarMap = { [_bKey1] = _bVal1, [_bKey2] = _bVal2 }
--    ^ hover: (global) bracketVarMap: table<string, number>  def: local
-- String-literal bracket keys produce named fields (like `a = v`)
local bracketNamedAccess = bracketStrMap.foo
--    ^ hover: (global) bracketNamedAccess: string  def: local
local mixedTable = { ["a"] = 1, x = "hello" }
local _mixedA = mixedTable.a
--    ^ hover: (global) _mixedA: number  def: local
local _mixedX = mixedTable.x
--    ^ hover: (global) _mixedX: string  def: local
-- Phase 2 deferred inference for positional arrays with variable values
local _arrVal1 = "foo"
local _arrVal2 = "bar"
local positionalVarArr = { _arrVal1, _arrVal2 }
--    ^ hover: (global) positionalVarArr: string[]  def: local
