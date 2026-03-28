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

-- ── Assert narrowing with and-chain ─────────────────────────────────────

---@type NilCheckFrame|nil
local f8a = nil
---@type NilCheckFrame|nil
local f8b = nil
---@type NilCheckFrame|nil
local f8c = nil
assert(f8a and f8b and f8c)
f8a.name = "hello"
-- ^ diag: none
f8b.name = "hello"
-- ^ diag: none
f8c.name = "hello"
-- ^ diag: none

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
---@field public _parent NilCheckElement|nil

---@param self NilCheckElement
---@return NilCheckElement
local function getParent(self)
    assert(self._parent)
    return self._parent
    -- ^ diag: none
end
_consume(getParent)

-- ── Assert field narrowing with bare nil field type ─────────────────────
-- When a field is typed as bare `nil` (not a union), strip_nil must still
-- produce a type that satisfies the @return annotation after assert().

---@class BareNilFieldObj
---@field public _data nil

---@param self BareNilFieldObj
---@return string
local function getBareNilField(self)
    assert(self._data)
    return self._data
    -- ^ diag: none
end
_consume(getBareNilField)

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
---@field public _parent NilCheckElement|nil
---@field public _id string

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

-- ── AND field guard with deep chain (3+ names) ───────────────────────

---@class NilCheckDeepState
---@field x NilCheckElement|nil

---@class NilCheckDeepObj
---@field public _state NilCheckDeepState

---@param self NilCheckDeepObj
local function testAndFieldChainGuard(self)
    local id = self._state.x and self._state.x._id
    --                                        ^ diag: none
    _consume(id)
end
_consume(testAndFieldChainGuard)

-- Variant: `self._state.x ~= nil and self._state.x._id`
---@param self NilCheckDeepObj
local function testAndFieldChainGuardNeq(self)
    local id = self._state.x ~= nil and self._state.x._id
    --                                                ^ diag: none
    _consume(id)
end
_consume(testAndFieldChainGuardNeq)

-- ── Truthiness guards strip false from unions ──────────────────────────

---@type string|false
local nilCheckFalsy1 = false
if not nilCheckFalsy1 then return end
local _ = nilCheckFalsy1
--        ^ hover: (global) nilCheckFalsy1: string

---@type string|false
local nilCheckFalsy2 = false
if nilCheckFalsy2 then
    local _ = nilCheckFalsy2
    --        ^ hover: (global) nilCheckFalsy2: string
end

-- `x ~= nil` should NOT strip false, only nil
---@type string|false|nil
local nilCheckFalsy3 = false
if nilCheckFalsy3 ~= nil then
    local _ = nilCheckFalsy3
    --        ^ hover: (global) nilCheckFalsy3: string | false
end

-- ── Elseif branch narrowing after early-exit guard ───────────────────────

---@param x string?
local function _elseifNarrow(x)
    if not x then
        return
    elseif x:len() > 5 then
        --  ^ diag: none
        return x:upper()
        --      ^ diag: none
    end
end

---@type NilCheckFrame|nil
local f_elseif = nil
if f_elseif == nil then
    error("missing")
elseif f_elseif.name == "test" then
    --            ^ diag: none
    f_elseif:Show()
    -- ^ diag: none
end

-- Early-exit narrowing with `or` and else branch present
---@param a? NilCheckFrame
---@param b? NilCheckFrame
local function _elseifOrNarrow(a, b)
    if not a or not b then
        return
    elseif a.name ~= b.name then
        --    ^ diag: none
        a:Show()
        -- ^ diag: none
    else
        b:Show()
        -- ^ diag: none
    end
end

-- ── Then-branch narrows assignment targets ──────────────────────────
-- `if value then local x = value end` should narrow x to non-nil

---@type string|nil
local thenNarrow1 = nil
if thenNarrow1 then
    local x = thenNarrow1
    --    ^ hover: (local) x: string
end

-- After the if-block, the type should be the original (un-narrowed)
local _ = thenNarrow1
--        ^ hover: (global) thenNarrow1: string | nil

-- ~= nil guard also narrows assignment
---@type number|nil
local thenNarrow2 = nil
if thenNarrow2 ~= nil then
    local y = thenNarrow2
    --    ^ hover: (local) y: number
end

-- ── `not x` unary in elseif inverse narrowing ──────────────────────────
-- Pattern: `if not x then ... elseif x.field` — the `not` must flip narrowing

---@type NilCheckFrame|nil
local notUnary1 = nil
if not notUnary1 then
    error("missing")
elseif notUnary1.name == "test" then
    --              ^ diag: none
    notUnary1:Show()
    -- ^ diag: none
end

