-- Test: @overload resolution

-- math.random has overloads:
--   fun():number
--   fun(m: integer):integer
--   primary: fun(m: integer, n: integer): integer

local a = math.random()        -- 0 args -> overload fun():number
--    ^ hover: (local) a: number  def: local
local b = math.random(10)      -- 1 arg  -> overload fun(m: integer):integer
--    ^ hover: (local) b: number  def: local

-- tonumber has overloads:
--   fun(e: string, base: integer):integer
--   primary: fun(e: any): number?

local d = tonumber("42")       -- 1 arg  -> primary: number?
--    ^ hover: (local) d: number?  def: local
local e = tonumber("FF", 16)   -- 2 args -> overload: integer
--    ^ hover: (local) e: number  def: local

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

-- 3-arg SetPoint with offsets only: multiple overloads tie at 0 mismatches
-- (both the (point, relativeTo?, ofsx?, ofsy?) and (point, ofsx?, ofsy?) overloads
-- accept (string, number, number)) — no false positive from primary's relativePoint param.
f:SetPoint("BOTTOMLEFT", 10, 10)
-- ^ diag: none

-- hooksecurefunc has overloads:
--   fun(name: `F`, hook: F) — the 2-arg generic form
--   primary: fun(tbl: table, name: string, hook: function)
-- When calling with 3 args, the base signature should match, not the 2-arg overload.
hooksecurefunc(f, "SetPoint", function() end)
-- ^ diag: none

-- hooksecurefunc 2-arg form: callback params inferred from hooked function.
-- The @generic F: function + @overload fun(name: `F`, hook: F) resolves the
-- string literal to the global function's type, then propagates its param types.
hooksecurefunc("UnitName", function(unit)
    local _hsUnit = unit
    --    ^ hover: (local) _hsUnit: string
end)
-- ^ diag: none

-- hooksecurefunc with unknown function name: params stay untyped, no false diagnostics.
hooksecurefunc("NonExistentFunction", function(x)
-- ^ diag: none
    local _hsUnknown = x
    --    ^ hover: (local) _hsUnknown: ?
end)

-- @overload on @class: callable table (e.g. LibStub)
-- LibStub is defined as @class with @overload fun(major: `T`, silent?: boolean): T, number?
---@class CallableTestLib
---@field Version number
local _CTL = {} ---@type CallableTestLib

