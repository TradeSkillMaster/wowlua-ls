---@diagnostic disable: undefined-global
-- Tests for type() guard narrowing on symbols and fields

---@diagnostic disable:unused-local

-- ── Basic type guard narrows away nil ──────────────────────────────────

---@param x string|nil
local function testTypeGuardString(x)
    if type(x) == "string" then
        local y = x
        --    ^ hover: (local) y: string
    end
end
_consume(testTypeGuardString)

---@param x number | nil
local function testTypeGuardNumber(x)
    if type(x) == "number" then
        local y = x
        --    ^ hover: (local) y: number
    end
end
_consume(testTypeGuardNumber)

-- ── Assert with type guard ─────────────────────────────────────────────

---@param x table|nil
local function testAssertTypeGuard(x)
    assert(type(x) == "table")
    local y = x
    --    ^ hover: (local) y: table
end
_consume(testAssertTypeGuard)

-- ── type ~= "nil" as nil guard ─────────────────────────────────────────

---@param x number | nil
local function testTypeNotNil(x)
    if type(x) ~= "nil" then
        local y = x
        --    ^ hover: (local) y: number
    end
end
_consume(testTypeNotNil)

---@param x string|nil
local function testTypeNotNilEarlyExit(x)
    if type(x) == "nil" then return end
    local y = x
    --    ^ hover: (local) y: string
end
_consume(testTypeNotNilEarlyExit)

-- ── Type guard on field access ─────────────────────────────────────────

---@class TypeGuardTestObj
---@field val string|nil
---@field tbl table|nil
---@field num number | nil

---@param obj TypeGuardTestObj
local function testFieldTypeGuardIf(obj)
    if type(obj.val) == "string" then
        local y = obj.val
        --    ^ hover: (local) y: string
    end
end
_consume(testFieldTypeGuardIf)

---@param obj TypeGuardTestObj
local function testFieldTypeGuardAssert(obj)
    assert(type(obj.tbl) == "table")
    local y = obj.tbl
    --    ^ hover: (local) y: table
end
_consume(testFieldTypeGuardAssert)

---@param obj TypeGuardTestObj
local function testFieldTypeGuardEarlyExit(obj)
    if type(obj.num) ~= "number" then return end
    local y = obj.num
    --    ^ hover: (local) y: number
end
_consume(testFieldTypeGuardEarlyExit)

-- ── type(field) ~= "nil" as nil guard ──────────────────────────────────

---@param obj TypeGuardTestObj
local function testFieldTypeNotNil(obj)
    if type(obj.val) ~= "nil" then
        local y = obj.val
        --    ^ hover: (local) y: string
    end
end
_consume(testFieldTypeNotNil)

---@param obj TypeGuardTestObj
local function testFieldTypeNilEarlyExit(obj)
    if type(obj.val) == "nil" then return end
    local y = obj.val
    --    ^ hover: (local) y: string
end
_consume(testFieldTypeNilEarlyExit)

-- ── Need-check-nil suppression on field after type guard ────────────────

---@class TypeGuardNilCheck
---@field data table|nil

---@param obj TypeGuardNilCheck
local function testFieldNilCheckSuppressed(obj)
    if type(obj.data) == "table" then
        obj.data.x = 1
    end
end
_consume(testFieldNilCheckSuppressed)

---@param obj TypeGuardNilCheck
local function testFieldNilCheckAssertSuppressed(obj)
    assert(type(obj.data) == "table")
    obj.data.x = 1
end
_consume(testFieldNilCheckAssertSuppressed)

-- ── Inverse type guard on fields (else-branch strips guarded type) ──────

---@class TypeGuardInverseObj
---@field val string|number | nil

---@param obj TypeGuardInverseObj
local function testFieldElseStripsType(obj)
    if type(obj.val) == "string" then
        local y = obj.val
        --    ^ hover: (local) y: string
    else
        local z = obj.val
        --    ^ hover: (local) z: number?
    end
end
_consume(testFieldElseStripsType)

-- ── Early-exit inverse type guard on field ──────────────────────────────

---@param obj TypeGuardInverseObj
local function testFieldEarlyExitStripsType(obj)
    if type(obj.val) == "string" then return end
    local y = obj.val
    --    ^ hover: (local) y: number?
end
_consume(testFieldEarlyExitStripsType)

-- ── Early-exit with reference in then-branch doesn't leak version ────────

---@class TypeGuardEarlyExitClass

---@param action integer|TypeGuardEarlyExitClass
local function testEarlyExitWithRef(action)
    if type(action) == "number" then
        local _ = action
        --    ^ hover: (local) _: number
        return
    end

    local y = action
    --    ^ hover: (local) y: TypeGuardEarlyExitClass
end
_consume(testEarlyExitWithRef)

---@param action2 number|TypeGuardEarlyExitClass
local function testEarlyExitWithMethodCall(action2)
    if type(action2) == "number" then
        return tostring(action2)
    end

    local y = action2
    --    ^ hover: (local) y: TypeGuardEarlyExitClass
