-- Test: redundant-or and redundant-and diagnostics
---@diagnostic disable: unused-local, unused-function

local function _use(...) end

-- ── redundant-or: LHS always truthy ───────────────────────────────────────────

-- Number is always truthy
local a = 2 or 0
--        ^ diag: redundant-or

-- String is always truthy
local b = "hello" or "default"
--        ^ diag: redundant-or

-- Table is always truthy
local c = {} or {}
--        ^ diag: redundant-or

-- true is always truthy
local d = true or false
--        ^ diag: redundant-or

-- Function is always truthy
local e = _use or print
--        ^ diag: redundant-or

-- Variable with known truthy type
---@type number
local num
local f = num or 0
--        ^ diag: redundant-or

-- String variable
---@type string
local str
local g = str or ""
--        ^ diag: redundant-or

-- ── redundant-and: LHS always falsy ──────────────────────────────────────────

-- nil is always falsy
local h = nil and 1
--        ^ diag: redundant-and

-- false is always falsy
local i = false and "hello"
--        ^ diag: redundant-and

-- ── No diagnostic: LHS can be falsy (or) ────────────────────────────────────

-- nil|number — not guaranteed truthy
---@type number?
local maybeNum
_use(maybeNum or 0)

-- boolean — could be false
---@type boolean
local maybeBool
_use(maybeBool or "default")

-- Uninitialized local is nil
local uninit
_use(uninit or "fallback")

-- ── No diagnostic: LHS can be truthy (and) ──────────────────────────────────

-- Truthy LHS with and — common idiom, not flagged
_use(2 and "yes")

-- boolean LHS with and — could be true
_use(maybeBool and "yes")

-- ── No diagnostic: permissive types ─────────────────────────────────────────

---@param x any
local function withAny(x)
    _use(x or 0)
    _use(x and "yes")
end
_use(withAny)

---@generic T
---@param x T
---@return T
local function withGeneric(x)
    _use(x or 0)
    return x
end
_use(withGeneric)

-- ── Suppression ─────────────────────────────────────────────────────────────

---@diagnostic disable-next-line: redundant-or
local s1 = 2 or 0

---@diagnostic disable-next-line: redundant-and
local s2 = nil and 1
