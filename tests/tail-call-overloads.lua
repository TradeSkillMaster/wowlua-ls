---@diagnostic disable: shadowed-local, unused-function, unused-local

-- ============================================================================
-- Direct tail-call forwarding: return Func1(...)
-- ============================================================================

---@return (true, number)|(false, string)
local function Base(...)
-- ^ diag: missing-return
end

local function TailWrapper(...)
    return Base(...)
end

local ok, val = TailWrapper()
if ok then
    local n = val
    --        ^^^ hover: (local) val: number
else
    local s = val
    --        ^^^ hover: (local) val: string
end

-- ============================================================================
-- Chain: A -> B -> C
-- ============================================================================

local function Chain(...)
    return TailWrapper(...)
end

local a, b = Chain()
if not a then
    local err = b
    --          ^ hover: (local) b: string
end

-- ============================================================================
-- Destructure + re-return (pass-through pattern)
-- ============================================================================

---@return (true, number)|(false, string)
local function Source(...)
-- ^ diag: missing-return
end

local function Passthrough(...)
    local ok2, val2 = Source(...)
    return ok2, val2
end

local x, y = Passthrough()
if x then
    local n2 = y
    --         ^ hover: (local) y: number
else
    local s2 = y
    --         ^ hover: (local) y: string
end

-- ============================================================================
-- Destructure + re-return with @as cast
-- ============================================================================

---@class SubResult
---@field extra boolean

---@return (SubResult, string)|(nil, nil)
local function GetResult(...)
end

---@class SpecificResult: SubResult
---@field name string

local function GetResultWrapped(...)
    local result, name = GetResult(...)
    return result --[[@as SpecificResult?]], name
end

local r, n = GetResultWrapped()
if r then
    local name = n
    --           ^ hover: (local) n: string
else
    local nothing = n
    --              ^ hover: (local) n: nil
end

-- ============================================================================
-- Forward-declared function + multi-return reassignment
-- ============================================================================

local priv = {}

local function Consumer(x)
    local minVal, errType, errArg = nil, nil, nil
    minVal, errType, errArg = priv.GetValues(x)
    if not minVal then
        local e = errType
        --        ^^^^^^^ hover: (local) errType: string
        local d = errArg
        --        ^^^^^^ hover: (local) errArg: string
    end
end

function priv.GetValues(x)
    if x > 0 then
        return x, nil, nil
    end
    return nil, "ERR", "detail"
end