end
_consume(testEarlyExitWithMethodCall)

-- ── Cached type guard with "nil" ────────────────────────────────────────

---@param x string|nil
local function testCachedTypeNotNil(x)
    local t = type(x)
    if t ~= "nil" then
        local y = x
        --    ^ hover: (local) y: string
    end
end
_consume(testCachedTypeNotNil)

-- ── String literal equality narrowing (simple symbols) ──────────────────

---@param x "FIRST" | "LAST" | number | nil
local function testStringLiteralElseif(x)
    if x == "LAST" then
        local a = x
        --    ^ hover: (local) a: "LAST"
    elseif x == "FIRST" then
        local b = x
        --    ^ hover: (local) b: "FIRST"
    elseif x then
        local c = x
        --    ^ hover: (local) c: number
    end
end
_consume(testStringLiteralElseif)

---@param x "A" | "B" | "C" | number
local function testStringLiteralNeq(x)
    if x ~= "A" then
        local a = x
        --    ^ hover: (local) a: "B" | "C" | number
    end
end
_consume(testStringLiteralNeq)

-- ── String literal equality narrowing (field chains) ────────────────────

---@class LiteralNarrowObj
---@field mode "idle" | "running" | "done" | nil

---@param obj LiteralNarrowObj
local function testFieldLiteralElseif(obj)
    if obj.mode == "idle" then
        local a = obj.mode
        --    ^ hover: (local) a: "idle"
    elseif obj.mode == "running" then
        local b = obj.mode
        --    ^ hover: (local) b: "running"
    elseif obj.mode then
        local c = obj.mode
        --    ^ hover: (local) c: "done"
    end
end
_consume(testFieldLiteralElseif)

---@class PageQuery
---@field _specifiedPage "FIRST" | "LAST" | number | nil
---@field _page number

---@param self PageQuery
local function testFieldLiteralNoDiag(self)
    if self._specifiedPage == "LAST" then
        self._page = 0
    elseif self._specifiedPage == "FIRST" then
        self._page = 0
    elseif self._specifiedPage then
        self._page = self._specifiedPage
    end
end
_consume(testFieldLiteralNoDiag)

-- ── Boolean type-guard alias: if/else ────────────────────────────────

---@param data string | number
local function testBoolGuardAlias(data)
    local isString = type(data) == "string"
    if isString then
        local x = data
        --    ^ hover: (local) x: string
    else
        local y = data
        --    ^ hover: (local) y: number
    end
end
_consume(testBoolGuardAlias)

-- ── Boolean type-guard alias: negated (~=) ───────────────────────────

---@param data string | number
local function testBoolGuardAliasNeq(data)
    local isNotString = type(data) ~= "string"
    if isNotString then
        local a = data
        --    ^ hover: (local) a: number
    else
        local b = data
        --    ^ hover: (local) b: string
    end
end
_consume(testBoolGuardAliasNeq)

-- ── Boolean type-guard alias: early exit ─────────────────────────────

---@param data string | number
local function testBoolGuardAliasEarlyExit(data)
    local isString = type(data) == "string"
    if not isString then return end
    local z = data
    --    ^ hover: (local) z: string
end
_consume(testBoolGuardAliasEarlyExit)

-- ── Boolean type-guard alias: early exit (truthy) ────────────────────

---@param data string | number
local function testBoolGuardAliasEarlyExitTruthy(data)
    local isString = type(data) == "string"
    if isString then return end
    local w = data
    --    ^ hover: (local) w: number
end
_consume(testBoolGuardAliasEarlyExitTruthy)

-- ── Boolean type-guard alias: assert ─────────────────────────────────

---@param data string | number
local function testBoolGuardAliasAssert(data)
    local isString = type(data) == "string"
    assert(isString)
    local r = data
    --    ^ hover: (local) r: string
end
_consume(testBoolGuardAliasAssert)

-- ── Boolean type-guard alias: and-chain ──────────────────────────────

---@param data string | number
local function testBoolGuardAliasAndChain(data)
    local isString = type(data) == "string"
    local x = isString and data
    --    ^ hover: (local) x: false | string
end
_consume(testBoolGuardAliasAndChain)

-- ── type() guard on field chain in and expression ────────────────────

---@class TypeGuardSettings
---@field value string | number
---@field label string?

---@param settings TypeGuardSettings
local function testTypeGuardFieldInAnd(settings)
    -- type(obj.field) == "number" and obj.field → field narrowed to number
    local x = type(settings.value) == "number" and settings.value
    --    ^ hover: (local) x: false | number
    -- ternary pattern: type guard ensures numeric result
    local y = type(settings.value) == "number" and settings.value or 0
    --    ^ hover: (local) y: number
    -- comparison on ternary result should not warn
    local z = (type(settings.value) == "number" and settings.value or 0) < 100
    --    ^ hover: (local) z: boolean
