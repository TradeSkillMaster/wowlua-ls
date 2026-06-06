-- Test: redundant-condition diagnostic (always-truthy/falsy if/while conditions)
---@diagnostic disable: unused-local, unused-function, undefined-global, empty-block, shadowed-local

local function _use(...) end

-- ── Always truthy: if ────────────────────────────────────────────────────────

-- Number literal
local x = 2
if x then end
-- ^ diag: redundant-condition

-- String literal
local s = "hello"
if s then end
-- ^ diag: redundant-condition

-- Table literal
local t = {}
if t then end
-- ^ diag: redundant-condition

-- true literal
if true then end
-- ^ diag: redundant-condition

-- Function value
local function fn() end
if fn then end
-- ^ diag: redundant-condition

-- Typed variable (number)
---@type number
local num
if num then end
-- ^ diag: redundant-condition

-- Typed variable (string)
---@type string
local str
if str then end
-- ^ diag: redundant-condition

-- ── Always falsy: if ─────────────────────────────────────────────────────────

-- nil
if nil then end
-- ^ diag: redundant-condition

-- false
if false then end
-- ^ diag: redundant-condition

-- ── Always truthy: while ─────────────────────────────────────────────────────

-- `while true` is a common idiom; still flagged for strict users
while true do break end
--    ^ diag: redundant-condition

-- Number condition
---@type number
local n
while n do break end
--    ^ diag: redundant-condition

-- ── Always truthy: elseif ────────────────────────────────────────────────────

---@type boolean
local flag
if flag then
    _use(1)
elseif true then
    --   ^ diag: redundant-condition
    _use(2)
end

-- ── Always truthy: repeat...until ─────────────────────────────────────────────

-- `repeat...until true` always exits after one iteration
repeat break until true
--                ^ diag: redundant-condition

-- ── Always falsy: repeat...until ─────────────────────────────────────────────

-- `repeat...until false` is an infinite loop
repeat break until false
--                 ^ diag: redundant-condition

-- ── No diagnostic: compound conditions ───────────────────────────────────────

-- `x and y` where x is always truthy — the `and` result type depends on y,
-- so the overall condition is not guaranteed truthy/falsy.
---@type number
local a
---@type boolean
local b
if a and b then end

-- ── No diagnostic: nilable types ─────────────────────────────────────────────

---@type number?
local maybeNum
if maybeNum then end

---@type string?
local maybeStr
if maybeStr then end

-- boolean can be false
---@type boolean
local maybeBool
if maybeBool then end

-- Uninitialized local resolves to `?` (unknown), not flagged
local uninit
if uninit then end

-- ── No diagnostic: permissive types ──────────────────────────────────────────

---@param x any
local function withAny(x)
    if x then end
end
_use(withAny)

---@generic T
---@param x T
---@return T
local function withGeneric(x)
    if x then end
    return x
end
_use(withGeneric)

-- ── No diagnostic: nil-initialized local with conditional assignment ────────
-- When a variable is initialized to nil and conditionally assigned from an
-- unresolved call, the branch merge should yield `any` (not `nil`), so
-- `if x then` is NOT flagged as redundant-condition.

local function conditionalAssign(cond)
    local price = nil
    if cond then
        price = unknownFunc() -- intentionally undefined
    end
    if price then
        return price
    end
end
_use(conditionalAssign)

-- ── Suppression ──────────────────────────────────────────────────────────────

---@diagnostic disable-next-line: redundant-condition
if true then end
