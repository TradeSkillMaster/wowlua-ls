-- Test: backward type inference from body usage

-- ── Signal 1: arithmetic with a typed-number operand → number ──
local function addOne(x)
--                    ^ hover: (param) x: number
    return x + 1
end

local function scale(y)
--                   ^ hover: (param) y: number
    return y * 2
end

local function unaryNeg(z)
--                      ^ hover: (param) z: number
    return -z
end

-- ── Signal 2: concat with a string-compatible operand → string | number ──
local function greet(name)
--                   ^ hover: (param) name: string | number
    return "hi " .. name
end

local function suffix(s)
--                    ^ hover: (param) s: string | number
    return s .. "!"
end

-- ── Signal 3: passed as arg to a typed function → annotated type ──
---@param tag string
local function logTag(tag) end

local function forwardTag(t)
--                        ^ hover: (param) t: string
    logTag(t)
end

---@param count number
local function bump(count) end

local function forwardCount(c)
--                          ^ hover: (param) c: number
    bump(c)
end

-- ── No-override: annotated @param is NOT replaced by body inference ──
---@param n string
local function keepAnnotation(n)
--                            ^ hover: (param) n: string
    return n
end
-- Passing a number where the annotation declares `string` must still flag
-- type-mismatch — proving the annotation, not a body-inferred number type,
-- is authoritative.
local _ka = keepAnnotation(5)
--                         ^ diag: type-mismatch

-- ── Typed-call signal across colon syntax ──
-- `Receiver:colonTyped(x)` — the method's self param consumes the receiver,
-- so args[0] maps to params[1] (self_offset = 1). Inference must honour
-- self_offset and propagate the annotation of the second param.
---@class Receiver
local Receiver = {}
---@param label string
function Receiver:colonTyped(label) end

local function colonForward(lbl)
--                          ^ hover: (param) lbl: string
    Receiver:colonTyped(lbl)
end

-- ── Conflicting signals → no inference (conservative fallback) ──
local function conflicting(a)
--                         ^ hover: (param) a: ?
    local x = a + 1
    local y = a .. "x"
    return x, y
end

-- ── Callers see the inferred type ──
local result = addOne(5)
--    ^ hover: (global) result: number  def: local
