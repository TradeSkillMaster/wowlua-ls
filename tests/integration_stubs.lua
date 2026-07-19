---@diagnostic disable: inject-field, shadowed-local, undefined-global, unused-function, unused-local
-- wowlua_ls integration test (with stubs)
-- Requires: --with-stubs

local t = setmetatable({}, {})
--        ^ hover: (global) function setmetatable(tbl: table, metatable?: metatable | table)  def: external

local s = type("hello")
--        ^ hover: (global) function type(v: any)  def: external

local ok = pcall(print, "hi")
--         ^ hover: (global) function pcall(f: F, ...: params<F>)  def: external

-- pcall multi-return unpacking
local pcallOk, pcallErr = pcall(error, "boom")
--    ^ hover: (local) pcallOk: boolean  def: local

-- xpcall multi-return unpacking
local xpOk, xpErr = xpcall(error, print, "boom")
--    ^ hover: (local) xpOk: boolean  def: local

-- ── pcall generic return type projection ─────────────────────────────────────

-- pcall with single-return function
---@param name string
---@return string
local function greetPcall(name) return "Hi " .. name end

local pOk1, pVal1 = pcall(greetPcall, "world")
--    ^ hover: (local) pOk1: boolean
--            ^ hover: (local) pVal1: string

-- pcall with multi-return function
---@return string name
---@return number level
---@return boolean active
local function getInfoPcall() return "x", 1, true end

local pOk2, pName, pLevel, pActive = pcall(getInfoPcall)
--    ^ hover: (local) pOk2: boolean
--            ^ hover: (local) pName: string
--                     ^ hover: (local) pLevel: number?
--                              ^ hover: (local) pActive: boolean?

-- pcall validates argument types via params<F>
pcall(greetPcall, "ok")
pcall(greetPcall, 42)
--                ^ diag: type-mismatch

-- pcallwithenv generic return type projection
---@return number result
local function computePcall() return 42 end

local peOk, peVal = pcallwithenv(computePcall, {})
--    ^ hover: (local) peOk: boolean
--            ^ hover: (local) peVal: number | string

-- pcall with non-string return: second value is `T | string` (error case)
---@return Frame
local function makeFramePcall() return CreateFrame("Frame") end

local pfOk, pfResult = pcall(makeFramePcall)
--    ^ hover: (local) pfOk: boolean
--            ^ hover: (local) pfResult: Frame | string

-- pcall narrowing: `if ok then` narrows to success case
local nOk, nVal = pcall(computePcall)
if nOk then
    local _ = nVal
    --        ^ hover: (local) nVal: number
end

-- pcall narrowing: else branch narrows to error case
local nOk2, nVal2 = pcall(makeFramePcall)
if nOk2 then
    local _ = nVal2
    --        ^ hover: (local) nVal2: Frame
else
    local _ = nVal2
    --        ^ hover: (local) nVal2: string
end

-- pcall narrowing: early-exit pattern
local nOk3, nVal3 = pcall(computePcall)
if not nOk3 then return end
local _ = nVal3
--        ^ hover: (local) nVal3: number

-- pcall with zero-return function: success gives nil, error gives string
local function voidPcallFn() end
local vOk, vVal = pcall(voidPcallFn)
--    ^ hover: (local) vOk: boolean
--          ^ hover: (local) vVal: string?

-- pcall with vararg-return function
---@return ...number
local function varargPcallFn() return 1, 2, 3 end

local vrOk, vrA, vrB = pcall(varargPcallFn)
--    ^ hover: (local) vrOk: boolean
--            ^ hover: (local) vrA: number | string
--                 ^ hover: (local) vrB: number?

