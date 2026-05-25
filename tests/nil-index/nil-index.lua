---@diagnostic disable: undefined-global
-- Test: nil-index diagnostic
local function _consume(...) end

---@type table<string, number>
local lookup = {}

-- ═════════════════════════════════════════════════════════
-- Read with possibly-nil key
-- ═════════════════════════════════════════════════════════

---@param key string?
local function readNilKey(key)
    local val = lookup[key]
    --                  ^ diag: nil-index
    _consume(val)
end
_consume(readNilKey)

-- ═════════════════════════════════════════════════════════
-- Write with possibly-nil key
-- ═════════════════════════════════════════════════════════

---@param key string?
local function writeNilKey(key)
    lookup[key] = 42
    --     ^ diag: nil-index
end
_consume(writeNilKey)

-- ═════════════════════════════════════════════════════════
-- Nil guard suppresses warning
-- ═════════════════════════════════════════════════════════

---@param key string?
local function guardedRead(key)
    if key then
        local val = lookup[key]
        --                  ^ diag: none
        _consume(val)
    end
end
_consume(guardedRead)

---@param key string?
local function guardedWrite(key)
    if key ~= nil then
        lookup[key] = 42
        --     ^ diag: none
    end
end
_consume(guardedWrite)

-- ═════════════════════════════════════════════════════════
-- Non-nil key: no warning
-- ═════════════════════════════════════════════════════════

---@param key string
local function safeKey(key)
    local val = lookup[key]
    --                  ^ diag: none
    _consume(val)
end
_consume(safeKey)

-- ═════════════════════════════════════════════════════════
-- Literal key: no warning
-- ═════════════════════════════════════════════════════════

local function literalKey()
    local val = lookup["hello"]
    --                  ^ diag: none
    _consume(val)
end
_consume(literalKey)

-- ═════════════════════════════════════════════════════════
-- Key type inference strips nil
-- ═════════════════════════════════════════════════════════

---@type string?
local nilKey = nil

local tbl = {}
---@diagnostic disable-next-line: nil-index
tbl[nilKey] = "value"

local tblVal = tbl["x"]
--    ^ hover: (local) tblVal: string

-- ═════════════════════════════════════════════════════════
-- `and` short-circuit suppresses nil-index on RHS
-- ═════════════════════════════════════════════════════════

---@param key string?
local function andGuard(key)
    local val = key and lookup[key]
    --                          ^ diag: none
    _consume(val)
end
_consume(andGuard)

-- ═════════════════════════════════════════════════════════
-- `and` guard propagates through if-check on derived var
-- ═════════════════════════════════════════════════════════

---@param key string?
local function andGuardDerived(key)
    local data = key and lookup[key]
    if data then
        lookup[key] = 42
        --     ^ diag: none
    end
end
_consume(andGuardDerived)

-- ═════════════════════════════════════════════════════════
-- `and` guard derived: ~= nil comparison
-- ═════════════════════════════════════════════════════════

---@param key string?
local function andGuardNeqNil(key)
    local data = key and lookup[key]
    if data ~= nil then
        lookup[key] = 42
        --     ^ diag: none
    end
end
_consume(andGuardNeqNil)

-- ═════════════════════════════════════════════════════════
-- `and` guard derived: early-exit with == nil
-- ═════════════════════════════════════════════════════════

---@param key string?
local function andGuardEarlyExit(key)
    local data = key and lookup[key]
    if data == nil then
        return
    end
    lookup[key] = 42
    --     ^ diag: none
end
_consume(andGuardEarlyExit)

-- ═════════════════════════════════════════════════════════
-- `and` guard derived: early-exit with `not`
-- ═════════════════════════════════════════════════════════

---@param key string?
local function andGuardNotExit(key)
    local data = key and lookup[key]
    if not data then
        error("missing")
    end
    lookup[key] = 42
    --     ^ diag: none
end
_consume(andGuardNotExit)

-- ═════════════════════════════════════════════════════════
-- `and` guard derived: assert narrowing
-- ═════════════════════════════════════════════════════════

---@param key string?
local function andGuardAssert(key)
    local data = key and lookup[key]
    assert(data)
    lookup[key] = 42
    --     ^ diag: none
end
_consume(andGuardAssert)

-- ═════════════════════════════════════════════════════════
-- `and` guard derived: else branch (not narrowed)
-- ═════════════════════════════════════════════════════════

---@param key string?
local function andGuardElse(key)
    local data = key and lookup[key]
    if data then
        lookup[key] = 42
        --     ^ diag: none
    else
        lookup[key] = 0
        --     ^ diag: nil-index
    end
end
_consume(andGuardElse)

-- ═════════════════════════════════════════════════════════
-- Multi-return sibling with unresolved type: no warning
-- ═════════════════════════════════════════════════════════

local function parseFilter(s)
    if s == "" then
        return false, "EMPTY", "arg"
    end
    return true
end

local function validateFilter()
    local isValid, errType, errArg = parseFilter("test")
    if not isValid then
        local msg = lookup[errType]
        --                ^ diag: none
        _consume(msg, errArg)
    end
end
_consume(validateFilter)
