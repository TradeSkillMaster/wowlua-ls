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

-- ── self.field narrowing (if self.field then self.field:Method()) ────

---@class SelfFieldMenu
---@field Show fun(self: SelfFieldMenu)
---@field name string

---@class SelfFieldState
---@field subMenu SelfFieldMenu|nil

---@class SelfFieldObj
---@field public _state SelfFieldState

-- Basic: `if self.field then self.field:Method() end`
---@param self SelfFieldObj
local function testSelfFieldBasic(self)
    if self._state.subMenu then
        self._state.subMenu:Show()
        -- ^ diag: none
        self._state.subMenu.name = "ok"
        -- ^ diag: none
    end
end
_consume(testSelfFieldBasic)

-- Two-level: `if self.a.b then self.a.b:Method() end`
---@class SelfFieldDeepInner
---@field widget SelfFieldMenu|nil

---@class SelfFieldDeepState
---@field inner SelfFieldDeepInner

---@class SelfFieldDeepObj
---@field public _state SelfFieldDeepState

---@param self SelfFieldDeepObj
local function testSelfFieldTwoLevel(self)
    if self._state.inner.widget then
        self._state.inner.widget:Show()
        -- ^ diag: none
    end
end
_consume(testSelfFieldTwoLevel)

-- Early-exit: `if not self.field then return end; self.field:Method()`
---@param self SelfFieldObj
local function testSelfFieldEarlyExit(self)
    if not self._state.subMenu then
        return
    end
    self._state.subMenu:Show()
    -- ^ diag: none
    self._state.subMenu.name = "ok"
    -- ^ diag: none
end
_consume(testSelfFieldEarlyExit)

-- Early-exit with == nil: `if self.field == nil then return end`
---@param self SelfFieldObj
local function testSelfFieldEarlyExitEqNil(self)
    if self._state.subMenu == nil then
        return
    end
    self._state.subMenu:Show()
    -- ^ diag: none
end
_consume(testSelfFieldEarlyExitEqNil)

-- ~= nil guard: `if self.field ~= nil then`
---@param self SelfFieldObj
local function testSelfFieldNeqNil(self)
    if self._state.subMenu ~= nil then
        self._state.subMenu:Show()
        -- ^ diag: none
    end
end
_consume(testSelfFieldNeqNil)

-- Without guard: should still warn
---@param self SelfFieldObj
local function testSelfFieldNoGuard(self)
    self._state.subMenu:Show()
    -- ^ diag: need-check-nil
end
_consume(testSelfFieldNoGuard)

-- ── Early-return narrowing: if/elseif assign, else exits ──

-- Basic: else returns, all if/elseif assign → narrowed to non-nil
---@param condA boolean
---@param condB boolean
local function earlyReturnElseNarrow(condA, condB)
    local x = nil
    if condA then
        x = 1
    elseif condB then
        x = 2
    else
        return
    end
    local _ = x
    --        ^ hover: (local) x: number
end
_consume(earlyReturnElseNarrow)

-- Else calls error() instead of return → same narrowing
---@param condA boolean
---@param condB boolean
local function earlyReturnElseError(condA, condB)
    local x = nil
    if condA then
        x = "hello"
    elseif condB then
        x = "world"
    else
        error("unreachable")
    end
    local _ = x
    --        ^ hover: (local) x: string
end
_consume(earlyReturnElseError)

-- Single if + else returns → narrowed
---@param condA boolean
local function earlyReturnSingleIfElse(condA)
    local x = nil
    if condA then
        x = 42
    else
        return
    end
    local _ = x
    --        ^ hover: (local) x: number
end
_consume(earlyReturnSingleIfElse)

-- Mixed exits: if exits, elseif assigns, else exits → narrowed
---@param condA boolean
---@param condB boolean
local function earlyReturnMixedExits(condA, condB)
    local x = nil
    if condA then
        return
    elseif condB then
        x = 10
    else
        return
    end
    local _ = x
    --        ^ hover: (local) x: number
