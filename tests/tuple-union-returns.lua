-- Tuple-union `@return` syntax: new multi-value return form.
-- Replaces the old `@return T name` + `@overload return:` pattern with a
-- single `@return (T1 name1, T2 name2) | (U1, U2) desc` line.

local function _consume(...) end

-- ══════════════════════════════════════════════════════════════════════════
-- Single-tuple shorthand: replaces legacy multi-line `@return T name`
-- ══════════════════════════════════════════════════════════════════════════

-- Basic single-tuple with labels
---@return (string firstName, number age)
local function getPerson()
    return "Alice", 42
end
_consume(getPerson)

local p_name, p_age = getPerson()
local _ = p_name
--        ^ hover: (global) p_name: string
local _ = p_age
--        ^ hover: (global) p_age: number

-- Function hover shows labeled returns
local _ = getPerson
--        ^ hover: (global) function getPerson()

-- ══════════════════════════════════════════════════════════════════════════
-- Multi-case tuple-union: correlated returns with narrowing
-- ══════════════════════════════════════════════════════════════════════════

---@return (true ok, number value)
---      | (false, string)
local function tryParse()
    if math.random() > 0.5 then
        return true, 42
    else
        return false, "bad input"
    end
end
_consume(tryParse)

-- Baseline: column-union types
local ok1, v1 = tryParse()
local _ = ok1
--        ^ hover: (global) ok1: boolean
local _ = v1
--        ^ hover: (global) v1: number | string

-- Narrowing: `if ok then` → case 1 only
local ok2, v2 = tryParse()
if ok2 then
    local _ = v2
    --        ^ hover: (global) v2: number
end

-- Narrowing: `if not ok then return end` → case 1 after
local ok3, v3 = tryParse()
if not ok3 then return end
local _ = v3
--        ^ hover: (global) v3: number

-- ══════════════════════════════════════════════════════════════════════════
-- Per-case descriptions (trailing text after `)` with optional `@` prefix)
-- ══════════════════════════════════════════════════════════════════════════

---@return (true ok, number value) success
---      | (false, string) @ failure
local function describedCases()
    return true, 1
end
_consume(describedCases)

-- Hover renders the column-union signature plus a right-aligned `cases:` table
-- with each case's description after `--`
local _ = describedCases
--        ^ hover: (global) function describedCases()\n  -> ok: boolean, value: number | string\n  cases:\n    (true, number)   -- success\n    (false, string)  -- failure

-- ══════════════════════════════════════════════════════════════════════════
-- fun() return type carries tuple-union through
-- ══════════════════════════════════════════════════════════════════════════

---@param cb fun(): (true ok, number v) | (false, string)
local function runCallback(cb)
    local ok, v = cb()
    local _ = ok
    --        ^ hover: (local) ok: boolean
    local _ = v
    --        ^ hover: (local) v: number | string
    if ok then
        local _ = v
        --        ^ hover: (local) v: number
    end
end
_consume(runCallback)

-- ══════════════════════════════════════════════════════════════════════════
-- @alias with tuple-union body
-- ══════════════════════════════════════════════════════════════════════════

---@alias ParseResult (true ok, number value) | (false, string error)
local function _pr() end
_consume(_pr)

---@return ParseResult
local function parseViaAlias()
    return true, 42
end
_consume(parseViaAlias)

-- Alias usage: same narrowing behavior as direct tuple-union
local pa_ok, pa_v = parseViaAlias()
local _ = pa_ok
--        ^ hover: (global) pa_ok: boolean
if pa_ok then
    local _ = pa_v
    --        ^ hover: (global) pa_v: number
end

-- ══════════════════════════════════════════════════════════════════════════
-- Mixing legacy `@return` with tuple-union → malformed-annotation
-- ══════════════════════════════════════════════════════════════════════════

---@return boolean isValid
---@return (string name, number level)
---      | (nil, nil)
local function mixedForm()
--            ^ diag: malformed-annotation
    return true, "hi", 1
end
_consume(mixedForm)

