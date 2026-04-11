local function _consume(...) end

-- ── All-or-nothing: return-only overloads ──────────────────────────────

---@return string? name
---@return number? level
---@overload return: string, number
---@overload return: nil
local function allOrNothing()
    if math.random() > 0.5 then
        return "Alice", 42
    end
end
_consume(allOrNothing)

-- Baseline: without narrowing, types are optional
local a1, b1 = allOrNothing()
local _ = a1
--        ^ hover: (global) a1: string | nil
local _ = b1
--        ^ hover: (global) b1: number | nil

-- ── Bare truthiness narrows siblings ────────────────────────────────────

local a2, b2 = allOrNothing()
if a2 then
    local _ = a2
    --        ^ hover: (global) a2: string
    local _ = b2
    --        ^ hover: (global) b2: number
end

-- ── Nil comparison narrows siblings ─────────────────────────────────────

local a3, b3 = allOrNothing()
if a3 ~= nil then
    local _ = a3
    --        ^ hover: (global) a3: string
    local _ = b3
    --        ^ hover: (global) b3: number
end

-- ── Inverse nil comparison (else branch) narrows siblings ───────────────

local a4, b4 = allOrNothing()
if a4 == nil then
    _consume("nil path")
else
    local _ = a4
    --        ^ hover: (global) a4: string
    local _ = b4
    --        ^ hover: (global) b4: number
end

-- ── Early exit with `if not x then error() end` ────────────────────────

local a5, b5 = allOrNothing()
if not a5 then
    error("expected value")
end
local _ = a5
--        ^ hover: (global) a5: string
local _ = b5
--        ^ hover: (global) b5: number

-- ── Early exit with `if x == nil then return end` ───────────────────────

local a6, b6 = allOrNothing()
if a6 == nil then
    return
end
local _ = a6
--        ^ hover: (global) a6: string
local _ = b6
--        ^ hover: (global) b6: number

-- ── Assert narrows siblings ─────────────────────────────────────────────

local a7, b7 = allOrNothing()
assert(a7)
local _ = a7
--        ^ hover: (global) a7: string
local _ = b7
--        ^ hover: (global) b7: number

-- ── Nested scope inherits sibling narrowing ─────────────────────────────

local a8, b8 = allOrNothing()
if a8 then
    if true then
        local _ = b8
        --        ^ hover: (global) b8: number
    end
end

-- ── Three return values ─────────────────────────────────────────────────

---@return string? name
---@return number? level
---@return boolean? active
---@overload return: string, number, boolean
---@overload return: nil
local function threeReturns()
    if math.random() > 0.5 then
        return "Bob", 10, true
    end
end
_consume(threeReturns)

local t1, t2, t3 = threeReturns()
if t1 then
    local _ = t2
    --        ^ hover: (global) t2: number
    local _ = t3
    --        ^ hover: (global) t3: boolean
end

-- ── No return-only overload: siblings NOT narrowed ──────────────────────

---@return string? name
---@return number? level
local function noOverload()
    if math.random() > 0.5 then
        return "Carol", 99
    end
end
_consume(noOverload)

local n1, n2 = noOverload()
if n1 then
    local _ = n2
    --        ^ hover: (global) n2: number | nil
end

-- ── Check second return narrows first sibling ───────────────────────────

local c1, c2 = allOrNothing()
if c2 then
    local _ = c1
    --        ^ hover: (global) c1: string
end

-- ── Table.Method() return-only overload narrows siblings ─────────────────

local Scanner = {}

---@return number? speciesId
---@return number? level
---@return number? quality
---@overload return: number, number, number
---@overload return: nil, nil, nil
function Scanner.GetInfo()
    if math.random() > 0.5 then
        return 1, 2, 3
    end
    return nil, nil, nil
end
_consume(Scanner)

local s1, s2, s3 = Scanner.GetInfo()
if s1 then
    local _ = s1
    --        ^ hover: (global) s1: number
    local _ = s2
    --        ^ hover: (global) s2: number
    local _ = s3
    --        ^ hover: (global) s3: number
end

-- ── Compound guard (x and x > 0) still narrows siblings ─────────────────

local g1, g2, g3 = Scanner.GetInfo()
if g1 and g1 > 0 then
    local _ = g2
    --        ^ hover: (global) g2: number
    local _ = g3
    --        ^ hover: (global) g3: number
end

-- ══════════════════════════════════════════════════════════════════════════
-- Callee-side enforcement: grouped-return-mismatch diagnostic
-- ══════════════════════════════════════════════════════════════════════════

-- ── Valid: returns all values ────────────────────────────────────────────

---@return string? name
---@return number? level
---@overload return: string, number
---@overload return: nil
local function validAll()
    return "Alice", 42
    -- ^ diag: none
end
_consume(validAll)

-- ── Valid: bare return (nothing) ────────────────────────────────────────

---@return string? name
---@return number? level
---@overload return: string, number
---@overload return: nil
local function validNone()
    return
    -- ^ diag: none
end
_consume(validNone)

-- ── Invalid: partial return (some nil, some not) ────────────────────────

---@return string? name
---@return number? level
---@overload return: string, number
---@overload return: nil
local function invalidPartial()
    return "Alice", nil
    --     ^ diag: grouped-return-mismatch
end
_consume(invalidPartial)

-- ── Invalid: reversed partial ───────────────────────────────────────────

---@return string? name
---@return number? level
---@overload return: string, number
---@overload return: nil
local function invalidReversed()
    return nil, 42
    --     ^ diag: grouped-return-mismatch
end
_consume(invalidReversed)

-- ── Valid: return nil, nil (matches nil overload) ───────────────────────

---@return string? name
---@return number? level
---@overload return: string, number
---@overload return: nil
local function validAllNil()
    return nil, nil
    -- ^ diag: none
end
_consume(validAllNil)

-- ══════════════════════════════════════════════════════════════════════════
-- Annotation validation diagnostics
-- ══════════════════════════════════════════════════════════════════════════

-- ── Invalid: @overload with garbage content ───────────────────────────────

---@overload gibberish
-- ^ diag: malformed-annotation
local function badOverload() end
_consume(badOverload)

-- ── Invalid: @overload return: without any @return ────────────────────────

---@overload return: string, number
---@overload return: nil
local function noReturnAnnotations()
--            ^ diag: malformed-annotation
    return "hi", 1
end
_consume(noReturnAnnotations)

-- ── Invalid: @overload return: count mismatch with @return count ──────────

---@return string? name
---@return number? level
---@overload return: string, number, boolean
---@overload return: nil
local function countMismatch()
--            ^ diag: malformed-annotation
    return "hi", 1
    --     ^ diag: grouped-return-mismatch
end
_consume(countMismatch)

-- ── Valid: @overload return: count matches @return count ──────────────────

---@return string? name
---@return number? level
---@overload return: string, number
---@overload return: nil
local function countMatch()
    return "hi", 1
    -- ^ diag: none
end
_consume(countMatch)

-- ── Valid: delegating to callee with return-only overloads ─────────────

---@return number uuid, ...any
---@overload return:
local function innerFunc(n, ...)
    if n then
        return n, ...
    end
end
_consume(innerFunc)

---@return number uuid, ...any
---@overload return:
local function delegatingFunc(...)
    return innerFunc(1, ...)
    -- ^ diag: none
end
_consume(delegatingFunc)
