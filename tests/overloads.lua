-- Test: @overload resolution

-- math.random has overloads:
--   fun():number
--   fun(m: integer):integer
--   primary: fun(m: integer, n: integer): integer

local a = math.random()        -- 0 args -> overload fun():number
--    ^ hover: (global) a: number  def: local
local b = math.random(10)      -- 1 arg  -> overload fun(m: integer):integer
--    ^ hover: (global) b: number  def: local

-- tonumber has overloads:
--   fun(e: string, base: integer):integer
--   primary: fun(e: any): number?

local d = tonumber("42")       -- 1 arg  -> primary: number?
--    ^ hover: (global) d: number  def: local
local e = tonumber("FF", 16)   -- 2 args -> overload: integer
--    ^ hover: (global) e: number  def: local

-- table.insert has overloads:
--   fun(list: table, value: any)
--   primary: fun(list: table, pos: integer, value: any)

local t = {}
table.insert(t, "hello")      -- 2 args -> overload (no return)
-- ^ diag: none
table.insert(t, 1, "hello")   -- 3 args -> primary (no return)
-- ^ diag: none

-- empty table {} should be assignable to T[] param (no type-mismatch)
local t2 = {}
tinsert(t2, 42)
-- ^ diag: none

-- table with named fields should match 2-arg overload (not 3-arg primary)
-- Regression: { compressed = true } was rejected by overload compatibility check
-- because T[] (containing TypeVariable) failed structural table comparison,
-- falling through to the 3-arg primary and producing false type-mismatch.
local mixed = { compressed = true }
tinsert(mixed, "hello")
-- ^ diag: none

-- non-table arg to tinsert should still produce type-mismatch
tinsert("not_a_table", 42)
--      ^ diag: type-mismatch

-- @overload with explicit `self` param in method overloads
-- SetPoint has overloads:
--   fun(self, point: FramePoint, relativeTo?: any, ofsx?: number, ofsy?: number)
--   fun(self, point: FramePoint, ofsx?: number, ofsy?: number)
--   primary: fun(point: FramePoint, relativeTo?: any, relativePoint?: FramePoint, offsetX?: uiUnit, offsetY?: uiUnit)
-- The overload `self` param must not be counted against call-site arg count.
local f = CreateFrame("Frame") ---@type Frame
f:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 100, 100)
-- ^ diag: none

-- 3-arg SetPoint: should match primary (point, relativeTo, relativePoint) not the
-- short overload (point, ofsx, ofsy) which expects numbers for args 2 & 3.
f:SetPoint("TOPLEFT", UIParent, "TOPLEFT")
-- ^ diag: none

-- hooksecurefunc has overloads:
--   fun(name: string, hook: function) — the 2-arg form
--   primary: fun(tbl: table, name: string, hook: function)
-- When calling with 3 args, the base signature should match, not the 2-arg overload.
hooksecurefunc(f, "SetPoint", function() end)
-- ^ diag: none

-- @overload on @class: callable table (e.g. LibStub)
-- LibStub is defined as @class with @overload fun(major: `T`, minor?: number): T, number?
---@class CallableTestLib
---@field Version number
local _CTL = {} ---@type CallableTestLib

local ctlib = LibStub("CallableTestLib")
--    ^ hover: (global) ctlib: CallableTestLib {
local ctver = ctlib.Version
--    ^ hover: (global) ctver: number

-- String-literal-based overload dispatch:
-- Same arity, different string literal first param → different return types.
---@overload fun(kind: "number", value: number): number
---@overload fun(kind: "string", value: string): string
---@param kind string
---@param value any
---@return any
local function coerce(kind, value)
    return value
end

local cn = coerce("number", 42)
--    ^ hover: (global) cn: number
local cs = coerce("string", "hello")
--    ^ hover: (global) cs: string

-- Fallback: non-literal arg → first count-matched overload
local kind = "number"
local cf = coerce(kind, 42)
--    ^ hover: (global) cf: number

-- String-literal dispatch enforces handler signature (param count)
---@overload fun(kind: "one", handler: fun(x: number))
---@overload fun(kind: "two", handler: fun(x: number, y: number))
---@param kind string
---@param handler function
local function on(kind, handler) end

on("one", function(x) end)
-- ^ diag: none
on("two", function(x, y) end)
-- ^ diag: none
on("one", function() end)
-- ^ diag: none
on("two", function(x) end)
-- ^ diag: none

-- String-literal dispatch with method self param (inline @type)
---@class ScriptHost
local _SH = {}
---@overload fun(self: ScriptHost, script: "OnDone", handler: fun(self: ScriptHost))
---@overload fun(self: ScriptHost, script: "OnCleanup", handler: fun())
---@param script "OnDone"|"OnCleanup"
---@param handler function
function _SH:SetScript(script, handler) end
local sh = {} ---@type ScriptHost
sh:SetScript("OnDone", function(self) end)
-- ^ diag: none
sh:SetScript("OnCleanup", function() end)
-- ^ diag: none
sh:SetScript("OnDone", function() end)
-- ^ diag: none
sh:SetScript("OnCleanup", function(self) end)
--                         ^ diag: type-mismatch

-- CreateFrame without template: overload returns just T (no Tp in return type).
local eb = CreateFrame("EditBox")
--    ^ hover: (global) eb: EditBox
--         ^ def: external
---@param frame Frame
local function _takeFrame(frame) end
_takeFrame(eb)
-- ^ diag: none

---@class TestMixin
---@field DoSomething fun(self)

-- CreateFrame with template: overload should return T & Tp (intersection type).
local _cfWithTemplate = CreateFrame("Frame", nil, nil, "TestMixin")
--     ^ hover: (global) _cfWithTemplate: Frame & TestMixin
--     ^ diag: none