-- ══════════════════════════════════════════════════════════════════════════
-- Arity mismatch: shorter cases are implicitly nil-padded at missing positions
-- (mirrors Lua's runtime semantics — missing return values are nil)
-- ══════════════════════════════════════════════════════════════════════════

-- Single-position `(nil)` case — the `---|` continuation accepts it even
-- though `(T)` would parse as grouping in a non-tuple context.
---@return (number uuid, ...any)
---      | (nil)
local function getFields(n, ...)
--            ^ diag: none
    if n == 0 then return nil end
    return n, ...
end
_consume(getFields)

local gf_uuid, gf_a, gf_b = getFields(1, "x", "y")
local _ = gf_uuid
--        ^ hover: (global) gf_uuid: number | nil
-- Columns past arity 1 pick up the `...any` from case 1 plus implicit nil
-- from case 2, yielding `any | nil`.
local _ = gf_a
--        ^ hover: (global) gf_a: any | nil

-- Narrowing: `if uuid then` → matches case 1 (varargs present)
if gf_uuid then
    local _ = gf_a
    --        ^ hover: (global) gf_a: any
end

-- Early-exit on nil → after the guard, matches case 1
local ef_uuid, ef_a = getFields(1, "f")
if not ef_uuid then return end
local _ = ef_a
--        ^ hover: (global) ef_a: any

-- Shorter-first, longer-second also works; labels come from whichever case
-- has a name at that position (first-case-wins per column).
---@return (nil)
---      | (string name, number level)
local function shortFirst()
--            ^ diag: none
    return nil
end
_consume(shortFirst)

-- ══════════════════════════════════════════════════════════════════════════
-- Legacy LuaLS-style `@return T name` — names picked up for hover labels
-- ══════════════════════════════════════════════════════════════════════════

---@return number numSites
---@return string playerName
---@return boolean isOnline
local function legacyLabels()
    return 1, "Alice", true
end
_consume(legacyLabels)

-- Hover on the function shows the per-position labels
local _ = legacyLabels
--        ^ hover: (global) function legacyLabels()\n  -> numSites: number, playerName: string, isOnline: boolean

-- Legacy trailing `@description` is parsed without breaking the type
---@return number count @number of items
local function legacyDesc() return 1 end
_consume(legacyDesc)

local lc = legacyDesc()
local _ = lc
--        ^ hover: (global) lc: number

-- ══════════════════════════════════════════════════════════════════════════
-- Single-position parens are grouping, not a tuple
-- ══════════════════════════════════════════════════════════════════════════

---@return (string|nil) name
local function groupedSingle()
    return "hi"
end
_consume(groupedSingle)

-- `(string|nil)` is parsed as a grouped single type (not a 1-position tuple),
-- so this is equivalent to the legacy `@return T name` form — the trailing
-- `name` token is picked up as the return label.
local _ = groupedSingle
--        ^ hover: (global) function groupedSingle()\n  -> name: string | nil

local gs = groupedSingle()
local _ = gs
--        ^ hover: (global) gs: string | nil

-- ══════════════════════════════════════════════════════════════════════════
-- Inline tuple union: `(A) | (B)` on a single line
-- ══════════════════════════════════════════════════════════════════════════

-- Same as the `---|` continuation form, but all on one line.
---@param n number
---@return (number uuid, ...any) | (nil)
local function inlineUnion(n, ...)
--            ^ diag: none
    if n < 1 then
        return nil
    end
    if select("#", ...) == 0 then
        return n
    end
    return n, ...
end
_consume(inlineUnion)

local iu = inlineUnion(1)
local _ = iu
--        ^ hover: (global) iu: number | nil

-- Three-case inline union
---@return (true) | (false, string) | (nil)
local function threeCase()
--            ^ diag: none
    return true
end
_consume(threeCase)

-- ══════════════════════════════════════════════════════════════════════════
-- Deferred sibling narrowing: callee is a FieldAccess whose base is a
-- function-call result that build-ir can't resolve to a TableIndex. The
-- sibling narrowing is queued and processed during the fixpoint resolve
-- phase. Refs at later lines were already lowered pointing at the
-- pre-narrow version; the deferred path must redirect them so narrowed
-- types reach downstream diagnostics.
-- ══════════════════════════════════════════════════════════════════════════

---@class DeferQ
local DeferQ = {}

---@param ... string
---@return (number? uuid, ...any) | (nil)
function DeferQ:Get(...) end

---@param ... string
---@return (...any) | ()
function DeferQ:GetNth(...) end

---@return DeferQ
local function getQ() return DeferQ end

local function _deferredFirst()
    local uuid, a, b = getQ():Get("field1", "field2")
    if not uuid then return end
    local _ = a
    --        ^ hover: (local) a: any
    local _ = b
    --        ^ hover: (local) b: any
end
_consume(_deferredFirst)

-- Bare `(...any) | ()` with an `if first then` guard, same deferred path.
local function _deferredBare()
    local a, b, c = getQ():GetNth("x", "y")
    if a then
        local _ = b
        --        ^ hover: (local) b: any
        local _ = c
        --        ^ hover: (local) c: any
    end
end
_consume(_deferredBare)