end
_consume(earlyReturnMixedExits)

-- Else doesn't exit, all branches assign → merged to union
---@param condA boolean
---@param condB boolean
local function noEarlyReturnElse(condA, condB)
    local x = nil
    if condA then
        x = 1
    elseif condB then
        x = 2
    else
        x = 3
    end
    local _ = x
    --        ^ hover: (local) x: number
end
_consume(noEarlyReturnElse)

-- Union type: different types assigned in each branch
---@param condA boolean
---@param condB boolean
local function earlyReturnUnionType(condA, condB)
    local x = nil
    if condA then
        x = 1
    elseif condB then
        x = "str"
    else
        return
    end
    local _ = x
    --        ^ hover: (local) x: number | string
end
_consume(earlyReturnUnionType)

-- ── Assert narrows field chains for need-check-nil ──────────────────────

---@class AssertFieldNilCheck
---@field _data string?
---@field Show fun(self: AssertFieldNilCheck)

---@class AssertFieldContainer
---@field public _child AssertFieldNilCheck?

-- Basic: assert(self.field) then use field
---@param self AssertFieldContainer
local function testAssertFieldNarrow(self)
    assert(self._child)
    self._child:Show()
    -- ^ diag: none
end
_consume(testAssertFieldNarrow)

-- Hover shows narrowed type after assert
---@param self AssertFieldContainer
local function testAssertFieldHover(self)
    assert(self._child)
    local x = self._child
    --    ^ hover: (local) x: AssertFieldNilCheck {
end
_consume(testAssertFieldHover)

-- `if self.field then` narrows hover type
---@param self AssertFieldContainer
local function testIfFieldHover(self)
    if self._child then
        local x = self._child
        --    ^ hover: (local) x: AssertFieldNilCheck {
    end
end
_consume(testIfFieldHover)

-- `if not self.field then return end` narrows hover type
---@param self AssertFieldContainer
local function testEarlyExitFieldHover(self)
    if not self._child then return end
    local x = self._child
    --    ^ hover: (local) x: AssertFieldNilCheck {
end
_consume(testEarlyExitFieldHover)

-- ── Field narrowing propagates through local variable assignment ────

-- Early-exit guard: `if not self.field then return end; local x = self.field`
---@class FieldNarrowContainer
---@field Hide fun(self: FieldNarrowContainer)

---@class FieldNarrowState
---@field frame FieldNarrowContainer|nil

---@param state FieldNarrowState
local function testFieldNarrowLocalEarlyExit(state)
    if not state.frame then return end
    local frame = state.frame
    --    ^ hover: (local) frame: FieldNarrowContainer
    frame:Hide()
    -- ^ diag: none
end
_consume(testFieldNarrowLocalEarlyExit)

-- Truthiness guard: `if self.field then local x = self.field end`
---@param state FieldNarrowState
local function testFieldNarrowLocalTruthiness(state)
    if state.frame then
        local frame = state.frame
        --    ^ hover: (local) frame: FieldNarrowContainer
        frame:Hide()
        -- ^ diag: none
    end
end
_consume(testFieldNarrowLocalTruthiness)

-- Without guard: local should retain nullable type
---@param state FieldNarrowState
local function testFieldNarrowLocalNoGuard(state)
    local frame = state.frame
    --    ^ hover: (local) frame: FieldNarrowContainer | nil
    frame:Hide()
    -- ^ diag: need-check-nil
end
_consume(testFieldNarrowLocalNoGuard)

-- Type-mismatch: narrowed field assigned to local, passed to non-nil param
---@class FieldNarrowPathObj
---@field _path string|nil

---@param path string
local function _acceptPath(path) _consume(path) end

---@param self FieldNarrowPathObj
local function testFieldNarrowTypeMismatch(self)
    if self._path then
        local oldPath = self._path
        --    ^ hover: (local) oldPath: string
        _acceptPath(oldPath)
        -- ^ diag: none
    end
end
_consume(testFieldNarrowTypeMismatch, _acceptPath)

-- ── Calling a possibly-nil function value ───────────────────────────────

---@class NilCallObj
---@field public _callback nil | fun(self: NilCallObj, path: string): NilCallObj

-- Direct call on nullable field without guard — should warn
---@param self NilCallObj
local function testNilCallNoGuard(self)
    self:_callback("test")
    -- ^ diag: need-check-nil
end
_consume(testNilCallNoGuard)

-- Call inside if-guard — should suppress
---@param self NilCallObj
local function testNilCallGuarded(self)
    if self._callback then
        self:_callback("test")
        -- ^ diag: none
    end
end
_consume(testNilCallGuarded)

-- Call after assert — should suppress
---@param self NilCallObj
local function testNilCallAssert(self)
    assert(self._callback)
    self:_callback("test")
    -- ^ diag: none
end
_consume(testNilCallAssert)

-- Call after early-exit guard — should suppress
---@param self NilCallObj
local function testNilCallEarlyExit(self)
    if not self._callback then return end
    self:_callback("test")
    -- ^ diag: none
end
_consume(testNilCallEarlyExit)

-- Local variable with nullable function type — should warn
---@type nil | fun(): string
local maybeFunc = nil
maybeFunc()
-- ^ diag: need-check-nil

-- Local variable guarded — should suppress
if maybeFunc then
    maybeFunc()
    -- ^ diag: none
end

-- `and`-guard should suppress call-on-nil for field calls
---@class AndCallGuardClass
---@field callback (fun(self: AndCallGuardClass))?
---@field dotCallback (fun(...))?
local AndCallGuardObj = {}

function AndCallGuardObj:testColon()
    local _ = self.callback and self:callback()
    -- ^ diag: none
end

function AndCallGuardObj:testDot()
    local _ = self.dotCallback and self.dotCallback("a", "b")
    -- ^ diag: none
end

-- Without guard — should still warn
function AndCallGuardObj:testMissing()
    self:callback()
    -- ^ diag: need-check-nil
end

-- ── Break as early exit for nil narrowing ──────────────────────────────────

---@type NilCheckFrame|nil
local breakItem = nil

for i = 1, 10 do
    if not breakItem then break end
    breakItem:Show()
    -- ^ diag: none
end

-- Break after reassignment inside preceding if-block
---@type table<string, NilCheckFrame>
local breakRows = {}
while true do
    local baseItem, breakRow = nil, nil
    if baseItem then
        breakRow = baseItem and breakRows[baseItem]
    end
    if not breakRow then
        baseItem, breakRow = next(breakRows)
    end
    if not breakRow then
        break
    end
    breakRow:Show()
    -- ^ diag: none
end

-- ── and-narrowing suppresses need-check-nil for function args ────────

---@param n number
local function takeNum(n) return n end

---@param x number?
local function testAndNarrowArg(x)
    local _ = x and takeNum(x)
    -- ^ diag: none
end

---@param x number?
local function testNoAndGuard(x)
    local _ = takeNum(x)
    -- ^ diag: need-check-nil
end

-- Hover shows narrowed type inside `and` RHS
---@param x number?
local function testAndNarrowHover(x)
    local _ = x and takeNum(x)
    --                      ^ hover: (param) x: number
end

-- Chained `and` narrows all operands
---@param x number?
---@param y number?
local function testAndChainNarrow(x, y)
    local _ = x and y and takeNum(x)
    -- ^ diag: none
end

-- ── If-block reassignment merged back to outer scope ────────────────────
-- Regression: BranchMerge produced a partial union (just nil) when the
-- branch assignment hadn't resolved yet during the fixpoint loop.

---@class IfBlockMergeObj
---@field name string
local IfBlockMergeObj = {}
---@return string
function IfBlockMergeObj:GetName()
    return self.name
end

---@return IfBlockMergeObj
local function createObj()
    ---@type IfBlockMergeObj
    return {}
end

do
    -- Simple case: reassignment inside if-block
    local x = nil
    if true then
        x = "hello"
    end
    local _chk1 = x
    --    ^ hover: (local) _chk1: string | nil

    -- Inside a for-loop with function call assignment
    local currentObj = nil
    for i = 1, 10 do
        if i > 5 then
            currentObj = createObj()
        end
        local _chk2 = currentObj
        --    ^ hover: (local) _chk2: IfBlockMergeObj | nil
    end

    -- After a for-loop the merge is also visible
    local _chk3 = currentObj
    --    ^ hover: (local) _chk3: IfBlockMergeObj | nil
end

-- ── @correlated: type-mismatch suppression via sibling narrowing ─────────────

---@param str string
---@param num number
local function _takeStrNum(str, num) end

---@class CorrTypeMismatch
---@correlated name, count
---@field name string?
---@field count number?
---@field cache string?
local CorrTypeMismatch = {}

-- Guard on one correlated field narrows siblings (no type-mismatch)
---@param self CorrTypeMismatch
local function useCorrTypeMismatch(self)
    if self.name then
        _takeStrNum(self.name, self.count)
        -- ^ diag: none
    end
end

-- Non-correlated field still produces type-mismatch
---@param self CorrTypeMismatch
local function uncorrelatedStillWarns(self)
    if self.name then
        _consume(self.cache)
        --            ^ diag: none
    end
end

-- ── @correlated: direct self.field with early-exit ───────────────────────────

---@class DirectCorr
---@correlated handler, money
---@field handler function?
---@field money number?
---@field label string?
local DirectCorr = {}

---@param n number
local function _takeNum(n) end

-- Early-exit narrows correlated siblings in parent scope
---@param self DirectCorr
local function earlyExitCorr(self)
    if not self.handler then return end
    _takeNum(self.money)
    -- ^ diag: none
end

-- ── @correlated: multiple groups ─────────────────────────────────────────────

---@class MultiGroup
---@correlated a, b
---@correlated c, d
---@field a string?
---@field b number?
---@field c boolean?
---@field d string?
local MultiGroup = {}

---@param str string
---@param num number
---@param bool boolean
local function _takeTypes(str, num, bool) end

-- Narrowing group1 does not narrow group2
---@param self MultiGroup
local function multiGroupTest(self)
    if self.a then
        _takeTypes(self.a, self.b, self.c)
        --                         ^ diag: need-check-nil
    end
end

-- ── @correlated: inherited from parent class ─────────────────────────────────

---@class CorrParent
---@correlated x, y
---@field x string?
---@field y number?

---@class CorrChild : CorrParent
---@field z boolean

---@param self CorrChild
local function useInheritedCorr(self)
    if self.x then
        _takeStrNum(self.x, self.y)
        -- ^ diag: none
    end
end

-- ── @correlated: nested field chain (obj.sub.field) ──────────────────────────

---@class CorrAuction
---@correlated itemString, duration, buyout
---@field itemString string?
---@field duration number?
---@field buyout number?

---@class CorrHolder
---@field auction CorrAuction

---@param self CorrHolder
local function useNestedCorr(self)
    if self.auction.itemString then
        _takeStrNum(self.auction.itemString, self.auction.duration)
        -- ^ diag: none
    end
end

-- ── @correlated: falsy guard narrows siblings in else-branch ─────────────────

---@class FalsyCorr
---@correlated active, data
---@field active boolean?
---@field data string?
local FalsyCorr = {}

---@param s string
local function _takeStr(s) end

---@param self FalsyCorr
local function falsyCorrTest(self)
    if not self.active then
        -- Inside negated guard: active is nil|false, data should NOT be narrowed
        _consume(self.data)
        --            ^ diag: none
    else
        -- Else-branch: active is truthy, correlated siblings are also narrowed
        _takeStr(self.data)
        -- ^ diag: none
    end
end

---@class MergeRetTest
---@field _hasName boolean
---@field _name string
local MergeRetTest = {}
---@return string
function MergeRetTest:GetName()
    local result = nil
    if self._hasName then
        result = self._name
    end
    return result
    -- ^ diag: return-mismatch
end
