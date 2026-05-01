-- wowlua_ls integration test (with stubs)
-- Requires: --with-stubs

local t = setmetatable({}, {})
--        ^ hover: (global) function setmetatable(tbl: table, metatable?: metatable | table)  def: external

local s = type("hello")
--        ^ hover: (global) function type(v: any)  def: external

local ok = pcall(print, "hi")
--         ^ hover: (global) function pcall(f: function, arg1?: any, ...: any)  def: external

-- pcall multi-return unpacking
local pcallOk, pcallErr = pcall(error, "boom")
--    ^ hover: (local) pcallOk: boolean  def: local

-- xpcall multi-return unpacking
local xpOk, xpErr = xpcall(error, print, "boom")
--    ^ hover: (local) xpOk: boolean  def: local

---@type Frame
local f = nil
--    ^ hover: (local) f: Frame {  def: local

-- Go-to-definition on external @class @field annotations
---@type CurrencyInfo
local ci = nil
local _ciName = ci.name
--                 ^ hover: (field) name: string  def: external

-- Compat globals (local alias → field ref, e.g. `local str = string; strmatch = str.match`)
local a = strmatch("hello", "(%w+)")
--        ^ hover: (global) function strmatch(s: string | number, pattern: string | number, init?: integer)  def: external

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

-- Ternary pattern with @return any function (strmatch returns any|nil)
local isMatch = strmatch("hello", "(%w+)") and true or false
--    ^ hover: (local) isMatch: boolean

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
--                 ^ hover: (global) LE_EXPANSION_CLASSIC: number  def: external

-- ── tinsert type checking against typed arrays ──────────────────────────────

---@type string[]
local names = {}
tinsert(names, "hello")
-- ^ diag: none
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

-- ── Dot-calling colon-defined stub methods (explicit self) ──────────────────

---@type Frame
local myFrame2 = nil
GameTooltip.Show(myFrame2)
-- ^ diag: none

-- ── Keyword-named parameters (e.g. `repeat`) should still be parsed ─────────

DoTradeSkill(1, 5)
-- ^ hover: (global) function DoTradeSkill(index: number, repeat: number)  def: external
-- ^ diag: none

-- ── setfenv: `async fun(...)` in union should parse as function ─────────

local function myFunc() end
setfenv(myFunc, {})
-- ^ diag: none

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
-- ^ diag: none

-- _G dot access reads resolve to globals
local _g_c = _G.print
--    ^ hover: (local) function _g_c(...: any)

-- _G dot access on table globals (no undefined-field)
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

-- Definition on indirect _G field resolves to the global
local _g_e = _g_indirect.type
--                        ^ def: external

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
--    ^ hover: (local) _bdFrame: Frame & BackdropTemplate
--    ^ diag: none

-- ── Classic XML frame globals get their @type annotation (not nil) ───────────

-- Frame globals extracted from XML (e.g. `---@type Button\nCraftCreateButton = nil`)
-- should resolve to the annotated type, not nil.
local _craftBtn = CraftCreateButton
--    ^ hover: (local) _craftBtn: CraftCreateButtonType {
--                ^ hover: (global) CraftCreateButton: CraftCreateButtonType {  def: external

-- ── WoW Enum types (Enum.X) accept plain number ───────────────────────────
local _power = UnitPower("player", 0)
--                                 ^ diag: none
local _power2 = UnitPower("player", Enum.PowerType.Mana)
--                                  ^ diag: none

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
--     ^ diag: none

local aceDrop = AceGUI:Create("Dropdown")
--    ^ hover: (local) aceDrop: AceGUIDropdown {
aceDrop:SetLabel("Pick one")
--      ^ hover: (method) function AceGUIDropdown:SetLabel(text: string)
aceDrop:SetList({})
--      ^ hover: (method) function AceGUIDropdown:SetList(list: table, order?: any[])
aceDrop:SetValue("foo")
--      ^ diag: none
aceDrop:SetMultiselect(false)
--      ^ diag: none

local aceFrame = AceGUI:Create("Frame")
--    ^ hover: (local) aceFrame: AceGUIFrame {
aceFrame:SetTitle("My Window")
--       ^ hover: (method) function AceGUIFrame:SetTitle(text: string)
aceFrame:SetStatusText("Ready")
--       ^ hover: (method) function AceGUIFrame:SetStatusText(text: string)
aceFrame:AddChild(aceBtn)
--       ^ hover: (method) function AceGUIFrame:AddChild(widget: AceGUIWidget, beforeWidget?: AceGUIWidget)
aceFrame:SetLayout("Flow")
--       ^ diag: none

local aceSlider = AceGUI:Create("Slider")
--    ^ hover: (local) aceSlider: AceGUISlider {
aceSlider:SetSliderValues(0, 100, 1)
--        ^ hover: (method) function AceGUISlider:SetSliderValues(min?: number, max?: number, step?: number)

local aceTree = AceGUI:Create("TreeGroup")
--    ^ hover: (local) aceTree: AceGUITreeGroup {
aceTree:SetTree({})
--      ^ hover: (method) function AceGUITreeGroup:SetTree(tree: table, filter?: boolean)
aceTree:SetStatusTable({})
--      ^ diag: none

-- debugstack: all params optional
local _ds1 = debugstack()
--            ^ hover: (global) function debugstack(\ncoroutine: thread,\nstart?: number,\ncount1?: number,\ncount2?: number\n)\n-> string\nfunction debugstack(start?: number, count1?: number, count2?: number)\n-> string  def: external
--            ^ diag: none
local _ds2 = debugstack(2)
--            ^ diag: none
local _ds3 = debugstack(2, 10)
--            ^ diag: none
local _ds4 = debugstack(2, 10, 5)
--            ^ diag: none

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

-- Colon-method definition on CreateFrame result (was false positive: undefined-field)
do
    local evtFrame = CreateFrame('Frame')
    function evtFrame:OnEvent(e, ...)
    end
    evtFrame:SetScript('OnEvent', evtFrame.OnEvent)
    --                                     ^ hover: (field) function Frame.OnEvent(self: Frame, e, ...)
    --                                     ^ diag: none

    -- Dot-method assignment on CreateFrame result
    evtFrame.OnClick = function(self) end
    --       ^ diag: none
    local _ref = evtFrame.OnClick
    --                    ^ hover: (field) function Frame.OnClick(self)
end

-- ── and-guarded field assignments resolve through StripNil/StripFalsy ──

---@class StripTestAPI
---@field DoThing fun(x: number): string

---@type StripTestAPI?
local MaybeAPI

local directLocal = MaybeAPI and MaybeAPI.DoThing
--    ^ hover: (local) directLocal: nil | fun(x: number): string

local tbl = {}
tbl.guardedField = MaybeAPI and MaybeAPI.DoThing
--  ^ hover: (field) guardedField: nil | fun(x: number): string

tbl.directField = MaybeAPI and MaybeAPI.DoThing
--  ^ hover: (field) directField: nil | fun(x: number): string

local tbl2 = {}
tbl2.orField = MaybeAPI or "fallback"
--   ^ hover: (field) orField: StripTestAPI | string

-- Chained and-guard: both sides contribute to narrowing
local tbl3 = {}
tbl3.chained = MaybeAPI and MaybeAPI.DoThing and MaybeAPI.DoThing
--   ^ hover: (field) chained: nil | fun(x: number): string

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
--                       ^ hover: (event) PLAYER_LOGIN

_evFrame:RegisterEvent("PLAYER_LOGOUT")
--                       ^ hover: (event) PLAYER_LOGOUT

_evFrame:RegisterEvent("ENCOUNTER_END")
--                       ^ hover: (event) ENCOUNTER_END →

_evFrame:RegisterEvent("PLAYER_ENTERING_WORLD")
--                       ^ hover: (event) PLAYER_ENTERING_WORLD →

_evFrame:RegisterEvent("NONEXISTENT_EVENT_XYZ")
--                       ^ hover: <missing>