-- Variant: `if not x then return end; x.field` (early-exit)
---@type NilCheckFrame|nil
local notUnary2 = nil
if not notUnary2 then
    return
end
notUnary2.name = "ok"

-- ── AND short-circuit suppresses nil-check on field chains ──────────────

---@type NilCheckFrame|nil
local f30 = nil
local _ = f30 and f30.name
--                ^ diag: none

---@type NilCheckFrame|nil
local f31 = nil
local _ = f31 and f31:Show()
--                ^ diag: none

-- Chained and: first guard suppresses nil-checks in entire RHS
---@type NilCheckFrame|nil
local f32 = nil
local _ = f32 and f32.name ~= "" and f32.name
--                ^ diag: none
-- ^ diag: none

-- Ternary idiom: `x and x.a or x.b` suppresses nil-checks on x in or-branch
---@type NilCheckFrame|nil
local f33 = nil
local _ = f33 and f33.name or f33.name
--                ^ diag: none
-- ^ diag: none

-- ── All-branch-assign narrows post-chain ──────────────────────────────
-- When every branch of if/elseif/else assigns to a variable, the type
-- after the chain should be the union of all branch types (no pre-chain nil).

---@class BranchQuery
---@field ResetOrderBy fun(self: BranchQuery)
---@field OrderBy fun(self: BranchQuery, field: string, ascending: boolean)

---@return BranchQuery
local function createQuery()
    ---@type BranchQuery
    local q = {}
    return q
end

-- Pattern 1: All branches assign (if/elseif/else)
local query1 = nil
if true then
    query1 = createQuery()
elseif true then
    query1 = createQuery()
else
    query1 = createQuery()
end
query1:ResetOrderBy()
-- ^ diag: none
local _ = query1
--        ^ hover: (global) query1: BranchQuery

-- Pattern 2: If-guard + else assigns (optional param pattern)
---@param q? BranchQuery
local function testGuardElseAssign(q)
    if q then
        q:ResetOrderBy()
    else
        q = createQuery()
    end
    q:OrderBy("name", true)
    -- ^ diag: none
end
_consume(testGuardElseAssign)

-- Pattern 3: All branches assign, usage in nested scope
local query3 = nil
if true then
    query3 = createQuery()
elseif true then
    query3 = createQuery()
else
    query3 = createQuery()
end
if true then
    query3:ResetOrderBy()
    -- ^ diag: none
end

-- Pattern 4: Without else, usage in nested scope — should still warn
local query4 = nil
if true then
    query4 = createQuery()
elseif true then
    query4 = createQuery()
end
if true then
    query4:ResetOrderBy()
    -- ^ diag: need-check-nil
end

-- ── `not x` elseif narrowing WITHOUT early exit ────────────────────────
-- When the then-branch does NOT return, elseif inverse narrowing must
-- still strip nil from x because reaching the elseif means `not x` was false.

---@class NilCheckAuction
---@field hasInvalidSeller boolean
---@field buyout number
---@field seller string

---@param lowestAuction NilCheckAuction?
local function _elseifNoReturn(lowestAuction)
    local reason = ""
    if not lowestAuction then
        reason = "none"
    elseif lowestAuction.hasInvalidSeller then
        --                ^ diag: none
        reason = "invalid"
    elseif lowestAuction.buyout > 100 then
        --               ^ diag: none
        reason = lowestAuction.seller
        --                     ^ diag: none
    end
    return reason
end
_consume(_elseifNoReturn)

-- Variant: multi-branch early exit before a second if-chain
-- `if cond1 then return elseif not x then return end; x.field` — all branches
-- return, so reaching past the chain means `not x` was false and x is non-nil.

---@param lowestAuction NilCheckAuction?
---@param cancelRepost boolean
local function _elseifMultiBranchExit(lowestAuction, cancelRepost)
    if not lowestAuction and cancelRepost then
        return "repost"
    elseif not lowestAuction then
        return "no_undercut"
    elseif lowestAuction.hasInvalidSeller then
        return "invalid"
    end
    -- All branches above return, so lowestAuction is non-nil here
    if lowestAuction.buyout > 100 then
        --               ^ diag: none
        return lowestAuction.seller
        --                   ^ diag: none
    end
    return ""
end
_consume(_elseifMultiBranchExit)

-- Nil union with typed function passed to generic function parameter
-- Should be need-check-nil, not type-mismatch
---@param callback function
local function _acceptsFunction(callback)
    callback()
end

---@type nil | fun(): boolean
local _maybeFunc

_acceptsFunction(_maybeFunc)
--               ^ diag: need-check-nil

---@type fun(): boolean
local _definiteFunc

_acceptsFunction(_definiteFunc)
--               ^ diag: none
_consume(_acceptsFunction, _maybeFunc, _definiteFunc)