local ctlib = LibStub("CallableTestLib")
--    ^ hover: (local) ctlib: CallableTestLib {
local ctver = ctlib.Version
--    ^ hover: (local) ctver: number

local ctsilent = LibStub("CallableTestLib", true)
--    ^ hover: (local) ctsilent: CallableTestLib?  diag: none
print(ctsilent)

local ctget = LibStub:GetLibrary("CallableTestLib", true)
--    ^ hover: (local) ctget: CallableTestLib?  diag: none
print(ctget)

local ctget2 = LibStub:GetLibrary("CallableTestLib")
--    ^ hover: (local) ctget2: CallableTestLib  diag: none
print(ctget2)

-- Unknown library name: backtick generic should resolve to any, not string
local unknownLib = LibStub:GetLibrary("UnknownLib-1.0")
--    ^ hover: (local) unknownLib: any
local unknownLib2 = LibStub("UnknownLib-1.0")
--    ^ hover: (local) unknownLib2: any

-- Backtick generic through a variable (not a string literal)
local libName = "CallableTestLib"
local ctvar = LibStub:GetLibrary(libName)
--    ^ hover: (local) ctvar: CallableTestLib  diag: none
print(ctvar)

-- Unknown library name through a variable: should resolve to any, not string
local unknownName = "UnknownLib-1.0"
local unknownVar = LibStub:GetLibrary(unknownName)
--    ^ hover: (local) unknownVar: any
print(unknownVar)

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
--    ^ hover: (local) cn: number
local cs = coerce("string", "hello")
--    ^ hover: (local) cs: string

-- Fallback: non-literal arg → first count-matched overload
local kind = "number"
local cf = coerce(kind, 42)
--    ^ hover: (local) cf: number

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

-- Overload-based contextual callback typing: inline function params get types from matched overload
sh:SetScript("OnDone", function(self)
    local s = self
--        ^ hover: (local) s: ScriptHost
end)

-- CreateFrame without template: overload returns just T (no Tp in return type).
local eb = CreateFrame("EditBox")
--    ^ hover: (local) eb: EditBox
--         ^ def: external
---@param frame Frame
local function _takeFrame(frame) end
_takeFrame(eb)
-- ^ diag: none

---@class TestMixin
---@field DoSomething fun(self)

-- CreateFrame with template: overload should return T & Tp (intersection type).
local _cfWithTemplate = CreateFrame("Frame", nil, nil, "TestMixin")
--     ^ hover: (local) _cfWithTemplate: Frame & TestMixin
--     ^ diag: none

---@class TestMixin2
---@field DoOtherThing fun(self)

-- CreateFrame with comma-separated templates: should produce intersection of all parts.
local _cfMultiTmpl = CreateFrame("Frame", nil, nil, "TestMixin, TestMixin2")
--     ^ hover: (local) _cfMultiTmpl: Frame & TestMixin & TestMixin2
--     ^ diag: none

---@class TestMixin3
---@field DoThirdThing fun(self)

-- CreateFrame with three comma-separated templates.
local _cfTripleTmpl = CreateFrame("Frame", nil, nil, "TestMixin, TestMixin2, TestMixin3")
--     ^ hover: (local) _cfTripleTmpl: Frame & TestMixin & TestMixin2 & TestMixin3

-- CreateFrame with comma-separated templates (spaces around commas).
local _cfMultiTmpl2 = CreateFrame("Button", nil, nil, "TestMixin , TestMixin2")
--     ^ hover: (local) _cfMultiTmpl2: Button & TestMixin & TestMixin2

-- CreateFrame with an unknown template in a comma list: unknown part is filtered out,
-- known template still contributes its fields.
local _cfPartialUnknown = CreateFrame("Frame", nil, nil, "TestMixin, NonExistent")
--     ^ hover: (local) _cfPartialUnknown: Frame & TestMixin

-- CreateFrame with nil template: should fall back to primary signature (template is optional),
-- not select the template-requiring overload that produces a false positive type-mismatch.
local _cfNilTemplate = CreateFrame("Slider", nil, nil, nil)
--     ^ hover: (local) _cfNilTemplate: Slider
--     ^ diag: none

-- AceGUI:Create() overloads: string-literal dispatch returns specific widget types.
-- Regression: "Button" used to resolve to WoW's Button frame class via the
-- local_class_vars prescan heuristic instead of AceGUIButton.
local _ag = LibStub("AceGUI-3.0")
local _agBtn = _ag:Create("Button")
--     ^ hover: (local) _agBtn: AceGUIButton
local _agHeading = _ag:Create("Heading")
--     ^ hover: (local) _agHeading: AceGUIHeading
local _agFrame = _ag:Create("Frame")
--     ^ hover: (local) _agFrame: AceGUIFrame
_agFrame:SetTitle("Test")
-- ^ diag: none
_agFrame:SetLayout("Flow")
-- ^ diag: none
_agFrame:AddChild(_agBtn)
-- ^ diag: none
_agBtn:SetText("Click Me")
-- ^ diag: none
local _agBtnFrame = _agBtn.frame
--     ^ hover: (local) _agBtnFrame: Frame
local _agBtnUserdata = _agBtn.userdata
--     ^ hover: (local) _agBtnUserdata: table

-- AceAddon-3.0: LibStub backtick resolves class and its methods are accessible.
-- Regression: Ketho's stub had blank `--` comments between @class and the local,
-- breaking annotation extraction. Fixed via local override.
local _aceAddon = LibStub("AceAddon-3.0")
--     ^ hover: (local) _aceAddon: AceAddon-3.0
-- NewAddon with table-first overload: @defclass fires through the overload
-- fun(self, object: table, name: `T`, ...: string): T, creating "TestAddon".
_aceAddon:NewAddon({}, "TestAddon")
-- ^ diag: none
-- GetAddon("TestAddon") uses backtick generic; "TestAddon" was @defclass'd
-- via the table-first overload above, so backtick resolution returns TestAddon.
local _aceAddonByName = _aceAddon:GetAddon("TestAddon")
--     ^ hover: (local) _aceAddonByName: TestAddon
_aceAddonByName:GetName()
-- ^ diag: none
local _aceModule = _aceAddonByName:NewModule("TestModule")
--     ^ hover: (local) _aceModule: TestModule
_aceModule:GetName()
-- ^ diag: none

-- ── need-check-nil suppressed when primary signature param allows nil ───────
-- When an overload's param is non-optional but the primary signature's
-- same-named param IS optional, passing a nil-able value should not fire
-- need-check-nil.

---@overload fun(frameType: string, name?: string, parent?: any, template: string): Frame
---@param frameType string
---@param name? string
---@param parent? any
---@param template? string
---@return Frame
local function createWidget(frameType, name, parent, template) return {} end

local _maybeTemplate = true and "MyTemplate" or nil ---@type string | nil
local _widget = createWidget("Button", "MyBtn", nil, _maybeTemplate)
--                                                    ^ diag: none

-- Regression: varargs overload should match when arg count exceeds
-- non-vararg param count (e.g. AceConsole:Print with optional first Frame param)
---@class TestMixin
local TestMixin = {}

---@overload fun(self: TestMixin, chatframe: Frame, ...: any)
---@param ... any
function TestMixin:Print(...) end

TestMixin:Print("hello", "world")
-- ^ diag: none

-- ── Mixin / CreateFromMixins / CreateAndInitFromMixin ────────────────────

---@class MixinAlpha
---@field alphaMethod fun(self)
local MixinAlpha = {} ---@type MixinAlpha

---@class MixinBeta
---@field betaMethod fun(self)
local MixinBeta = {} ---@type MixinBeta

---@class MixinGamma
---@field gammaMethod fun(self)
local MixinGamma = {} ---@type MixinGamma

---@class MixinDelta
---@field deltaMethod fun(self)
local MixinDelta = {} ---@type MixinDelta

-- Mixin: single mixin
---@type Frame
local _mxFrame = {}
local _mxOne = Mixin(_mxFrame, MixinAlpha)
--     ^ hover: (local) _mxOne: Frame & MixinAlpha
--     ^ diag: none

-- Mixin: two mixins
local _mxTwo = Mixin(_mxFrame, MixinAlpha, MixinBeta)
--     ^ hover: (local) _mxTwo: Frame & MixinAlpha & MixinBeta

-- Mixin: three mixins
local _mxThree = Mixin(_mxFrame, MixinAlpha, MixinBeta, MixinGamma)
--     ^ hover: (local) _mxThree: Frame & MixinAlpha & MixinBeta & MixinGamma

-- CreateFromMixins: hover on the function itself shows ...M (not ......M)
local _cfmOne = CreateFromMixins(MixinAlpha)
--              ^ hover: (global) function CreateFromMixins(...M)\n  -> & ...M
--     ^ hover: (local) _cfmOne: MixinAlpha
--     ^ diag: none

-- CreateFromMixins: two mixins
local _cfmTwo = CreateFromMixins(MixinAlpha, MixinBeta)
--     ^ hover: (local) _cfmTwo: MixinAlpha & MixinBeta

-- CreateFromMixins: three mixins
local _cfmThree = CreateFromMixins(MixinAlpha, MixinBeta, MixinGamma)
--     ^ hover: (local) _cfmThree: MixinAlpha & MixinBeta & MixinGamma

-- CreateAndInitFromMixin: returns the mixin type
local _caimOne = CreateAndInitFromMixin(MixinAlpha)
--     ^ hover: (local) _caimOne: MixinAlpha
--     ^ diag: none

-- Field access on Mixin result: hover/def on intersection method
local _mxAccess = Mixin(_mxFrame, MixinAlpha)
_mxAccess:alphaMethod()
--        ^ hover: (method) function MixinAlpha:alphaMethod()  def: local 317:1  diag: none

-- Field access on CreateFromMixins intersection
local _cfmAccess = CreateFromMixins(MixinAlpha, MixinBeta)
_cfmAccess:alphaMethod()
--         ^ hover: (method) function MixinAlpha:alphaMethod()  diag: none
_cfmAccess:betaMethod()
--         ^ hover: (method) function MixinBeta:betaMethod()  diag: none

-- Mixin: four mixins (proves variadic generics, no 3-mixin cap)
local _mxFour = Mixin(_mxFrame, MixinAlpha, MixinBeta, MixinGamma, MixinDelta)
--     ^ hover: (local) _mxFour: Frame & MixinAlpha & MixinBeta & MixinGamma & MixinDelta
_mxFour:deltaMethod()
-- ^ diag: none

-- CreateFromMixins: four mixins
local _cfmFour = CreateFromMixins(MixinAlpha, MixinBeta, MixinGamma, MixinDelta)
--     ^ hover: (local) _cfmFour: MixinAlpha & MixinBeta & MixinGamma & MixinDelta

-- @narrows-arg: bare Mixin() without capturing return narrows the argument
---@type Frame
local _naFrame = {}
Mixin(_naFrame, MixinAlpha)
local _naAfter = _naFrame
--     ^ hover: (local) _naAfter: Frame & MixinAlpha
_naFrame:alphaMethod()
--       ^ hover: (method) function MixinAlpha:alphaMethod()  diag: none

-- @narrows-arg: multiple mixins in bare call
---@type Frame
local _naFrame2 = {}
Mixin(_naFrame2, MixinAlpha, MixinBeta)
_naFrame2:alphaMethod()
--        ^ hover: (method) function MixinAlpha:alphaMethod()  diag: none
_naFrame2:betaMethod()
--        ^ hover: (method) function MixinBeta:betaMethod()  diag: none

-- GetCursorInfo has 8 zero-param overloads with different return types.
-- When no overload can be discriminated (all match equally), return types
-- should be the union across all overloads — not just the first overload.
local cursorType = GetCursorInfo()
--    ^ hover: (local) cursorType: "item" | "spell" | "petaction" | (5 more)
local _, cursorId = GetCursorInfo()
--       ^ hover: (local) cursorId: number | string
local _, _, cursorItem = GetCursorInfo()
--          ^ hover: (local) cursorItem: string | number | nil
