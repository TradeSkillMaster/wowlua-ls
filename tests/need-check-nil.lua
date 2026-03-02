local function _consume(...) end

---@class NilCheckFrame
---@field Show fun(self: NilCheckFrame)
---@field name string

-- ── Basic nullable field access ──────────────────────────────────────────

---@type NilCheckFrame|nil
local f1 = nil
f1.name = "hello"
-- ^ diag: need-check-nil

-- ── Hover shows full union outside guard ────────────────────────────────

local _ = f1
--         ^ hover: f1: NilCheckFrame | nil

-- ── Nil guard with bare name ─────────────────────────────────────────────

if f1 then
    f1.name = "hello"
    -- ^ diag: none
    local _ = f1
    --        ^ hover: f1: NilCheckFrame
end

-- ── Comparison guard (~= nil) ────────────────────────────────────────────

---@type NilCheckFrame|nil
local f2 = nil
if f2 ~= nil then
    f2.name = "hello"
    -- ^ diag: none
    --    ^ hover: name: string
end

-- ── Inverse guard (== nil else) ──────────────────────────────────────────

---@type NilCheckFrame|nil
local f3 = nil
if f3 == nil then
    _consume("nil")
else
    f3.name = "hello"
    -- ^ diag: none
end

-- ── Non-nullable: no warning ─────────────────────────────────────────────

---@type NilCheckFrame
local f4 = {}
f4.name = "hello"
-- ^ diag: none

-- ── Method call on nullable ──────────────────────────────────────────────

---@type NilCheckFrame|nil
local f5 = nil
f5:Show()
-- ^ diag: need-check-nil

-- ── Method call inside guard ─────────────────────────────────────────────

if f5 then
    f5:Show()
    -- ^ diag: none
end

-- ── Nested scope inherits narrowing ──────────────────────────────────────

---@type NilCheckFrame|nil
local f6 = nil
if f6 then
    if true then
        f6.name = "nested"
        -- ^ diag: none
    end
end

-- ── Optional parameter ───────────────────────────────────────────────────

---@param f? NilCheckFrame
local function useFrame(f)
    f.name = "hello"
    -- ^ diag: need-check-nil
    if f then
        f.name = "hello"
        -- ^ diag: none
    end
end
_consume(useFrame)

-- ── Assert narrowing ────────────────────────────────────────────────────

---@type NilCheckFrame|nil
local f8 = nil
assert(f8)
f8.name = "hello"
-- ^ diag: none
local _ = f8
--        ^ hover: f8: NilCheckFrame

-- ── Early-exit with `not x` ─────────────────────────────────────────────

---@type NilCheckFrame|nil
local f9 = nil
if not f9 then
    error("expected f9")
end
f9.name = "hello"
-- ^ diag: none
local _ = f9
--        ^ hover: f9: NilCheckFrame

-- ── Early-exit with `x == nil` ──────────────────────────────────────────

---@type NilCheckFrame|nil
local f10 = nil
if f10 == nil then
    return
end
f10.name = "hello"
-- ^ diag: none

-- ── Suppression ──────────────────────────────────────────────────────────

---@type NilCheckFrame|nil
local f7 = nil
---@diagnostic disable-next-line: need-check-nil
f7.name = "suppressed"
-- ^ diag: none
