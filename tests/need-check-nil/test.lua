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
--         ^ hover: (local) f1: NilCheckFrame?

-- ── Nil guard with bare name ─────────────────────────────────────────────

if f1 then
    f1.name = "hello"
    -- ^ diag: none
    local _ = f1
    --        ^ hover: (local) f1: NilCheckFrame {
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
--        ^ hover: (local) f8: NilCheckFrame {

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
--        ^ hover: (local) f9: NilCheckFrame {

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
    --        ^ hover: (local) f11: NilCheckFrame {
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
--                 ^ hover: (local) f13: NilCheckFrame {
    local _ = f13
    --        ^ hover: (local) f13: NilCheckFrame {
end
-- hover outside guard shows full union
local _ = f13
--        ^ hover: (local) f13: NilCheckFrame?

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
--        ^ hover: (local) nilCheckFalsy1: string

---@type string|false
local nilCheckFalsy2 = false
if nilCheckFalsy2 then
    local _ = nilCheckFalsy2
    --        ^ hover: (local) nilCheckFalsy2: string
end

-- `x ~= nil` should NOT strip false, only nil
---@type string|false|nil
local nilCheckFalsy3 = false
if nilCheckFalsy3 ~= nil then
    local _ = nilCheckFalsy3
    --        ^ hover: (local) nilCheckFalsy3: string | false
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
--        ^ hover: (local) thenNarrow1: string?

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
--        ^ hover: (local) query1: BranchQuery

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
    --    ^ hover: (local) frame: FieldNarrowContainer?
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
    --    ^ hover: (local) _chk1: string?

    -- Inside a for-loop with function call assignment
    local currentObj = nil
    for i = 1, 10 do
        if i > 5 then
            currentObj = createObj()
        end
        local _chk2 = currentObj
        --    ^ hover: (local) _chk2: IfBlockMergeObj?
    end

    -- After a for-loop the merge is also visible
    local _chk3 = currentObj
    --    ^ hover: (local) _chk3: IfBlockMergeObj?
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

-- ── @correlated: nested chain fields as function call arguments ──────────────

---@class CorrAuction2
---@correlated itemString, duration, bid, buyout, stackSize, undercut
---@field itemString string?
---@field duration number?
---@field bid number?
---@field buyout number?
---@field stackSize number?
---@field undercut number?

---@class CorrHolder2
---@field _auction CorrAuction2

---@param s string
---@param d number
---@param b number
---@param bo number
---@param st number
---@param u number
local function _takeSixArgs(s, d, b, bo, st, u) end

---@param self CorrHolder2
local function useNestedCorrAsArgs(self)
    if self._auction.itemString then
        _takeSixArgs(self._auction.itemString, self._auction.duration, self._auction.bid, self._auction.buyout, self._auction.stackSize, self._auction.undercut)
        -- ^ diag: none
    end
end

-- ── @correlated: nested chain with method call (colon syntax) ────────────────

---@class CorrFrame
---@field SetAuction fun(self: CorrFrame, s: string, d: number, b: number, bo: number, st: number, u: number)
local CorrFrame = {}

---@class CorrDialogWithMethod
---@field _frame CorrFrame
---@field _auction CorrAuction2

function CorrDialogWithMethod:PostAuction()
    if self._auction.itemString then
        self._frame:SetAuction(self._auction.itemString, self._auction.duration, self._auction.bid, self._auction.buyout, self._auction.stackSize, self._auction.undercut)
        -- ^ diag: none
    end
end

-- ── @correlated: nested chain with runtime-typed field via @type ──────────────

---@class CorrDialogRuntime
local CorrDialogRuntime = {}

function CorrDialogRuntime:__init()
    self._auction = { ---@type CorrAuction2
        itemString = nil,
        duration = nil,
        bid = nil,
        buyout = nil,
        stackSize = nil,
        undercut = nil,
    }
end

function CorrDialogRuntime:DoPost()
    if self._auction.itemString then
        _takeSixArgs(self._auction.itemString, self._auction.duration, self._auction.bid, self._auction.buyout, self._auction.stackSize, self._auction.undercut)
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

-- ── Early return path elimination for locals ───────────────────────────

-- Basic: if not x then ... assign ... end with nested early return
---@param info {title?: string}
local function earlyReturnNarrow(info)
    local title = nil ---@type string?
    if true then
        title = "foo"
    end
    if not title then
        if not info.title then return end
        title = info.title
    end
    local ern1 = title
    --    ^ hover: (local) ern1: string
end

-- x == nil guard variant
---@param info {title?: string}
local function eqNilNarrow(info)
    local title = nil ---@type string?
    if true then
        title = "foo"
    end
    if title == nil then
        if not info.title then return end
        title = info.title
    end
    local enn1 = title
    --    ^ hover: (local) enn1: string
end

-- Multiple assignment paths: all branches assign or return
---@param a string?
---@param b string?
local function multiPathNarrow(a, b)
    local result = nil ---@type string?
    if not result then
        if a then
            result = a
        elseif b then
            result = b
        else
            return
        end
    end
    local mpn1 = result
    --    ^ hover: (local) mpn1: string
end

-- Simple: if not x then x = val end (no early return needed)
local function simpleAssignNarrow()
    local x = nil ---@type string?
    if not x then
        x = "hello"
    end
    local san1 = x
    --    ^ hover: (local) san1: string
end

-- ═══════════════════════════════════════════════════════════
-- Edge cases from LuaLS need-check-nil test comparison
-- ═══════════════════════════════════════════════════════════

-- ── Method call on nullable type (basic case, already covered above but adding explicit label) ──

---@class NilEdge
---@field handler? fun()

---@type NilEdge
local nilEdge

-- Direct invocation of nullable function field
nilEdge.handler()
--      ^ diag: need-check-nil

-- Guarded invocation is safe
if nilEdge.handler then
    nilEdge.handler()
    --      ^ diag: none
end

-- ── Negation guard + reassignment pattern ──

---@type string?
local maybeStr = nil
if not maybeStr then maybeStr = "fallback" end
local useStr = maybeStr
--    ^ hover: (local) useStr: string  def: local

-- ── Mutual recursion should not cause false positives ──

local function mutualA()
    return mutualB()
end
function mutualB()
    return mutualA()
end
-- No need-check-nil diagnostics should fire on these
_consume(mutualA, mutualB)

-- ── While loop with reassignment narrows across iterations ──

---@class NilLinkedNode
---@field next? NilLinkedNode
---@field value number

---@type NilLinkedNode?
local linkedNode = nil
while linkedNode do
    _consume(linkedNode.value)
    --       ^ diag: none
    linkedNode = linkedNode.next
end

-- ── While loop exit narrows condition variable ──────────────────────────

-- Basic: `while not x do ... end` → x is non-nil after

---@type string?
local whileNarrow1 = nil
while not whileNarrow1 do
    whileNarrow1 = "found"
end
local wn1 = whileNarrow1
--    ^ hover: (local) wn1: string

-- Method call after while loop should not warn
---@type NilCheckFrame?
local whileFrame = nil
while not whileFrame do
    ---@type NilCheckFrame
    whileFrame = {}
end
whileFrame:Show()
-- ^ diag: none

-- Nil comparison: `while x == nil do ... end` → x is non-nil after

---@type string?
local whileNarrow2 = nil
while whileNarrow2 == nil do
    whileNarrow2 = "found"
end
local wn2 = whileNarrow2
--    ^ hover: (local) wn2: string

-- Complex condition: `while not x or cond do ... end` → x is non-nil after

---@type string?
local whileNarrow3 = nil
---@type boolean
local whileCond = true
while not whileNarrow3 or whileCond do
    whileNarrow3 = "found"
    whileCond = false
end
local wn3 = whileNarrow3
--    ^ hover: (local) wn3: string

-- While true with break: should NOT narrow (break exits without condition being false)

---@type string?
local whileNoNarrow1 = nil
while true do
    whileNoNarrow1 = "found"
    break
end
local wnn1 = whileNoNarrow1
--    ^ hover: (local) wnn1: string?

-- While with break inside if: should NOT narrow

---@type string?
local whileNoNarrow2 = nil
while not whileNoNarrow2 do
    whileNoNarrow2 = "found"
    if true then break end
end
local wnn2 = whileNoNarrow2
--    ^ hover: (local) wnn2: string?

-- Break inside nested loop should NOT prevent narrowing of outer while

---@type string?
local whileNarrow4 = nil
while not whileNarrow4 do
    for i = 1, 5 do
        break
    end
    whileNarrow4 = "found"
end
local wn4 = whileNarrow4
--    ^ hover: (local) wn4: string

-- And condition: `while a == nil and b == nil do` → NOT narrowed
-- (exit means NOT(a==nil AND b==nil) = a~=nil OR b~=nil; only one guaranteed)

---@type string?
local whileAndA = nil
---@type number?
local whileAndB = nil
while whileAndA == nil and whileAndB == nil do
    whileAndA = "x"
    whileAndB = 1
end
local wna = whileAndA
--    ^ hover: (local) wna: string?
local wnb = whileAndB
--    ^ hover: (local) wnb: number?

-- ── @param function type not contaminated by nullable field assignment ───

---@class ParamCallHolder
---@field _func nil | fun(): number?
local ParamCallHolder = {}

---@param func fun(): number?
function ParamCallHolder:SetFunc(func)
    self._func = func
    local result = func()
    --             ^ diag: unused-local
    --      ^ hover: (local) result: number?
end

-- Edge case: fun()? — nullable void function (? on the function itself)
---@param maybeVoid fun()?
function ParamCallHolder:CallMaybeVoid(maybeVoid)
    maybeVoid()
    -- ^ diag: need-check-nil
end

-- Edge case: fun(x: number)? — `:` inside parens at depth 1, ? on function
---@param maybeTyped fun(x: number)?
function ParamCallHolder:CallMaybeTyped(maybeTyped)
    maybeTyped(1)
    -- ^ diag: need-check-nil
end

-- Edge case: fun(x: number): string? — param has `:` at depth 1, return has ?
---@param strFunc fun(x: number): string?
function ParamCallHolder:CallStrFunc(strFunc)
    local s = strFunc(1)
    --    ^ hover: (local) s: string?
    --        ^ diag: none
    _consume(s)
end

-- ── And-expression field chain narrowing ───────────────────────────────

---@class AndFieldTest
---@field _data string?
---@field _sub AndFieldTestSub?
local AndFieldTest = {}

---@class AndFieldTestSub
---@field value number?

---@param s string
---@return number
local function _strLen(s) return #s end

function AndFieldTest:TestFieldAnd()
    -- Field access narrowed through `and` (bare truthiness guard)
    local _ = self._data and _strLen(self._data) or 0
    --                                    ^ diag: none

    -- Nested field access: `self._sub and self._sub.value`
    local _ = self._sub and self._sub.value or nil
    --                            ^ diag: none

    -- Field ~= nil guard in and: `self._data ~= nil and ...`
    local _ = self._data ~= nil and _strLen(self._data) or 0
    --                                       ^ diag: none

    -- After the and-expression, field should NOT be narrowed
    local _ = self._data
    --             ^ hover: (field) _data: string?
end

-- Chained field-and: `self._data and self._sub and func(self._data, self._sub)`
function AndFieldTest:TestChainedFieldAnd()
    local _ = self._data and self._sub and _strLen(self._data) or 0
    --                                              ^ diag: none
end

-- Chained ~= nil guards through and (StripNil path)
function AndFieldTest:TestNilCheckChain()
    ---@type number
    local _ = self._sub ~= nil and self._sub.value ~= nil and self._sub.value or 0
    --                                  ^ diag: none
end

-- Mixed bare-truthiness and ~= nil in chain
function AndFieldTest:TestMixedChain()
    local _ = self._sub and self._sub.value ~= nil and self._sub.value or 0
    --                           ^ diag: none
end

-- ── Multi-level field chain narrowing in and-expressions ───────────────

---@class MultiLevelNarrowState
---@field icon? string

---@class MultiLevelNarrowTest
---@field state MultiLevelNarrowState

---@type MultiLevelNarrowTest
local mlnt = {}

-- Two-level chain: bare truthiness guard
local _ = mlnt.state.icon and _strLen(mlnt.state.icon)
--                                              ^ diag: none

-- Two-level chain: ~= nil guard
local _ = mlnt.state.icon ~= nil and _strLen(mlnt.state.icon)
--                                                     ^ diag: none

-- ── `x = x or y` coalesce narrowing ──────────────────────────────────────
-- When `x = x or y` is assigned, narrowing `y` (non-nil) propagates to `x`.

---@param s string
---@return number
local function _takeString(s) return #s end

---@param link string|nil
---@param itemLinkIn string|nil
local function _coalesceOr(link, itemLinkIn)
    local itemLink = itemLinkIn
    itemLink = itemLink or link
    -- Inside `or` RHS: link narrowed truthy, so itemLink (coalesce-derived) is too.
    if not link or _takeString(itemLink) ~= _takeString(link) then
        --                        ^ diag: none
        return itemLink
    end
    return itemLink
end
_consume(_coalesceOr)

---@param link string|nil
---@param itemLinkIn string|nil
local function _coalesceOrAndChain(link, itemLinkIn)
    local itemLink = itemLinkIn
    itemLink = itemLink or link
    -- Inside `and` RHS: link narrowed truthy → itemLink also narrowed.
    local _ = link and _takeString(itemLink)
    --                                 ^ diag: none
    return _
end
_consume(_coalesceOrAndChain)

---@param link string|nil
---@param itemLinkIn string|nil
local function _coalesceNoNarrowYet(link, itemLinkIn)
    local itemLink = itemLinkIn
    itemLink = itemLink or link
    -- Without narrowing `link`, `itemLink` is still possibly nil.
    return _takeString(itemLink)
    --                    ^ diag: need-check-nil
end
_consume(_coalesceNoNarrowYet)

---@param y string
---@param xIn string|nil
local function _coalesceFromNonNilSource(y, xIn)
    local x = xIn
    -- `y` is annotated non-nil; `x = x or y` narrows x inside a guard on y.
    x = x or y
    if y ~= nil then
        return _takeString(x)
        --                    ^ diag: none
    end
    return x
end
_consume(_coalesceFromNonNilSource)

---@param y string|nil
---@param xIn string|nil
---@param other string|nil
local function _coalesceInvalidatedByReassign(y, xIn, other)
    local x = xIn
    x = x or y
    x = other
    -- After reassignment to `other`, the (y → x) derivation no longer applies.
    if y ~= nil then
        return _takeString(x)
        --                    ^ diag: need-check-nil
    end
    return x
end
_consume(_coalesceInvalidatedByReassign)

---@param y string|nil
---@param xIn string|nil
local function _coalesceAssertOnSource(y, xIn)
    local x = xIn
    x = x or y
    assert(y)
    return _takeString(x)
    --                    ^ diag: none
end
_consume(_coalesceAssertOnSource)

---@param y string|nil
---@param z string|nil
---@param xIn string|nil
---@param yIn string|nil
local function _coalesceChainedNoHop(y, z, xIn, yIn)
    local x = xIn
    local yLocal = yIn
    x = x or y
    yLocal = yLocal or z
    -- Narrowing `z` narrows `yLocal` (direct), not `x` (no transitive hop).
    if z ~= nil then
        _consume(_takeString(yLocal))
        return _takeString(x)
        --                    ^ diag: need-check-nil
    end
    return x
end
_consume(_coalesceChainedNoHop)

---@type string|nil
local _coalesceOuterX = nil

---@param y string|nil
local function _coalesceLocalDeclNoRegister(y)
    -- `local x = _coalesceOuterX or y` declares a NEW local x. Registration
    -- runs only on simple-assignment statements, not local decls — so
    -- narrowing `y` does NOT narrow the new `x`.
    local x = _coalesceOuterX or y
    if y ~= nil then
        return _takeString(x)
        --                    ^ diag: need-check-nil
    end
    return x
end
_consume(_coalesceLocalDeclNoRegister)

-- ── `y = x and _ or nil` coalesce narrowing ──────────────────────────────
-- Narrowing `y` non-nil implies `x` is truthy (because the trailing `or nil`
-- forces `y` to nil whenever `x` is falsy).

---@param link string|nil
local function _andOrNilLocalDecl(link)
    local itemString = link and _takeString(link) or nil
    if not itemString then
        return 0
    end
    -- itemString narrowed non-nil → link also narrowed.
    return _takeString(link)
    --                    ^ diag: none
end
_consume(_andOrNilLocalDecl)

---@param link string|nil
local function _andOrNilReassign(link)
    ---@type number|nil
    local itemString = nil
    itemString = link and _takeString(link) or nil
    if itemString == nil then
        return 0
    end
    return _takeString(link)
    --                    ^ diag: none
end
_consume(_andOrNilReassign)

---@param link string|nil
---@param other string|nil
local function _andOrNilInvalidatedByReassign(link, other)
    local itemString = link and _takeString(link) or nil
    itemString = other and 1 or nil
    -- The new assignment re-registers (itemString → other); the original
    -- (itemString → link) derivation is gone.
    if itemString ~= nil then
        return _takeString(link)
        --                    ^ diag: need-check-nil
    end
    return 0
end
_consume(_andOrNilInvalidatedByReassign)

---@param link string|nil
local function _andOrNilInvalidatedByPlainReassign(link)
    ---@type number|nil
    local itemString = link and _takeString(link) or nil
    itemString = 42
    -- The plain reassignment matches no coalesce pattern, so the prior
    -- (itemString → link) derivation is cleared without being replaced.
    if itemString ~= nil then
        return _takeString(link)
        --                    ^ diag: need-check-nil
    end
    return 0
end
_consume(_andOrNilInvalidatedByPlainReassign)

-- Transitive narrowing: narrowing a correlated-local sibling should propagate
-- through the coalesce derivation attached to its partner.
---@param cond boolean
---@param aIn string|nil
---@param bIn string|nil
---@param xIn string|nil
local function _coalesceViaCorrelated(cond, aIn, bIn, xIn)
    local a, b
    if cond then
        a = aIn
        b = bIn
    elseif not cond then
        a = "a"
        b = "b"
    end
    -- After the implicit-else if/elseif chain, a and b are correlated.
    local x = xIn
    x = x or b
    if a ~= nil then
        -- a narrowed → correlated narrows b → coalesce narrows x.
        return _takeString(x)
        --                    ^ diag: none
    end
    return x
end
_consume(_coalesceViaCorrelated)

-- `y = x and _ or nil` then-branch narrowing: `if y then` narrows `x` too.
---@param sel string|nil
local function _andOrNilThenBranch(sel)
    local idx = sel and #sel or nil
    if idx then
        return _takeString(sel)
        --                    ^ diag: none
    end
    return 0
end
_consume(_andOrNilThenBranch)

-- `y = x and _ or nil` then-branch narrowing via `y ~= nil`.
---@param sel string|nil
local function _andOrNilThenBranchNeqNil(sel)
    local idx = sel and #sel or nil
    if idx ~= nil then
        return _takeString(sel)
        --                    ^ diag: none
    end
    return 0
end
_consume(_andOrNilThenBranchNeqNil)

-- Narrowing the source does NOT narrow the derived (one-directional).
---@param sel string|nil
local function _andOrNilSourceNotNarrowedByDerived(sel)
    local idx = sel and #sel or nil
    if sel then
        -- Narrowing `sel` does NOT narrow `idx` — `idx` could still be nil
        -- if `#sel` evaluated to nil (hypothetically).
        return idx
        --     ^ hover: (local) idx: number?
    end
    return 0
end
_consume(_andOrNilSourceNotNarrowedByDerived)

-- `y = x and _ or nil` via `if type(y) ~= "nil" then` guard.
---@param sel string|nil
local function _andOrNilTypeNotNil(sel)
    local idx = sel and #sel or nil
    if type(idx) ~= "nil" then
        return _takeString(sel)
        --                    ^ diag: none
    end
    return 0
end
_consume(_andOrNilTypeNotNil)

-- `y = x and _ or nil` via `if type(y) == "T" then` positive type guard.
---@param sel string|nil
local function _andOrNilTypePositive(sel)
    local idx = sel and #sel or nil
    if type(idx) == "number" then
        return _takeString(sel)
        --                    ^ diag: none
    end
    return 0
end
_consume(_andOrNilTypePositive)

-- `y = x and _ or nil` via `assert(y ~= nil)`.
---@param sel string|nil
local function _andOrNilAssertNeqNil(sel)
    local idx = sel and #sel or nil
    assert(idx ~= nil)
    return _takeString(sel)
    --                    ^ diag: none
end
_consume(_andOrNilAssertNeqNil)

-- `y = x and _ or nil` via `assert(type(y) == "T")`.
---@param sel string|nil
local function _andOrNilAssertType(sel)
    local idx = sel and #sel or nil
    assert(type(idx) == "number")
    return _takeString(sel)
    --                    ^ diag: none
end
_consume(_andOrNilAssertType)

-- `y = x and _ or nil` via `if y == A or y == B` union narrowing.
---@param kind "a" | "b" | "c" | nil
local function _andOrNilOrTermUnion(kind)
    local idx = kind and 5 or nil
    if idx == 5 or idx == 6 then
        -- idx narrowed to `5 | 6` (no nil) → kind also narrowed truthy.
        return _takeString(kind)
        --                    ^ hover: (param) kind: "a" | "b" | "c"
    end
    return "x"
end
_consume(_andOrNilOrTermUnion)

-- ── Union dedup: `x = x or {}` across if/elseif branches ─────────────────
-- Regression for union-type deduplication: separate `{}` literals across
-- branches produce distinct TableIndex values, but they all render as
-- `table` and should collapse in the resulting union.
---@param takeTable fun(t: table)
---@param cond1 boolean
---@param cond2 boolean
local function _dedupOrAssignTable(takeTable, cond1, cond2)
    local t = nil ---@type table?
    if cond1 then
        t = t or {}
    elseif cond2 then
        t = t or {}
    end
    local u = t
    --    ^ hover: (local) u: table?
    if t then
        takeTable(t)
        --         ^ diag: none
    end
end
_consume(_dedupOrAssignTable)

-- ──────────────────────────────────────────────────────────────────────────
-- Tuple-union `(...any) | ()` sibling narrowing through the deferred path.
-- The callee is a FieldAccess whose base is a function-call result, so
-- build-ir can't resolve it; narrowing runs during the resolve fixpoint
-- and must rewrite existing sibling SymbolRefs so later uses see the
-- narrowed (non-nil) type and don't emit `need-check-nil`.
-- ──────────────────────────────────────────────────────────────────────────

---@class DeferredQuery
local DeferredQuery = {}

---@param ... string
---@return (number? uuid, ...any) | (nil)
function DeferredQuery:Get(...) end

---@param ... string
---@return (...any) | ()
function DeferredQuery:GetNth(...) end

---@return DeferredQuery
local function _getDeferredQuery() return DeferredQuery end

local function _deferredSiblingAssert()
    local uuid, name, count = _getDeferredQuery():Get("name", "count")
    if not uuid then return end
    -- Post-guard, siblings narrow from `any | nil` to `any`. `any` satisfies
    -- every annotated param type, so no `need-check-nil` fires.
    _takeString(name)
    --          ^ diag: none
    _takeNum(count)
    --       ^ diag: none
end
_consume(_deferredSiblingAssert)

local function _deferredSiblingBare()
    local a, b, c = _getDeferredQuery():GetNth("x", "y")
    if a then
        _takeString(b)
        --          ^ diag: none
        _takeNum(c)
        --       ^ diag: none
    end
end
_consume(_deferredSiblingBare)

-- ── Bracket-access field chain narrowing ─────────────────────────────────

---@class BracketReagent
---@field itemID number|nil

---@class BracketSlotInfo
---@field reagents BracketReagent[]
---@field first BracketReagent|nil

---@param _x number
local function _takeNum(_x) end

-- Early-exit guard: `if not obj.arr[1].field then return end`
---@param info BracketSlotInfo
local function testBracketAccessEarlyExit(info)
    if not info.reagents[1].itemID then return end
    _takeNum(info.reagents[1].itemID)
    --                          ^ diag: none
end
_consume(testBracketAccessEarlyExit)

-- Truthiness guard: `if obj.arr[1].field then ... end`
---@param info BracketSlotInfo
local function testBracketAccessTruthiness(info)
    if info.reagents[1].itemID then
        _takeNum(info.reagents[1].itemID)
        --                          ^ diag: none
    end
end
_consume(testBracketAccessTruthiness)

-- Without guard, still warns (nil not narrowed)
---@param info BracketSlotInfo
local function testBracketAccessNoGuard(info)
    _takeNum(info.reagents[1].itemID)
    --                          ^ diag: need-check-nil
end
_consume(testBracketAccessNoGuard)

-- Comparison guard: `obj.arr[1].field == nil`
---@param info BracketSlotInfo
local function testBracketAccessNilCompare(info)
    if info.reagents[1].itemID == nil then return end
    _takeNum(info.reagents[1].itemID)
    --                          ^ diag: none
end
_consume(testBracketAccessNilCompare)

-- Bracket access in intermediate position: need-check-nil on nullable bracket result
---@param info BracketSlotInfo
local function testBracketNullableBase(info)
    if not info.first then return end
    info.first.itemID = 123
    -- ^ diag: none
end
_consume(testBracketNullableBase)

-- ── Nil init + complete if/else: initial nil is dead ───────────────────

-- When a local is initialized as nil and assigned in both branches of a
-- complete if/else, the initial nil should not contaminate the merged type.

---@param cond boolean
---@param id number
local function nilInitCompleteIfElse(cond, id)
    local x = nil
    if cond then
        x = "hello"
    else
        x = "world"
    end
    _takeStr(x)
    --       ^ diag: none
    --       ^ hover: (local) x: string
end
_consume(nilInitCompleteIfElse)

-- Nil init + early-exit narrowing: `if not x then return end` inside branch

---@param cond boolean
---@param spellId number
local function nilInitEarlyExit(cond, spellId)
    local indirectId = nil
    if cond then
        indirectId = tonumber("123")
        if not indirectId then return end
    else
        indirectId = spellId
    end
    _takeNum(indirectId)
    --       ^ diag: none
    --       ^ hover: (local) indirectId: number
end
_consume(nilInitEarlyExit)

-- Nil init + assert narrowing inside else branch

---@return string?
local function _optStr() return nil end

---@param cond boolean
local function nilInitAssertNarrow(cond)
    local itemStr = nil
    if cond then
        itemStr = "item:123"
    else
        itemStr = _optStr()
        assert(itemStr)
    end
    _takeStr(itemStr)
    --       ^ diag: none
    --       ^ hover: (local) itemStr: string
end
_consume(nilInitAssertNarrow)

-- ── (x or literal) comparison value: indirect nil narrowing ─────────────

-- Basic: `(x or 0) > 0` narrows x to non-nil
---@param x number?
local function testOrCoercionGt(x)
    if (x or 0) > 0 then
        local y = x
        --        ^ hover: (param) x: number
    end
end
_consume(testOrCoercionGt)

-- `and` chain: `a and (b or 0) > 0` narrows both
---@param a string?
---@param b number?
local function testOrCoercionAndChain(a, b)
    if a and (b or 0) > 0 then
        local y = a
        --        ^ hover: (param) a: string
        local z = b
        --        ^ hover: (param) b: number
    end
end
_consume(testOrCoercionAndChain)

-- Flipped comparison: `0 < (x or 0)` — same semantics
---@param x number?
local function testOrCoercionFlipped(x)
    if 0 < (x or 0) then
        local y = x
        --        ^ hover: (param) x: number
    end
end
_consume(testOrCoercionFlipped)

-- Should NOT narrow when fallback satisfies the comparison: `(x or 5) > 0`
---@param x number?
local function testOrCoercionNoNarrow(x)
    if (x or 5) > 0 then
        local y = x
        --        ^ hover: (param) x: number?
    end
end
_consume(testOrCoercionNoNarrow)

-- `>=` with equal values: `(x or 0) >= 0` — fallback 0 >= 0 is true, no narrow
---@param x number?
local function testOrCoercionGeNoNarrow(x)
    if (x or 0) >= 0 then
        local y = x
        --        ^ hover: (param) x: number?
    end
end
_consume(testOrCoercionGeNoNarrow)

-- `>=` where fallback fails: `(x or 0) >= 1` — 0 >= 1 is false → narrow
---@param x number?
local function testOrCoercionGe(x)
    if (x or 0) >= 1 then
        local y = x
        --        ^ hover: (param) x: number
    end
end
_consume(testOrCoercionGe)

-- `~=` comparison: `(x or 0) ~= 0` — 0 ~= 0 is false → narrow
---@param x number?
local function testOrCoercionNe(x)
    if (x or 0) ~= 0 then
        local y = x
        --        ^ hover: (param) x: number
    end
end
_consume(testOrCoercionNe)

-- String: `(x or "") ~= ""` — "" ~= "" is false → narrow
---@param x string?
local function testOrCoercionString(x)
    if (x or "") ~= "" then
        local y = x
        --        ^ hover: (param) x: string
    end
end
_consume(testOrCoercionString)

-- String: `(x or "") == ""` — "" == "" is true → no narrow
---@param x string?
local function testOrCoercionStringNoNarrow(x)
    if (x or "") == "" then
        local y = x
        --        ^ hover: (param) x: string?
    end
end
_consume(testOrCoercionStringNoNarrow)

-- ── Re-narrowing after reassignment in loop with early-exit guard ──────

---@param getLine fun(): string?
---@param matchLine fun(s: string, p: string): string?
local function testRenarrowAfterReassignInLoop(getLine, matchLine)
    while true do
        local stackLine = getLine()
        if not stackLine then
            return nil
        end
        local parsed = matchLine(stackLine, "pattern")
        stackLine = parsed or nil
        if stackLine then
            local _ = matchLine(stackLine, "x")
            --                  ^ hover: (local) stackLine: string
            -- ^ diag: none
        end
    end
end
_consume(testRenarrowAfterReassignInLoop)

-- Override still blocks when no new guard is established after reassignment

---@param getLine fun(): string?
---@param matchLine fun(s: string, p: string): string?
local function testOverrideStillBlocksWithoutNewGuard(getLine, matchLine)
    while true do
        local stackLine = getLine()
        if not stackLine then
            return nil
        end
        -- stackLine is narrowed to string here
        local parsed = matchLine(stackLine, "pattern")
        stackLine = parsed or nil
        -- after reassignment, stackLine is string? and the old narrowing is invalidated
        local _ = matchLine(stackLine, "x")
        --                  ^ hover: (local) stackLine: string?
        -- ^ diag: need-check-nil
    end
end
_consume(testOverrideStillBlocksWithoutNewGuard)

-- ── Ensure-initialized with bracket access (variable key) ────────────
-- `if not tbl[KEY] then tbl[KEY] = val end` guarantees non-nil after

---@type table<string, table>
local dialogs = {}
local DIALOG_KEY = "MY_DIALOG"

local function testBracketEnsureInit()
    if not dialogs[DIALOG_KEY] then
        dialogs[DIALOG_KEY] = { timeout = 0 }
    end
    local info = dialogs[DIALOG_KEY]
    --    ^ hover: (local) info: table
    info.timeout = 5
    -- ^ diag: none
end
_consume(testBracketEnsureInit)

-- Variant: `tbl[KEY] == nil then tbl[KEY] = val end`
local function testBracketEnsureInitEq()
    if dialogs[DIALOG_KEY] == nil then
        dialogs[DIALOG_KEY] = { timeout = 0 }
    end
    local info2 = dialogs[DIALOG_KEY]
    --    ^ hover: (local) info2: table
    info2.timeout = 5
    -- ^ diag: none
end
_consume(testBracketEnsureInitEq)

-- ── Dynamic bracket access narrowing ───────────────────────────────────────
-- If `tbl[key]` is used as a condition, the result is non-nil in the then-branch.

---@type table<string, {anchor: string, x: number}|nil>
local POINTS = {}

local function testBracketNarrow(name)
    if POINTS[name] then
        local pt = POINTS[name]
        --    ^ hover: (local) pt: {
        pt.anchor = "TOP"
        -- ^ diag: none
    end
end
_consume(testBracketNarrow)

-- elseif case
local function testBracketNarrowElseif(name, flag)
    if flag then
        _consume("flag")
    elseif POINTS[name] then
        local pt = POINTS[name]
        --    ^ hover: (local) pt: {
        pt.x = 10
        -- ^ diag: none
    end
end
_consume(testBracketNarrowElseif)

-- early-exit pattern
local function testBracketEarlyExit(name)
    if not POINTS[name] then return end
    local pt = POINTS[name]
    --    ^ hover: (local) pt: {
    pt.anchor = "TOP"
    -- ^ diag: none
end
_consume(testBracketEarlyExit)

-- assert pattern
local function testBracketAssert(name)
    assert(POINTS[name])
    local pt = POINTS[name]
    --    ^ hover: (local) pt: {
    pt.anchor = "TOP"
    -- ^ diag: none
end
_consume(testBracketAssert)

-- Without guard, type includes nil
local function testBracketNoGuard(name)
    local pt = POINTS[name]
    --    ^ hover: (local) pt: {anchor: string, x: number}?
    pt.anchor = "TOP"
    -- ^ diag: need-check-nil
end
_consume(testBracketNoGuard)
