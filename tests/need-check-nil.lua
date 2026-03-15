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
--         ^ hover: (global) f1: NilCheckFrame | nil

-- ── Nil guard with bare name ─────────────────────────────────────────────

if f1 then
    f1.name = "hello"
    -- ^ diag: none
    local _ = f1
    --        ^ hover: (global) f1: NilCheckFrame {
end

-- ── Comparison guard (~= nil) ────────────────────────────────────────────

---@type NilCheckFrame|nil
local f2 = nil
if f2 ~= nil then
    f2.name = "hello"
    -- ^ diag: none
    --    ^ hover: (field) name: string
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
--        ^ hover: (global) f8: NilCheckFrame {

-- ── Early-exit with `not x` ─────────────────────────────────────────────

---@type NilCheckFrame|nil
local f9 = nil
if not f9 then
    error("expected f9")
end
f9.name = "hello"
-- ^ diag: none
local _ = f9
--        ^ hover: (global) f9: NilCheckFrame {

-- ── Early-exit with `x == nil` ──────────────────────────────────────────

---@type NilCheckFrame|nil
local f10 = nil
if f10 == nil then
    return
end
f10.name = "hello"
-- ^ diag: none

-- ── While loop condition narrows ────────────────────────────────────────

---@type NilCheckFrame|nil
local f11 = nil
while f11 do
    local _ = f11
    --        ^ hover: (global) f11: NilCheckFrame {
    f11.name = "ok"
    -- ^ diag: none
end

-- ── While loop condition narrows (with reassignment) ───────────────────

---@class NilCheckLinked
---@field next NilCheckLinked|nil
---@field name string

---@type NilCheckLinked|nil
local node = nil
while node do
    node.name = "ok"
    -- ^ diag: none
    node = node.next
    -- ^ diag: none
end

-- ── Suppression ──────────────────────────────────────────────────────────

---@type NilCheckFrame|nil
local f7 = nil
---@diagnostic disable-next-line: need-check-nil
f7.name = "suppressed"
-- ^ diag: none

-- ── and-condition propagates nil guard ─────────────────────────────────

---@type NilCheckFrame|nil
local f12 = nil
if f12 ~= nil and f12.name then
    f12.name = "ok"
    -- ^ diag: none
end

-- ── and-condition: hover shows narrowed type on RHS ───────────────────

---@type NilCheckFrame|nil
local f13 = nil
if f13 ~= nil and f13.name then
--                 ^ hover: (global) f13: NilCheckFrame {
    local _ = f13
    --        ^ hover: (global) f13: NilCheckFrame {
end
-- hover outside guard shows full union
local _ = f13
--        ^ hover: (global) f13: NilCheckFrame | nil

-- ── bare truthiness and ───────────────────────────────────────────────

---@type NilCheckFrame|nil
local f14 = nil
if f14 and f14.name then
    f14.name = "ok"
    -- ^ diag: none
end

-- ── chained and with two guards ───────────────────────────────────────

---@type NilCheckFrame|nil
local f15 = nil
---@type NilCheckFrame|nil
local f16 = nil
if f15 ~= nil and f16 ~= nil then
    f15.name = "ok"
    -- ^ diag: none
    f16.name = "ok"
    -- ^ diag: none
end

-- ── cached type() guard ─────────────────────────────────────────────

---@type NilCheckFrame|nil
local f17 = nil
local f17type = type(f17)
if f17type == "table" then
    f17.name = "ok"
    -- ^ diag: none
end

---@type NilCheckFrame|nil
local f18 = nil
local f18type = type(f18)
if f18type == "table" and f18.name then
--                            ^ diag: none
    f18.name = "ok"
    -- ^ diag: none
end

-- ── Assignment narrows field ────────────────────────────────────────────

---@class NilCheckState
---@field btn NilCheckFrame|nil

---@type fun(): NilCheckFrame
local createBtn

---@param state NilCheckState
local function testAssignNarrow(state)
    state.btn = state.btn or createBtn()
    state.btn:Show()
    -- ^ diag: none
    state.btn.name = "ok"
    -- ^ diag: none
end
_consume(testAssignNarrow)

-- ── Assert field narrowing applies to return type checks ────────────────

---@class NilCheckElement
---@field _parent NilCheckElement|nil

---@param self NilCheckElement
---@return NilCheckElement
local function getParent(self)
    assert(self._parent)
    return self._parent
    -- ^ diag: none
end
_consume(getParent)

-- ── Nil assignment does NOT narrow ──────────────────────────────────────

---@param state NilCheckState
local function testNilNoNarrow(state)
    state.btn = nil
    state.btn:Show()
    -- ^ diag: need-check-nil
end
_consume(testNilNoNarrow)

-- ── Ensure-initialized narrows field ──────────────────────────────────
-- `if not field then field = val end` guarantees non-nil after

---@param state NilCheckState
local function testEnsureInit(state)
    if not state.btn then
        state.btn = createBtn()
    end
    state.btn:Show()
    -- ^ diag: none
    state.btn.name = "ok"
    -- ^ diag: none
end
_consume(testEnsureInit)

-- Variant: `if field == nil then field = val end`
---@param state NilCheckState
local function testEnsureInitEq(state)
    if state.btn == nil then
        state.btn = createBtn()
    end
    state.btn:Show()
    -- ^ diag: none
end
_consume(testEnsureInitEq)

-- ── field access guard in `and` expression (not `if`) ───────────────

---@class NilCheckElement
---@field _parent NilCheckElement|nil
---@field _id string

---@param self NilCheckElement
local function testAndFieldGuard(self)
    local parentId = self._parent and self._parent._id
    --                                              ^ diag: none
    _consume(parentId)
end
_consume(testAndFieldGuard)

-- Variant: `self._parent ~= nil and self._parent._id`
---@param self NilCheckElement
local function testAndFieldGuardNeq(self)
    local parentId = self._parent ~= nil and self._parent._id
    --                                                    ^ diag: none
    _consume(parentId)
end
_consume(testAndFieldGuardNeq)
