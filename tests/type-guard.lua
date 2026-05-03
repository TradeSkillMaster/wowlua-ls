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
        --    ^ hover: (local) z: number | nil
    end
end
_consume(testFieldElseStripsType)

-- ── Early-exit inverse type guard on field ──────────────────────────────

---@param obj TypeGuardInverseObj
local function testFieldEarlyExitStripsType(obj)
    if type(obj.val) == "string" then return end
    local y = obj.val
    --    ^ hover: (local) y: number | nil
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
