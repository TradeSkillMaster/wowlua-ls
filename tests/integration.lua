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