end
_consume(testTypeGuardFieldInAnd)

-- ── Elseif chain: early-exit type guard must not leak into sibling branches ──

---@param x any
local function testElseifTypeGuardEarlyExit(x)
    if x == nil then
        return false
    elseif type(x) ~= "table" then
        -- then-branch: x is NOT table (CastRemove, but any absorbs the narrowing)
        local a = x
        --    ^ hover: (local) a: any
        return false
    elseif x then
        -- else-branch: x IS table (from inverse of preceding condition)
        local b = x
        --    ^ hover: (local) b: table
    end
end
_consume(testElseifTypeGuardEarlyExit)

-- Same pattern with a union type where narrowing is visibly effective
---@param x string|number|table
local function testElseifTypeGuardEarlyExitUnion(x)
    if x == nil then
        return false
    elseif type(x) ~= "table" then
        -- then-branch: table stripped from union
        local a = x
        --    ^ hover: (local) a: string | number
        return false
    elseif x then
        -- else-branch: only table survives
        local b = x
        --    ^ hover: (local) b: table
    end
end
_consume(testElseifTypeGuardEarlyExitUnion)

-- Symmetric: early-exit with == (strip path)
---@param x string|number|table
local function testElseifTypeGuardEarlyExitStrip(x)
    if x == nil then
        return false
    elseif type(x) == "table" then
        -- then-branch: only table
        local a = x
        --    ^ hover: (local) a: table
        return false
    elseif x then
        -- else-branch: table stripped from union
        local b = x
        --    ^ hover: (local) b: string | number
    end
end
_consume(testElseifTypeGuardEarlyExitStrip)

-- ── Alias narrows through the implicit-else merge ──────────────────────
-- A local initialised from another (`local ng = src`) is a live alias, so a
-- `type(src)` guard that reassigns ng inside the branch also refines ng in the
-- fall-through path: the post-if type drops the guarded member (no false
-- `field-type-mismatch` at `nameGetter = ng`, where ng must be `fun(): string`).

---@class NgExpandData
---@field nameGetter fun(): string

---@param sectionName string | fun(): string
local function testAliasImplicitElseMerge(sectionName)
    local ng = sectionName
    if type(sectionName) == "string" then
        ng = function() return sectionName end
    end
    local out = ng
    --    ^ hover: (local) out: fun(): string
    ---@type NgExpandData
    local data = { nameGetter = ng }
    _consume(data)
end
_consume(testAliasImplicitElseMerge)

-- Three-way union: only the guarded member (string) is stripped from the alias.
---@param sectionName string | number | fun(): string
local function testAliasImplicitElsePartial(sectionName)
    local ng = sectionName
    if type(sectionName) == "string" then
        ng = function() return "x" end
    end
    local out = ng
    --    ^ hover: (local) out: fun(): string | number
end
_consume(testAliasImplicitElsePartial)

-- Explicit (empty) else: the else branch narrows the origin, and ng — still an
-- alias there — is refined to drop the guarded member just like implicit-else.
---@param sectionName string | fun(): string
local function testAliasExplicitElse(sectionName)
    local ng = sectionName
    if type(sectionName) == "string" then
        ng = function() return sectionName end
    else
        _consume(ng)
    end
    local out = ng
    --    ^ hover: (local) out: fun(): string
end
_consume(testAliasExplicitElse)

-- elseif: the alias is filtered to the elseif's guard type in that branch.
---@param sectionName string | number | fun(): string
local function testAliasElseifChain(sectionName)
    local ng = sectionName
    if type(sectionName) == "string" then
        ng = function() return "x" end
    elseif type(sectionName) == "number" then
        _consume(ng)
    end
    local out = ng
    --    ^ hover: (local) out: fun(): string | number
end
_consume(testAliasElseifChain)

-- Soundness: if the origin is reassigned before the guard, the alias is dead —
-- the guard narrows a value ng no longer holds, so the pre-if member survives.
---@param sectionName string | fun(): string
---@param other string | fun(): string
local function testAliasDeadAfterReassign(sectionName, other)
    local ng = sectionName
    sectionName = other
    if type(sectionName) == "string" then
        ng = function() return "x" end
    end
    local out = ng
    --    ^ hover: (local) out: fun(): string | string
end
_consume(testAliasDeadAfterReassign)

-- Determinism: the origin is *also* reassigned inside the branch, so it too gets
-- a merge version. The alias-liveness check must observe the origin's pre-branch
-- version regardless of merge-processing order (snapshotted before the merge
-- loop), so this resolves to `fun(): string` on every run — never the imprecise
-- `fun(): string | string` that a race would sometimes produce.
---@param sectionName string | fun(): string
local function testAliasOriginAlsoMerged(sectionName)
    local ng = sectionName
    if type(sectionName) == "string" then
        ng = function() return "x" end
        sectionName = "literal"
    end
    local out = ng
    --    ^ hover: (local) out: fun(): string
end
_consume(testAliasOriginAlsoMerged)
