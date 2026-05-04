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
    --                  ^^^ diag: nil-index
    _consume(val)
end
_consume(readNilKey)

-- ═════════════════════════════════════════════════════════
-- Write with possibly-nil key
-- ═════════════════════════════════════════════════════════

---@param key string?
local function writeNilKey(key)
    lookup[key] = 42
    --     ^^^ diag: nil-index
end
_consume(writeNilKey)

-- ═════════════════════════════════════════════════════════
-- Nil guard suppresses warning
-- ═════════════════════════════════════════════════════════

---@param key string?
local function guardedRead(key)
    if key then
        local val = lookup[key]
        --                  ^^^ diag: none
        _consume(val)
    end
end
_consume(guardedRead)

---@param key string?
local function guardedWrite(key)
    if key ~= nil then
        lookup[key] = 42
        --     ^^^ diag: none
    end
end
_consume(guardedWrite)

-- ═════════════════════════════════════════════════════════
-- Non-nil key: no warning
-- ═════════════════════════════════════════════════════════

---@param key string
local function safeKey(key)
    local val = lookup[key]
    --                  ^^^ diag: none
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
--    ^^^^^^ hover: tblVal: string