---@type Frame
local f = nil
--    ^ hover: (local) f: Frame {  def: local  typedef: external

-- Go-to-definition on external @class @field annotations (path must be relativized, not absolute)
---@type CurrencyInfo
local ci = nil
local _ciName = ci.name
--                 ^ hover: (field) name: string  def: external vendor/

-- Compat globals (local alias → field ref, e.g. `local str = string; strmatch = str.match`)
local a = strmatch("hello", "(%w+)")
--    ^ hover: (local) a: string?
--        ^ hover: (global) function strmatch(s: string | number, pattern: string | number, init?: integer)  def: external
local a1, a2, a3 = strmatch("2024-01-15", "(%d+)-(%d+)-(%d+)")
--    ^ hover: (local) a1: string?
--        ^ hover: (local) a2: string?
--            ^ hover: (local) a3: string?

local b = strlen("hi")
--    ^ hover: (local) b: number
--        ^ hover: (global) function strlen(s: string | number)  def: external

local c = tinsert
--        ^ hover: (global) function tinsert(list: T[], pos: integer, value: T)  def: external

local d = floor(3.14)
--    ^ hover: (local) d: number
--        ^ hover: (global) function floor(x: number)  def: external

local e = strsub("hello", 1, 3)
--    ^ hover: (local) e: string
--        ^ hover: (global) function strsub(s: string | number, i: integer, j?: integer)  def: external

-- External function call return types
local sm = setmetatable({}, {})
--    ^ hover: (local) sm: table

local ts = tostring(42)
--    ^ hover: (local) ts: string

-- unpack with @return ...T propagates element type to all return positions
local _uArr = { 10, 20, 30 }
local _u1, _u2, _u3 = unpack(_uArr)
local _ = _u1
--        ^ hover: (local) _u1: number
local _ = _u2
--        ^ hover: (local) _u2: number
local _ = _u3
--        ^ hover: (local) _u3: number

-- Ternary pattern with strmatch (returns string?)
local isMatch = strmatch("hello", "(%w+)") and true or false
--    ^ hover: (local) isMatch: boolean

-- Sibling narrowing: checking one strmatch capture narrows the others
local m1, m2 = strmatch("2024-01-15", "(%d+)-(%d+)")
if m1 then
    local _ = m1
    --        ^ hover: (local) m1: string
    local _ = m2
    --        ^ hover: (local) m2: string
end

-- Sibling narrowing: early exit pattern
local m3, m4, m5 = strmatch("a-b-c", "(%a)-(%a)-(%a)")
if not m3 then return end
local _ = m3
--        ^ hover: (local) m3: string
local _ = m4
--        ^ hover: (local) m4: string
local _ = m5
--        ^ hover: (local) m5: string

-- A local function returning {} should not be typed as a class just because
-- its string argument happens to match a class name.
local function LibStub(name) return {} end
local myFrame = LibStub("Frame")
--    ^ hover: (local) myFrame: table  def: local

-- Global class instances (e.g. UIParent) should be visible as globals
local p = UIParent
--        ^ hover: (global) UIParent: UIParent {  def: external

-- Global strings show their literal value on hover
local msg = ACCEPT
--           ^ hover: (global) ACCEPT: string = "Accept"  def: external

-- Global numbers show their literal value on hover
local expansion = LE_EXPANSION_CLASSIC
--                 ^ hover: (global) LE_EXPANSION_CLASSIC: number = 0  def: external

-- ── tinsert type checking against typed arrays ──────────────────────────────

---@type string[]
local names = {}
tinsert(names, "hello")
tinsert(names, 42)
--             ^ diag: type-mismatch

-- ── for-in loop iterator variable types (pairs/ipairs) ──────────────────────

---@type table<string, number>
local kvTable = {}
for pk, pv in pairs(kvTable) do
    local _usepk = pk
--                 ^ hover: (local) pk: string
    local _usepv = pv
--                 ^ hover: (local) pv: number
end

---@type number[]
local numArr = {}
for ii, iv in ipairs(numArr) do
    local _useii = ii
--                 ^ hover: (local) ii: number
    local _useiv = iv
--                 ^ hover: (local) iv: number
end

-- Nested array: ipairs over string[][] should yield string[], not string[] | string[]
local nestedArr = {{"a", "b"}, {"c", "d"}}
for ni, nv in ipairs(nestedArr) do
    local _useni = ni
--                 ^ hover: (local) ni: number
    local _usenv = nv
--                 ^ hover: (local) nv: string[]
end

-- ── for-in with `next, tbl` (multi-expression generic for protocol) ─────────

for nk, nv in next, kvTable do
    local _usenk = nk
--                 ^ hover: (local) nk: string
    local _usenv = nv
--                 ^ hover: (local) nv: number
end

-- @class inheriting from table<K,V>: for-in loop types
---@class NamedColorMap : table<string, string>
---@field default string

---@type NamedColorMap
local colorMap = {}
for cmk, cmv in pairs(colorMap) do
    local _usecmk = cmk
--                  ^ hover: (local) cmk: string
    local _usecmv = cmv
--                  ^ hover: (local) cmv: string
end

-- Multiple parents: class + table<K,V> iteration
---@class StubBase
---@field id number

---@class StubMap : StubBase, table<string, number>

---@type StubMap
local stubMap = {}
for smk, smv in pairs(stubMap) do
    local _usesmk = smk
--                  ^ hover: (local) smk: string
    local _usesmv = smv
--                  ^ hover: (local) smv: number
end

-- @class with non-string key_type + fields: `next` should use key_type, not hardcode string
---@class NumKeyMap : table<number, number>
---@field label string

---@type NumKeyMap
local numKeyMap = {}
for nkmk, nkmv in next, numKeyMap do
    local _usenkmk = nkmk
--                   ^ hover: (local) nkmk: number
    local _usenkmv = nkmv
--                   ^ hover: (local) nkmv: number
end

-- @alias table<K,V>: `next` should also use key_type/value_type
---@alias NumAlias table<number, boolean>

---@type NumAlias
local numAlias = {}
for nak, nav in next, numAlias do
    local _usenak = nak
--                  ^ hover: (local) nak: number
    local _usenav = nav
--                  ^ hover: (local) nav: boolean
end

-- @class with non-string key_type + fields: `pairs` should also use key_type
for nkmpk, nkmpv in pairs(numKeyMap) do
    local _usenkmP = nkmpk
--                   ^ hover: (local) nkmpk: number
    local _usenkmPv = nkmpv
--                    ^ hover: (local) nkmpv: number
end

-- ── Dot-calling colon-defined stub methods (explicit self) ──────────────────

---@type Frame
local myFrame2 = nil
GameTooltip.Show(myFrame2)

-- ── Keyword-named parameters (e.g. `repeat`) should still be parsed ─────────

DoTradeSkill(1, 5)
-- ^ hover: (global) function DoTradeSkill(index: number, repeat: number)  def: external

-- ── setfenv: `async fun(...)` in union should parse as function ─────────

local function myFunc() end
setfenv(myFunc, {})

-- ── coroutine library stubs ─────────────────────────────────────────────

local co = coroutine.create(function() end)
--    ^ hover: (local) co: thread

local cok, cval = coroutine.resume(co)
--    ^ hover: (local) cok: boolean

local cstatus = coroutine.status(co)
--    ^ hover: (local) cstatus: string

local cwrap = coroutine.wrap(function() end)
--    ^ hover: (local) cwrap: function

local cyieldable = coroutine.isyieldable()
--    ^ hover: (local) cyieldable: boolean

-- ── _G bracket/dot access as global variable access ──────────────────

-- _G bracket write with string literal creates a global
_G["TestGlobalFromG"] = 42
local _g_a = TestGlobalFromG
--    ^ hover: (local) _g_a: number

-- _G bracket read resolves the global
local _g_b = _G["TestGlobalFromG"]
--    ^ hover: (local) _g_b: number

-- _G bracket with variable key should not emit diagnostics
local _g_dyn_name = "Dynamic"
_G[_g_dyn_name] = true

-- _G dot access reads resolve to globals
local _g_c = _G.print
--    ^ hover: (local) function _g_c(...: any)

-- _G dot access on table globals (no undefined-field)
---@diagnostic enable: unused-local
local gStr = _G.string
--    ^ diag: unused-local
local gTbl = _G.table
--    ^ diag: unused-local
local gCf = _G.CreateFrame
--    ^ diag: unused-local

-- _G bracket read with string literal for known globals (no undefined-field)
local gPairs = _G["pairs"]
--    ^ diag: unused-local

-- Indirect _G access: local aliasing _G resolves globals
local _g_indirect = _G
local _g_d = _g_indirect.print
--    ^ hover: (local) function _g_d(...: any)

-- Indirect _G access on table/function globals (no undefined-field)
local gIndStr = _g_indirect.string
--    ^ diag: unused-local
local gIndCf = _g_indirect.CreateFrame
--    ^ diag: unused-local
local gIndType = _g_indirect.type
--    ^ diag: unused-local
---@diagnostic disable: unused-local

-- Definition on indirect _G field resolves to the global
local _g_e = _g_indirect.type
--                        ^ def: external

-- _G.X hover for user-defined globals
---@class _GTestClass
---@field Val string
local _g_tbl = { Val = "hello" }
_G.GTestGlobal = _g_tbl
local _g_f = _G.GTestGlobal
--               ^ hover: (global) GTestGlobal: _GTestClass  def: local
-- _G.X.Y field access hover and completions through _G redirect
local _g_g = _G.GTestGlobal.Val
--                           ^ hover: (field) Val: string  comp: Val
-- _G.X.Y resolved type
local _g_h = _G.GTestGlobal.Val
--    ^ hover: (local) _g_h: string
-- _G.X hover on stub globals (external symbols without type_source)
local _g_i = _G.pairs
--               ^ hover: (global) function pairs  def: external

-- ── Go-to-definition on annotation type names ────────────────────────────────

-- Annotation class/alias names should resolve via go-to-definition
---@param f Frame
--          ^ def: external
---@type FrameType
--       ^ def: external
function _annot_def_test(f) end

-- ── CreateFrame with template produces intersection type ─────────────────────

-- CreateFrame("Frame", nil, nil, "BackdropTemplate") returns Frame & BackdropTemplate
local _bdFrame = CreateFrame("Frame", nil, nil, "BackdropTemplate")
--    ^ hover: (local) _bdFrame: BackdropTemplate {

-- CreateFrame("Frame", nil, parent) with 3 args should resolve generics, not show T & Tp | T
---@type Frame
local _parentFrame
local _cfNoTpl = CreateFrame("Frame", nil, _parentFrame)
--    ^ hover: (local) _cfNoTpl: Frame

-- ── Classic XML frame globals get their @type annotation (not nil) ───────────

-- Frame globals extracted from XML (e.g. `---@type Button\nCraftCreateButton = nil`)
-- should resolve to the annotated type, not nil.
local _craftBtn = CraftCreateButton
--    ^ hover: (local) _craftBtn: CraftCreateButtonType {
--                ^ hover: (global) CraftCreateButton: CraftCreateButtonType {  def: external

-- ── WoW Enum types (Enum.X) accept plain number ───────────────────────────
local _power = UnitPower("player", 0)
local _power2 = UnitPower("player", Enum.PowerType.Mana)

-- ── AceGUI:Create() type narrowing ──────────────────────────────────────────

---@type AceGUI-3.0
local AceGUI

local aceBtn = AceGUI:Create("Button")
--    ^ hover: (local) aceBtn: AceGUIButton {
aceBtn:SetText("OK")
--     ^ hover: (method) function AceGUIButton:SetText(text: string)
aceBtn:SetDisabled(true)
--     ^ hover: (method) function AceGUIButton:SetDisabled(flag: boolean)
aceBtn:SetCallback("OnClick", function() end)
--     ^ hover: (method) function AceGUIButton:SetCallback(name: string, func: function)
aceBtn:SetDisabled(true)

local aceDrop = AceGUI:Create("Dropdown")
--    ^ hover: (local) aceDrop: AceGUIDropdown {
aceDrop:SetLabel("Pick one")
--      ^ hover: (method) function AceGUIDropdown:SetLabel(text: string)
aceDrop:SetList({})
--      ^ hover: (method) function AceGUIDropdown:SetList(list: table<any, string>, order?: any[])
aceDrop:SetValue("foo")
aceDrop:SetMultiselect(false)

local aceFrame = AceGUI:Create("Frame")
--    ^ hover: (local) aceFrame: AceGUIFrame {
aceFrame:SetTitle("My Window")
--       ^ hover: (method) function AceGUIFrame:SetTitle(text: string)
aceFrame:SetStatusText("Ready")
--       ^ hover: (method) function AceGUIFrame:SetStatusText(text: string)
aceFrame:AddChild(aceBtn)
--       ^ hover: (method) function AceGUIFrame:AddChild(widget: AceGUIWidget, beforeWidget?: AceGUIWidget)
aceFrame:SetLayout("Flow")

local aceSlider = AceGUI:Create("Slider")
--    ^ hover: (local) aceSlider: AceGUISlider {
aceSlider:SetSliderValues(0, 100, 1)
--        ^ hover: (method) function AceGUISlider:SetSliderValues(min?: number, max?: number, step?: number)

local aceTree = AceGUI:Create("TreeGroup")
--    ^ hover: (local) aceTree: AceGUITreeGroup {
aceTree:SetTree({})
--      ^ hover: (method) function AceGUITreeGroup:SetTree(tree: table, filter?: boolean)
aceTree:SetStatusTable({})

-- debugstack: all params optional
local _ds1 = debugstack()
--            ^ hover: (global) function debugstack(\ncoroutine: thread,\nstart?: number,\ncount1?: number,\ncount2?: number\n)\n-> string\nfunction debugstack(start?: number, count1?: number, count2?: number)\n-> string  def: external
local _ds2 = debugstack(2)
local _ds3 = debugstack(2, 10)
local _ds4 = debugstack(2, 10, 5)

-- ── ipairs over class array fields ──────────────────────────────────────

---@class IpairsItem
---@field id number

---@class IpairsContainer
---@field items IpairsItem[]

---@param c IpairsContainer
local function testIpairsNonOptional(c)
    for _, curr in ipairs(c.items) do
        local x = curr
        --    ^ hover: (local) x: IpairsItem {  def: local
    end
end

---@class IpairsOptContainer
---@field items? IpairsItem[]

---@param c IpairsOptContainer
local function testIpairsOptional(c)
    for _, curr in ipairs(c.items) do
        local x = curr
        --    ^ hover: (local) x: IpairsItem {  def: local
    end
end

---@class IpairsMixed
---@field tags? string[]
---@field scores number[]

---@param m IpairsMixed
local function testIpairsMixed(m)
    for _, tag in ipairs(m.tags) do
        local t = tag
        --    ^ hover: (local) t: string  def: local
    end
    for _, score in ipairs(m.scores) do
        local s = score
        --    ^ hover: (local) s: number  def: local
    end
end

-- ── Addon namespace: select(2, ...) should NOT inherit FrameXML stubs ───
local ns = select(2, ...)
--    ^ hover: (local) ns: table
local _, ns2 = ...
--       ^ hover: (local) ns2: table

-- ── select(2, ...) inside a function is the function's own varargs, NOT the ──
-- file-level (addonName, addonTable) namespace. The addon-table special case is
-- file-scope only. (Regression: nsInFunc was wrongly typed `table`.)
---@param ... any
local function _nsVarargScope(...)
    local nsInFunc = select(2, ...)
    --    ^ hover: (local) nsInFunc: ?
    return nsInFunc
end
_nsVarargScope()

-- Colon-method definition on CreateFrame result (was false positive: undefined-field)
do
    local evtFrame = CreateFrame('Frame')
    function evtFrame:OnEvent(e, ...)
    end
    evtFrame:SetScript('OnEvent', evtFrame.OnEvent)
    --                                     ^ hover: (field) function Frame.OnEvent(self: Frame, e, ...)

    -- Dot-method assignment on CreateFrame result
    evtFrame.OnClick = function(self) end
    local _ref = evtFrame.OnClick
    --                    ^ hover: (field) function Frame.OnClick(self)
end

-- ── and-guarded field assignments resolve through StripNil/StripFalsy ──

---@class StripTestAPI
---@field DoThing fun(x: number): string

---@type StripTestAPI?
local MaybeAPI

local directLocal = MaybeAPI and MaybeAPI.DoThing
--    ^ hover: (local) directLocal: (fun(x: number): string)?

local tbl = {}
tbl.guardedField = MaybeAPI and MaybeAPI.DoThing
--  ^ hover: (field) guardedField: (fun(x: number): string)?

tbl.directField = MaybeAPI and MaybeAPI.DoThing
--  ^ hover: (field) directField: (fun(x: number): string)?

local tbl2 = {}
tbl2.orField = MaybeAPI or "fallback"
--   ^ hover: (field) orField: StripTestAPI | string

-- Chained and-guard: both sides contribute to narrowing
local tbl3 = {}
tbl3.chained = MaybeAPI and MaybeAPI.DoThing and MaybeAPI.DoThing
--   ^ hover: (field) chained: (fun(x: number): string)?

-- Cast expressions on fields (@as)
local tbl4 = {}
tbl4.castField = "hello" --[[@as number]]
--   ^ hover: (field) castField: number

-- ── CreateFrame overlay fields accessed through class field indirection ──────
do
    ---@class StubOverlayHost
    local Host = {}

    local myFrame = CreateFrame('Frame')
    myFrame.customData = 42
    myFrame.handler = function(self) end
    Host.display = myFrame

    -- Direct access on the local should work (already tested above)
    local _cd = myFrame.customData
    --                  ^ hover: (field) customData: number

    -- Access through class field indirection
    local retrieved = Host.display
    local _cd2 = retrieved.customData
    --                     ^ hover: (field) customData: number
    local _h = retrieved.handler
    --                   ^ hover: (field) function Frame.handler(self)
end

-- Event string hover on WoW API methods
---@type Frame
local _evFrame = nil
_evFrame:RegisterEvent("PLAYER_LOGIN")
--                       ^ hover: (event) PLAYER_LOGIN  doc: warcraft.wiki.gg/wiki/PLAYER_LOGIN

_evFrame:RegisterEvent("PLAYER_LOGOUT")
--                       ^ hover: (event) PLAYER_LOGOUT  doc: warcraft.wiki.gg/wiki/PLAYER_LOGOUT

_evFrame:RegisterEvent("ENCOUNTER_END")
--                       ^ hover: (event) ENCOUNTER_END →  doc: warcraft.wiki.gg/wiki/ENCOUNTER_END

_evFrame:RegisterEvent("PLAYER_ENTERING_WORLD")
--                       ^ hover: (event) PLAYER_ENTERING_WORLD →  doc: warcraft.wiki.gg/wiki/PLAYER_ENTERING_WORLD

_evFrame:RegisterEvent("NONEXISTENT_EVENT_XYZ")
--                       ^ hover: <missing>

-- Event name completions are filtered by typed prefix (regression: large sets
-- were truncated by the 100-item cap, hiding events past 'A')
_evFrame:RegisterEvent("NAME_PLATE_UNIT_ADDED")
--                         ^ comp: NAME_PLATE_CREATED, NAME_PLATE_UNIT_ADDED, NAME_PLATE_UNIT_BEHIND_CAMERA_CHANGED, NAME_PLATE_UNIT_REMOVED

-- UnitToken string argument: the `UnitToken` alias is defined as `string` with
-- `---|"player"`-style completion values. Its resolved type collapses to bare
-- `string` (so any string is accepted, no type-mismatch), but the enumerated unit
-- tokens are preserved and offered as completions inside the argument string.
-- Regression: this used to fall through to scope completion, offering unrelated
-- WoW globals (PLAY, PLAYER, PLAYER_*, …) inside the string instead.
UnitOnTaxi("")
--          ^ comp: player, target, focus, mouseover, pet, vehicle, npc, questnpc, none, party1, raid1, arena1, boss1, nameplate1, anyenemy, anyfriend, anyinteract, softenemy, softfriend, softinteract

-- Event hover through field chain (regression: manual chain resolution could fail)
do
    ---@class _EvNs
    local ns = {}
    ns.frame = CreateFrame("Frame")
    ns.frame:RegisterEvent("PLAYER_LOGIN")
    --                       ^ hover: (event) PLAYER_LOGIN  doc: warcraft.wiki.gg/wiki/PLAYER_LOGIN
end

-- EventRegistry inherits CallbackRegistryMixin methods and types event as FrameEvent
EventRegistry:RegisterCallback("PLAYER_REPORT_SUBMITTED", function() end)
--                                ^ hover: (event) PLAYER_REPORT_SUBMITTED → invitedByGUID: WOWGUID  doc: warcraft.wiki.gg/wiki/PLAYER_REPORT_SUBMITTED
EventRegistry:RegisterFrameEvent("PLAYER_LOGIN")
--                                  ^ hover: (event) PLAYER_LOGIN  doc: warcraft.wiki.gg/wiki/PLAYER_LOGIN

-- ── SetScript handler contextual typing from overload ──
do
    local sf = CreateFrame('Frame')
    sf:SetScript("OnEvent", function(self, event, ...)
        local s = self
--            ^ hover: (local) s: Frame
        local e = event
--            ^ hover: (local) e: FrameEvent
    end)
    sf:SetScript("OnUpdate", function(self, elapsed)
        local dt = elapsed
--            ^ hover: (local) dt: number
    end)
    sf:SetScript("OnShow", function(self)
        local s = self
--            ^ hover: (local) s: Frame
    end)
end

-- ── SetScript handler with underscore params (no false positive) ──
do
    local uf = CreateFrame('Frame')
    uf:SetScript("OnEvent", function(_, _, unit)
        local u = unit
    end)
end

-- ── HookScript handler contextual typing from overload ──
do
    local hf = CreateFrame('Frame')
    hf:HookScript("OnEvent", function(self, event, ...)
        local s = self
--            ^ hover: (local) s: Frame
        local e = event
--            ^ hover: (local) e: FrameEvent
    end)
    hf:HookScript("OnUpdate", function(self, elapsed)
        local dt = elapsed
--            ^ hover: (local) dt: number
    end)
    hf:HookScript("OnShow", function(self)
        local s = self
--            ^ hover: (local) s: Frame
    end)
end

-- ── WorldFrame inherits from Frame (no type-mismatch on Frame params) ─────
do
    local tt = CreateFrame("GameTooltip", nil, UIParent, "GameTooltipTemplate")
    tt:SetOwner(_G.WorldFrame, "ANCHOR_NONE")
end

-- ── Regression: class-eq narrowing with external symbol must not crash ────
-- When a global stub symbol (external, idx >= EXT_BASE) appears as a bare
-- name in `x == CLASS_EXPR`, the deferred class-eq narrowing path must
-- skip it rather than indexing into the local symbols array.
do
    ---@class StubEqTestCode
    local _StubEqTestCode = {}
    local CODES = {
        OK = nil, ---@type StubEqTestCode
    }
    if UIParent == CODES.OK then
        -- UIParent is external; this must not panic
        local _ = UIParent
        --        ^ hover: (global) UIParent: UIParent {  def: external
    end
end

-- ── or-chained function with unresolved LHS resolves to RHS type ──────
-- Regression: `local f = UnknownGlobal or C_AddOns.GetAddOnMetadata` was typed `?`
do
    local GetMetadata = UnknownMetadataGlobal or C_AddOns.GetAddOnMetadata
    --    ^ hover: (local) function GetMetadata(name: uiAddon, variable: string)
    local ver = GetMetadata("addon", "Version")
    --    ^ hover: (local) ver: string?

    -- Regression (stub discovery): a wiki-documented bare global that has a C_*
    -- namespaced twin (GetAddOnMetadata vs C_AddOns.GetAddOnMetadata) must be
    -- recovered as a real global, not dropped as a namespace-only alias.
    local _bareMeta = GetAddOnMetadata
    --                ^ def: external

    -- Truthy LHS with unresolved RHS: use LHS type
    local f2 = C_AddOns.GetAddOnMetadata or UNKNOWN_FUNC
    --    ^ hover: (local) function f2(name: uiAddon, variable: string)

    -- Non-truthy LHS (boolean) with unresolved RHS: stays unresolved
    -- because the boolean could be false, making RHS relevant
    ---@type boolean
    local flag
    local f3 = flag or UNKNOWN_FUNC
    --    ^ hover: (local) f3: ?
end

-- local x = x: RHS should resolve to the global, not the new local
local print = print
--    ^ hover: (local) function print(...: any)  def: local
--            ^ hover: (global) function print(...: any)  def: external

-- ── newproxy() returns userdata ───────────────────────────────────────────────

local _np1 = newproxy()
--    ^ hover: (local) _np1: userdata
--            ^ hover: (global) function newproxy(useMt?: boolean)  def: external

local _np2 = newproxy(false)
--    ^ hover: (local) _np2: userdata

local _np3 = newproxy(true)
--    ^ hover: (local) _np3: userdata

-- ── select() with returns<F, index> projection ──────────────────────────────

-- select(N, func()) projects to the Nth return type of func
do
    local name = select(1, GetSpellInfo(1))
    --    ^ hover: (local) name: string

    local icon = select(3, GetSpellInfo(1))
    --    ^ hover: (local) icon: number

    -- Local multi-return function
    ---@return string
    ---@return number
    ---@return boolean
    ---@diagnostic disable-next-line: missing-return
    local function multiRet() end

    local s1 = select(1, multiRet())
    --    ^ hover: (local) s1: string

    local s2 = select(2, multiRet())
    --    ^ hover: (local) s2: number

    local s3 = select(3, multiRet())
    --    ^ hover: (local) s3: boolean

    -- select("#", ...) overload returns integer (number)
    local count = select("#", multiRet())
    --    ^ hover: (local) count: number

    local count2 = select("#", GetSpellInfo(1))
    --    ^ hover: (local) count2: number

    -- select with returns<F> should not produce false type-mismatch
    -- on the vararg arguments (regression: projected_f_idx was checking
    -- against F's parameters instead of being skipped for returns<F>)
    local _selRet1 = select(2, GetSpellInfo(1))
    local _selRet2 = select(3, GetSpellInfo(1))

    -- strsplit returns vararg strings; select(N, strsplit(...)) should be string, not nil
    local piece1 = select(1, strsplit(":", "a:b:c"))
    --    ^ hover: (local) piece1: string
    local piece3 = select(3, strsplit(":", "a:b:c"))
    --    ^ hover: (local) piece3: string
    local _mid = strsplit(",", piece3)

    -- Table constructor from strsplit + bracket mutation: hover should show the
    -- initial constructor type (string[]), not the post-mutation type (number[]).
    local data = "1,2,3"
    local parts = {strsplit(",", data)}
    --    ^ hover: (local) parts: string[]
    for i = 1, #parts do
        parts[i] = tonumber(parts[i])
    end
end

-- ── FrameXML globals: type inference from RHS expressions ───────────────────

-- Font objects created by CreateFont() → return type (Font)
local _font1 = GameFontNormal
--    ^ hover: (local) _font1: Font {
local _font2 = GameFontHighlightSmall
--    ^ hover: (local) _font2: Font {

-- Color constants created by CreateColor() → return type (colorRGBA)
local _color1 = HIGHLIGHT_FONT_COLOR
--    ^ hover: (local) _color1: colorRGBA {

-- DEFAULT_CHAT_FRAME = ChatFrame1 → type of referenced global
local _dcf = DEFAULT_CHAT_FRAME
--    ^ hover: (local) _dcf: ChatFrame1 {

-- Table constructor field key should not resolve to a same-named global
local _itemClasses = {
	ACCEPT = true,
--  ^ hover: (field) ACCEPT: true  def: local
}

-- Enum constant references → number (via @enum class enum_kind)
local _bag = BACKPACK_CONTAINER
--    ^ hover: (local) _bag: number

-- ── @class table fields from function call return types ─────────────────────

-- Fields assigned via function calls in a @class table constructor should
-- resolve to the function's return type, not `any`.
---@class _ClassWithCallFields
local _cwcf = {
    proxy = newproxy(false),
    --  ^ hover: (field) proxy: userdata
    label = tostring(42),
    --  ^ hover: (field) label: string
    count = tonumber("5"),
    --  ^ hover: (field) count: number?
}

-- GetInboxInvoiceInfo: all 12 returns from wiki should be present
local _invType, _invItem, _invPlayer, _invBid, _invBuyout, _invDeposit, _invConsign, _invDelay, _invHour, _invMin, _invCount, _invCommerce = GetInboxInvoiceInfo(1)
--    ^ hover: (local) _invType: string?  def: local
local _invDelay2 = select(8, GetInboxInvoiceInfo(1))
--    ^ hover: (local) _invDelay2: number  def: local
local _invCommerce2 = select(12, GetInboxInvoiceInfo(1))
--    ^ hover: (local) _invCommerce2: boolean  def: local

-- GetProfessionInfo: all 11 returns from wiki should be present
local _pName, _pIcon, _pSkill, _pMax, _pAbils, _pOff, _pLine, _pMod, _pSpecIdx, _pSpecOff, _pLineName = GetProfessionInfo(1)
--    ^ hover: (local) _pName: string  def: local
local _pLineName2 = select(11, GetProfessionInfo(1))
--    ^ hover: (local) _pLineName2: string  def: local

-- ── next() on bare table: unresolved generic should not trigger nil-index ──

local function _loadData()
    local result = { fieldLookup = {}, itemLookup = {} }
    return result
end

local function _mergeData(tbl)
    local loadedData = _loadData()
    local existing = tbl[next(loadedData.fieldLookup)]
    local _ = existing
end
local _ = _mergeData

-- Widget method wiki-enriched return types

-- GameTooltip:GetItem returns (string, string) from wiki name-inference
local itemName, itemLink = GameTooltip:GetItem()
--    ^ hover: (local) itemName: string
--              ^ hover: (local) itemLink: string

-- GameTooltip:GetSpell returns (string, number) from wiki name-inference
local spellName, spellID = GameTooltip:GetSpell()
--    ^ hover: (local) spellName: string
--               ^ hover: (local) spellID: number

-- GameTooltip:GetUnit returns (string, string) from wiki structured types
local unitName, unitId = GameTooltip:GetUnit()
--    ^ hover: (local) unitName: string
--              ^ hover: (local) unitId: string

-- ── Mixin generic type variables don't leak ───────────────────────────────────
-- Regression: Mixin() return type T & ...M must resolve to concrete intersection.
local _baseObj = {}
local _mixed = Mixin(_baseObj, {})
--    ^ hover: (local) _mixed: table

-- ── Mixin(x, M) keeps x's original type in an intersection ────────────────────
-- Regression: a bare `Mixin(x, M)` narrows x to `typeof(x) & M`, not `M` alone.
-- When x's original type is statically unknown (an unannotated param), the
-- result must stay a top-level intersection (`any & M`) so x's pre-existing
-- runtime members (e.g. base Frame methods) are not wrongly flagged
-- `undefined-field`. (Mirrors Baganator's StyleButton/BackdropTemplateMixin.)
---@class StyleHookMixin
---@field ApplyStyle fun(self: StyleHookMixin)
local StyleHookMixin = {}

local function styleUnknownWidget(widget)
  Mixin(widget, StyleHookMixin)
  widget:ApplyStyle()      -- mixin method resolves
  widget:GetParent()       -- pre-existing (Frame) method: not undefined-field
  widget:IsEnabled()       -- another base method (Baganator's HookScript/IsEnabled case)
  return widget
  --     ^ hover: (param) widget: any & StyleHookMixin
end

-- Typed receiver: the original class type is preserved alongside the mixin.
---@param f Frame
local function styleFrameWidget(f)
  Mixin(f, StyleHookMixin)
  f:ApplyStyle()           -- mixin method
  f:GetParent()            -- Frame method, still resolves
  return f
  --     ^ hover: (param) f: Frame & StyleHookMixin
end

-- `@narrows-arg` is general: when the narrowed param's generic is *constrained*
-- (`@generic T: Frame`) and the original arg type is unknown, the constraint —
-- not bare `any` — is the precise stand-in, so the intersection keeps the
-- constrained class's methods. (Mixin's own `T` is unconstrained → `any`; this
-- exercises the constrained branch via a custom narrows-arg helper.)
---@generic T: Frame, M
---@narrows-arg 1
---@param object T
---@param mixin M
---@return T & M
local function MixConstrained(object, mixin)
  return Mixin(object, mixin)
end

local function useConstrainedMixin(widget)
  local cr = MixConstrained(widget, StyleHookMixin)
  return cr
  --     ^ hover: (local) cr: Frame & StyleHookMixin
end

-- ── Deep chained overlay on deferred runtime field ──────────────────────────
-- Regression: self.sub.field = CreateFrame(...) where `sub` is a runtime
-- field injected by a deferred field assignment. The deep field injection
-- must run after deferred field assignments so `sub` is visible.
---@class OverlayPanel
local OverlayPanel = {}

function OverlayPanel:Init()
    self.display = CreateFrame("Frame")
    self.display.wrapped = CreateFrame("Frame", nil, self.display)
    self.display.wrapped:SetSize(10, 10)
    --                       ^ hover: (method) function Frame:SetSize(x: uiUnit, y: uiUnit)
end

-- ── String method calls on variables ─────────────────────────────────────────
-- String variables have an implicit metatable with __index = string library,
-- so :method() calls should resolve to string library methods.

local strVar = "hello"
---@diagnostic disable-next-line: discard-returns
strVar:upper()
--     ^ hover: (method) function stringlib:upper(s: string | number)  def: external

local columnRange = "1-100"
local left, right = columnRange:match("^(%d+)%-(%d+)$")
--                              ^ hover: (method) function stringlib:match(  def: external

-- String methods on string|nil (union containing string)
---@return string?
local function maybeGetStr() return "hi" end
local optStr = maybeGetStr()
---@diagnostic disable-next-line: discard-returns
optStr:upper()
--     ^ hover: (method) function stringlib:upper(s: string | number)  def: external

-- ── String method calls on string literals ──────────────────────────────────
-- Parenthesized string literal: ("str"):method()
local fmtResult = ("|T%s:%d|t"):format("icon", 16)
--                              ^ hover: (method) function stringlib:format(  def: external
--                                      ^ sig: fun(s: string | number, ...: any): string

-- Bare string literal: "str":method()
local upperResult = "hello":upper()
--                          ^ hover: (method) function stringlib:upper(s: string | number)  def: external

-- ── String method COMPLETION on non-literal receivers ────────────────────────
-- Regression: string-typed variables/params/fields lost `:method` completion.
-- Only literal receivers resolved to the string library; identifier receivers
-- fell through extract_table_idx (a string is not a table) and returned nothing.
-- The completion path now mirrors hover (collect_library_table_indices).
local _cvar = strVar:upper()
--                    ^ comp: upper
---@param sparam string
local function _useStrParam(sparam)
    local _cparam = sparam:reverse()
    --                        ^ comp: reverse
    return _cparam
end
-- string|nil union receiver also resolves through the string library.
local _cunion = optStr:upper()
--                      ^ comp: upper
-- string-typed field receiver: exercises the dot-chain walk in
-- resolve_identifier_to_type (distinct from the simple-name cases above).
local _rec = { label = "hi" }
local _cfield = _rec.label:upper()
--                          ^ comp: upper

-- ── Enum value hover should show literal values ────────────────────────────
local _mana = Enum.PowerType.Mana
--                           ^ hover: (field) Mana: number = 0

-- ── AceAddon-3.0 type stubs ─────────────────────────────────────────────────

---@type AceAddon-3.0
local AceAddonLib

local myAddon = AceAddonLib:NewAddon("MyAddon", "AceEvent-3.0")
--    ^ hover: (local) myAddon: MyAddon {

local addonName = myAddon:GetName()
--    ^ hover: (local) addonName: string

local myMod = myAddon:NewModule("MyModule")
--    ^ hover: (local) myMod: MyModule {

local gotMod = myAddon:GetModule("MyModule")
--    ^ hover: (local) gotMod: MyModule {

-- IterateModules returns typed iterator for for-in loops
for modName, mod in myAddon:IterateModules() do
    local _mn = modName
    --    ^ hover: (local) _mn: string
    local _m = mod
    --    ^ hover: (local) _m: AceModule {
end

-- IterateAddons on the library object
for addonN, addon in AceAddonLib:IterateAddons() do
    local _an = addonN
    --    ^ hover: (local) _an: string
    local _a = addon
    --    ^ hover: (local) _a: AceAddon {
end

local isOn = myAddon:IsEnabled()
--    ^ hover: (local) isOn: boolean

-- ── Pool types (generic FramePool/ObjectPool stubs) ──────────────────────────

-- FramePool and ObjectPool are defined types — no undefined-doc-name
---@type FramePool<Frame>
local _testFramePool = nil

---@type ObjectPool<Button>
local _testObjPool = nil

-- ObjectPoolBaseMixin methods are accessible on pool objects (regression:
-- semicolon inline @class pattern `local Foo = {};---@class Foo` was
-- silently dropping methods — ensure the mixin class still has its methods)
---@type ObjectPoolBaseMixin
local _mixin = nil

-- CreateObjectPool returns a typed ObjectPool<T> where T comes from the creator
---@return Button
local function makeBtn()
    return CreateFrame("Button")
end
local btnPool = CreateObjectPool(makeBtn)
local acquiredBtn, acquiredBtnIsNew = btnPool:Acquire()
--    ^ hover: (local) acquiredBtn: Button
--                     ^ hover: (local) acquiredBtnIsNew: boolean

-- FramePool:Acquire() also returns (T & Tp, boolean)
---@type FramePool<Button>
local framePool
local fpFrame, fpIsNew = framePool:Acquire()
--             ^ hover: (local) fpIsNew: boolean

-- CreateFramePoolCollection returns a FramePoolCollection
local poolColl = CreateFramePoolCollection()
--    ^ hover: (local) poolColl: FramePoolCollection

-- Enum.ItemQuality includes both retail (Common/Uncommon) and classic (Standard/Good)
-- members so multi-flavor addons do not get undefined-field diagnostics.
local _qual = Enum.ItemQuality.Standard
--                              ^ hover: (field) Standard: number = 1
local _qual2 = Enum.ItemQuality.Good
--                               ^ hover: (field) Good: number = 2
local _qual3 = Enum.ItemQuality.Common
--                               ^ hover: (field) Common: number = 1
local _qual4 = Enum.ItemQuality.Uncommon
--                               ^ hover: (field) Uncommon: number = 2

-- ── Regression: @return Frame?, string? comma-form annotation ────────────────
-- When @return uses LuaLS comma-separated multi-return on a single line
-- (e.g. `@return Frame?, string?`), the first return type annotation must be
-- respected. Previously a parse bug caused `Frame?,` (with comma) to be
-- treated as an invalid type, leaving return_annotations empty and falling
-- back to the body's inferred type (ScriptRegion from GetMouseFoci()).

---@return Frame?
---@return string?
local function getFrameAndName()
    for _, frame in ipairs(GetMouseFoci()) do
        local f = frame --[[@as Frame]]
        return f, f:GetName()
    end
end

local commaRetFrame = getFrameAndName()
--    ^ hover: (local) commaRetFrame: Frame?

-- A nil-guarded call should give a Frame (not ScriptRegion | Frame).
--- @param f Frame
local function useFrame(f) end

local commaRetFrame2 = getFrameAndName()
if commaRetFrame2 then
    useFrame(commaRetFrame2)
end

-- ── loadstring tuple-union return ───────────────────────────────────────────

local lsFn, lsErr = loadstring("return 1")
--    ^ hover: (local) lsFn: function?
--            ^ hover: (local) lsErr: string?

-- narrowing: success case (early-exit on nil)
local lsFn2, lsErr2 = loadstring("return 1")
if not lsFn2 then return end
local _ = lsFn2
--        ^ hover: (local) lsFn2: function
local _ = lsErr2
--        ^ hover: (local) lsErr2: nil

-- narrowing: error case (early-exit on success)
local lsFn3, lsErr3 = loadstring("bad code")
if lsFn3 then return end
local _ = lsErr3
--        ^ hover: (local) lsErr3: string

-- Regression: a stub declaration with no @return (e.g. RunScript) has an
-- UNKNOWN return, not a confident nil. The empty placeholder body of a
-- generated stub must not be read as a nil-returning function. Spreading
-- such a call into a typed vararg (`strjoin(sep, ...: string | number)`)
-- must NOT flag type-mismatch.
local _joinedVoid = strjoin(", ", RunScript("test"))
--    ^ hover: (local) _joinedVoid: string

-- ── Factory function return types (CreateFromMixins inference) ─────────────

-- CreateTreeDataProvider returns LinearizedTreeDataProviderMixin
local _tdp = CreateTreeDataProvider()
--    ^ hover: (local) _tdp: LinearizedTreeDataProviderMixin

-- CreateDataProvider returns DataProviderMixin
local _dp = CreateDataProvider()
--    ^ hover: (local) _dp: DataProviderMixin

-- Methods on factory-created objects resolve correctly
_tdp:Insert("hello")
--   ^ hover: (method) function LinearizedTreeDataProviderMixin:Insert(data)

_dp:GetSize()
--  ^ hover: (method) function DataProviderMixin:GetSize()

-- Return type with unresolved tremove result should not collapse to nil.
-- When a function has `return nil` in one branch and `return result` where
-- result's type is unresolved in another, the return type should include
-- `any` (not just `nil`) to represent the unknown contribution.
local _retPriv = {}
_retPriv.chunks = {}
function _retPriv.getChunk(key)
--                ^ hover: (field) function getChunk(key)\n-> any?
    if not _retPriv.chunks[key] then
        return nil
    end
    local result = tremove(_retPriv.chunks[key])
    return result
end

-- When all return branches resolve, the union should converge to the
-- concrete types (not stay as `any`).
---@type string[]
local _retItems = {}
function _retPriv.getItem(key)
--                ^ hover: (field) function getItem(key: number)\n-> string?
    if not _retItems[key] then
        return nil
    end
    return tremove(_retItems)
end

-- ── TexturePool / FontStringPool subLayer param type ──────────────────────────
-- Numeric subLayer must not produce type-mismatch (exhaustive harness verifies).

---@type Frame
local _poolParent

-- CreateTexturePool: subLayer is number, not string
local _texPool = CreateTexturePool(_poolParent, "ARTWORK", 1)
--               ^ hover: (global) function CreateTexturePool(\nparent: Frame?,\nlayer?: string,\nsubLayer?: number,\ntemplate?: string,\nresetFunc?: function\n)\n-> ObjectPool<Texture>

-- CreateMaskTexturePool: subLayer is number, not string
local _maskPool = CreateMaskTexturePool(_poolParent, "ARTWORK", 2)
--                ^ hover: (global) function CreateMaskTexturePool(\nparent: Frame?,\nlayer?: string,\nsubLayer?: number,\ntemplate?: string,\nresetFunc?: function\n)\n-> ObjectPool<MaskTexture>

-- CreateFontStringPool: subLayer param exists and is number
local _fsPool = CreateFontStringPool(_poolParent, "OVERLAY", 0, "MyTemplate")
--              ^ hover: (global) function CreateFontStringPool(\nparent: Frame?,\nlayer?: string,\nsubLayer?: number,\ntemplate?: string,\nresetFunc?: function\n)\n-> ObjectPool<FontString>

-- ── Missing WoW API method stubs ──────────────────────────────────────────────

-- GameTooltip:SetItemByGUID (not in Blizzard APIDocumentation; hand-written override)
---@type GameTooltip
local _gtip = nil
_gtip:SetItemByGUID("item-guid-123")
--    ^ hover: (method) function GameTooltip:SetItemByGUID(itemGUID: string)

-- GameTooltip:SetUnitAuraByAuraInstanceID (not in Blizzard APIDocumentation; hand-written override)
_gtip:SetUnitAuraByAuraInstanceID("player", 42, "HELPFUL")
--    ^ hover: (method) function GameTooltip:SetUnitAuraByAuraInstanceID(

-- BattlePetTooltip:AddLine (Lua-injected method from BattlePetTooltipTemplate OnLoad)
BattlePetTooltip:AddLine("hello")
--               ^ hover: (method) function BattlePetTooltip:AddLine(

-- ColorPickerFrame:SetColorRGB (ColorSelect widget method via parent-class correction)
ColorPickerFrame:SetColorRGB(1, 0, 0)
--               ^ hover: (method) function ColorPickerFrame:SetColorRGB(rgbR: number, rgbG: number, rgbB: number)

-- ColorPickerFrame:GetColorRGB (ColorSelect widget method via parent-class correction)
local _cpR, _cpG, _cpB = ColorPickerFrame:GetColorRGB()
--                                        ^ hover: (method) function ColorPickerFrame:GetColorRGB()

-- ── AnchorUtil.CreateAnchor ────────────────────────────────────────────────
-- Regression: AnchorUtil.CreateAnchor must be callable and return AnchorMixin.

local anchor = AnchorUtil.CreateAnchor("TOPLEFT", nil, "TOPLEFT", 0, 0)
--    ^ hover: (local) anchor: AnchorMixin  def: local

local anchorFromPt = AnchorUtil.CreateAnchorFromPoint(anchor, 1)
--    ^ hover: (local) anchorFromPt: AnchorMixin  def: local

local globalAnchor = CreateAnchor("CENTER", nil, "CENTER")
--    ^ hover: (local) globalAnchor: AnchorMixin  def: local

-- ── TooltipDataLine runtime fields ──────────────────────────────────────────
-- Runtime fields discovered from wow-ui-source via structural matching:
-- Blizzard's code reads these fields on untyped `lineData` parameters in
-- TooltipDataRules.lua.  They are absent from APIDocumentationGenerated
-- (populated by TooltipUtil.SurfaceArgs from the C++ args array).

---@type TooltipDataLine
local _tdl = {}
local _tdlGemIcon = _tdl.gemIcon
--    ^ hover: (local) _tdlGemIcon: any?
local _tdlSocketType = _tdl.socketType
--    ^ hover: (local) _tdlSocketType: any?

-- ── EquipmentManager_UnpackLocation returns (deprecated; classic-only body) ──
-- Regression: the function is deprecated on retail (11.2.0) and Ketho stubs it
-- with NO @return, so destructuring its result false-positived as
-- `unbalanced-assignments`. stubs/overrides/EquipmentManager.lua declares the
-- six-value signature, so BOTH the 5-variable (Classic) and 6-variable (retail,
-- with the discarded isInVoidStorage at position 4) destructure idioms are
-- clean. The `deprecated` warning is suppressed per-line here only because this
-- file has no flavor config to suppress it; absence of `unbalanced-assignments`
-- is verified exhaustively by the harness.

-- 5-variable Classic idiom — slot/bag land at positions 4/5 (both number).
---@diagnostic disable-next-line: deprecated
local p1, b1, g1, s1, bag1 = EquipmentManager_UnpackLocation(5)
--    ^ hover: (local) p1: boolean  def: local
--                    ^ hover: (local) bag1: number

-- 6-variable retail idiom — slot/bag land at positions 5/6, position 4 discarded.
---@diagnostic disable-next-line: deprecated
local p2, b2, g2, _, s2, bag2 = EquipmentManager_UnpackLocation(5)
--                   ^ hover: (local) s2: number
--                         ^ hover: (local) bag2: number

-- ── C_EncounterJournal.GetDungeonEntrancesForMap: vector2 field is a Vector2DMixin ──
-- Regression: Blizzard tags struct fields such as DungeonEntranceMapInfo.position as
-- `Type = "vector2", Mixin = "Vector2DMixin"`. The vendor `vector2` alias lives in
-- Core/Type/Mixin.lua (`vector2 = Vector2DMixin`), whose file stem collided with the
-- Mixin() function override (formerly stubs/overrides/Mixin.lua) and was skipped
-- wholesale — dropping the alias and with it the whole field. Fixed by renaming the
-- override to MixinFunctions.lua so the vendor alias file is no longer shadowed.
--
-- The value IS a real mixin, not plain {x, y} data: WoW's C marshaller applies the
-- documented `Mixin`, so the field carries the Vector2DMixin methods (Blizzard's own
-- code calls e.g. `actorInfo.position:GetXYZ()` directly on such C returns). Mapping
-- it to the method-less Vector2DType instead — as a prior revision did — false-
-- positived every such method call. So it must resolve to Vector2DMixin.
--
-- Both the data fields (x/y, inherited from Vector2DType) and the methods resolve.
-- The x/y stayed `number` only after stripping the untyped `x: any` that the scan of
-- Vector2DMixin:SetXY's `self.x = x` body registered directly on the mixin, shadowing
-- the inherited Vector2DType.x — see strip_untyped_fields_shadowing_typed_ancestors.
local _dungeonEntrances = C_EncounterJournal.GetDungeonEntrancesForMap(123)
local _dungeonEntrance = _dungeonEntrances[1]
local _entrancePos = _dungeonEntrance.position
--    ^ hover: (local) _entrancePos: Vector2DMixin
local _entranceX = _entrancePos.x
--    ^ hover: (local) _entranceX: number
local _entranceLen = _entrancePos:GetLength()
--    ^ hover: (local) _entranceLen: number

-- ── C_Map.GetWorldPosFromMapPos: a vector2 return must not drop the return count ──
-- Regression (same root cause as above): the 2nd @return is `vector2 worldPosition`.
-- When `vector2` was unresolvable, that return was dropped and the function looked
-- like it returned a single value, so `local a, b = C_Map.GetWorldPosFromMapPos(...)`
-- false-positived as unbalanced-assignments. Exhaustive diag checking asserts that
-- diagnostic's absence; the hovers pin the recovered second return type.
local _continentID, _worldPos = C_Map.GetWorldPosFromMapPos(84, { x = 0.5, y = 0.5 })
--    ^ hover: (local) _continentID: number
--                  ^ hover: (local) _worldPos: Vector2DMixin

-- ── UiMapPoint static factories return the nominal UiMapPoint (not an anon shape) ──
-- Regression: UiMapPoint.CreateFrom* are un-annotated FrameXML factories, so return
-- inference builds their `{ uiMapID = mapID, position = CreateVector2D(x, y), z = z }`
-- table as an anonymous shape. It now collapses that shape to the `@class UiMapPoint`
-- sharing the namespace (every key is a declared field), so a `UiMapPoint` parameter
-- accepts the result. The hover pins the nominal return; exhaustive diag checking
-- asserts the absence of the former structural type-mismatch on the SetUserWaypoint
-- calls (including the 2-arg CreateFromVector2D form with `z` omitted).
local _waypoint = UiMapPoint.CreateFromCoordinates(84, 0.5, 0.5)
--    ^ hover: (local) _waypoint: UiMapPoint {
C_Map.SetUserWaypoint(UiMapPoint.CreateFromCoordinates(84, 0.5, 0.5))
C_Map.SetUserWaypoint(UiMapPoint.CreateFromVector2D(84, CreateVector2D(0.5, 0.5)))
C_Map.SetUserWaypoint(UiMapPoint.CreateFromVector3D(84, CreateVector3D(0.5, 0.5, 0)))

-- ── String receiver field access (undefined-field) ──────────────────────────
-- A `string`-typed value indexes into the `string` library through its
-- metatable, so its only valid fields are the string-library members. Reading a
-- field that isn't one yields nil — a silent typo, flagged. Requires the real
-- `string` stub (this maps `string` → the `stringlib` library table at scope 0).
-- Only a directly-string RECEIVER and only a field READ are flagged (see the two
-- gates in undefined_field.rs); calls on strings are deliberately left alone.
local strVal = "foo"

-- Known library field / method / literal receiver: no diagnostic.
local _su = strVal.upper
local _sm = strVal:upper()
local _sl = ("literal"):byte()

-- Unknown field READ on a string: flagged (dot access, various string sources).
local _sb1 = strVal.bogus
--                  ^ diag: undefined-field
local _sb2 = ("literal").missing
--                       ^ diag: undefined-field

-- Explicitly-typed string local is flagged the same way.
---@type string
local typedStr = "bar"
local _sb3 = typedStr.notAField
--                    ^ diag: undefined-field

-- Concatenation yields a string, so an unknown field READ on the result is flagged.
local concatStr = "a" .. "b"
local _sb4 = concatStr.gone
--                     ^ diag: undefined-field

-- NEGATIVE: a *call* on a string is NOT flagged. Addons routinely extend the
-- string metatable with custom methods we can't see (`("x"):Colorize()`), and a
-- genuinely missing method errors loudly at runtime — so unknown method/function
-- calls on strings are left alone (unlike the silent field reads above).
local _nc1 = strVal:nope()
local _nc2 = ("literal"):missing()

-- Membership probe on a string field is a defensive existence check: suppressed.
if strVal.maybe then
    local _guarded = 1
end

-- NEGATIVE: a table field can be mis-collapsed to `string` by lossy cross-file /
-- addon-namespace field inference (a module field colliding with a same-named
-- localized-string constant) or an unresolved-call heuristic. Field access on a
-- *table-member* string receiver is therefore NOT flagged, to avoid false
-- positives on real objects. (Exhaustive diag checking asserts the absence of
-- undefined-field on each line below.)
---@class StrFieldHost
---@field label string
local strHost = {} ---@type StrFieldHost
local _sh1 = strHost.label.someMethodThatWouldBeStringlib
-- The same via a method call, through post-assignment (AssignNarrow) refinement
-- — the exact shape of the reported false positive (`E.Minimap:Method()`).
strHost.label = "text"
local _sh2 = strHost.label:someMethodThatWouldBeStringlib()

-- NEGATIVE: a *classless* string-union receiver (`string | nil`, the type of any
-- optional `@field x string?` or an unnarrowed value) must NOT independently drive
-- the check — the string-library table is added to a union only additively (when a
-- class member is already present). Neither the field read nor the method call below
-- is flagged (both would false-positive otherwise: the union bypasses the pure-string
-- gates).
---@class OptStrHost
---@field opt string?
local optHost = {} ---@type OptStrHost
local optLocal = optHost.opt
local _ou1 = optLocal.bogusField
local _ou2 = optLocal:someCustomMethod()

-- POSITIVE (additive union): a `string | <class>` receiver suppresses valid members
-- of EITHER side (a string-library field and a class field), but a field on neither
-- is still flagged — named after the class, never the internal `stringlib`.
---@class StrUnionMember
---@field member number
---@type string | StrUnionMember
local strUnion = nil
local _uu1 = strUnion.upper   -- string-library field: suppressed
local _uu2 = strUnion.member  -- class field: suppressed
local _uu3 = strUnion.nowhere
--                    ^ diag: undefined-field
