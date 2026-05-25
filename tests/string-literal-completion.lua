-- String literal completion tests: suggesting values from string literal union types
-- in == and ~= comparisons.

-- ── Field access with string literal union ──────────────────────────────────

---@class SLCReward
---@field type "Recipe"|"Profession"|"Mount"|"Cosmetic"
---@field name string

---@type SLCReward
local reward

-- Completions inside "" on RHS of ==
if reward.type == "" then
--                  ^ comp: Recipe, Profession, Mount, Cosmetic
end

-- Completions inside "" on RHS of ~=
if reward.type ~= "" then
--                  ^ comp: Recipe, Profession, Mount, Cosmetic
end

-- No completions for plain string fields (falls through to scope completions)
if reward.name == "" then
--                  ^ comp: SLCCreateWidget, SLCInnerFn, SLCObj, SLCOuterFn, SLCRegistry, SLCSet, SLCUtils, _G, count, mode, nested, obj, reg, reward, toggle
end

-- ── Simple variable with string literal union ───────────────────────────────

---@type "alpha"|"beta"|"gamma"
local mode

if mode == "" then
--          ^ comp: alpha, beta, gamma
end

-- ── String on left side of == ───────────────────────────────────────────────

if "" == mode then
--  ^ comp: alpha, beta, gamma
end

-- ── Method call return type ─────────────────────────────────────────────────

---@class SLCObj
local SLCObj = {}

---@return "active"|"inactive"|"pending"
function SLCObj:GetStatus()
    return "active"
end

---@type SLCObj
local obj

if obj:GetStatus() == "" then
--                      ^ comp: active, inactive, pending
end

-- ── Non-string-literal type (no completions) ────────────────────────────────

---@type number
local count

if count == "" then
--           ^ comp: SLCCreateWidget, SLCInnerFn, SLCObj, SLCOuterFn, SLCRegistry, SLCSet, SLCUtils, _G, count, mode, nested, obj, reg, reward, toggle
end

-- ── Two-value string literal union ──────────────────────────────────────────

---@type "on"|"off"
local toggle

if toggle == "" then
--            ^ comp: on, off
end

-- ── Single-quote strings ────────────────────────────────────────────────────

if mode == '' then
--          ^ comp: alpha, beta, gamma
end

-- ── Partially typed string ──────────────────────────────────────────────────

if reward.type == "Re" then
--                   ^ comp: Recipe, Profession, Mount, Cosmetic
end

-- ── Nested field access ─────────────────────────────────────────────────────

---@class SLCInner
---@field kind "a"|"b"|"c"

---@class SLCOuter
---@field sub SLCInner

---@type SLCOuter
local nested

if nested.sub.kind == "" then
--                      ^ comp: a, b, c
end

-- ── Function parameter string literal completions ─────────────────────────

---@param frameType "Frame"|"Button"|"Slider"|"EditBox"
---@param name string
local function SLCCreateWidget(frameType, name)
end

---@diagnostic disable-next-line: missing-parameter
SLCCreateWidget("")
--               ^ comp: Frame, Button, Slider, EditBox

-- Second parameter (plain string, no completions → falls through to scope)
SLCCreateWidget("Frame", "")
--                         ^ comp: SLCCreateWidget, SLCInnerFn, SLCObj, SLCOuterFn, SLCRegistry, SLCSet, SLCUtils, _G, count, mode, nested, obj, reg, reward, toggle

-- ── Method call with string literal param ─────────────────────────────────

---@class SLCRegistry
local SLCRegistry = {}

---@param category "spell"|"item"|"quest"
---@param id number
function SLCRegistry:Register(category, id)
end

---@type SLCRegistry
local reg

---@diagnostic disable-next-line: missing-parameter
reg:Register("")
--            ^ comp: spell, item, quest

-- ── Dot-call with string literal param ────────────────────────────────────

---@class SLCUtils
local SLCUtils = {}

---@param level "low"|"medium"|"high"
function SLCUtils.SetLevel(level)
end

SLCUtils.SetLevel("")
--                 ^ comp: low, medium, high

-- ── Overloaded function with different string literal params ──────────────

---@overload fun(kind: "text", value: string): nil
---@overload fun(kind: "number", value: number): nil
local function SLCSet(kind, value)
end

SLCSet("")
--      ^ comp: text, number

-- ── Nested call: string is in the inner call ─────────────────────────────

---@param x "yes"|"no"
---@return string
local function SLCInnerFn(x)
    return x
end

---@param s string
local function SLCOuterFn(s)
end

SLCOuterFn(SLCInnerFn(""))
--                      ^ comp: yes, no
