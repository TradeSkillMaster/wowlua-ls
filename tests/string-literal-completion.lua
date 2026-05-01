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
--                  ^ comp: SLCObj, _G, count, mode, nested, obj, reward, toggle
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
--           ^ comp: SLCObj, _G, count, mode, nested, obj, reward, toggle
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
