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
        -- ^ diag: none
    end
end
_consume(testFieldNilCheckSuppressed)

---@param obj TypeGuardNilCheck
local function testFieldNilCheckAssertSuppressed(obj)
    assert(type(obj.data) == "table")
    obj.data.x = 1
    -- ^ diag: none
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
        --    ^ hover: (local) a: "FIRST" | "LAST" | number
    elseif x == "FIRST" then
        local b = x
        --    ^ hover: (local) b: "FIRST" | number
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
        -- ^ diag: none
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
