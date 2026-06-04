-- Test: redundant-or and redundant-and diagnostics
---@diagnostic disable: unused-local, unused-function, undefined-global

local function _use(...) end

-- ── redundant-or: LHS always truthy ───────────────────────────────────────────

-- Number is always truthy
local a = 2 or 0
--        ^ diag: redundant-or

-- String is always truthy
local b = "hello" or "default"
--        ^ diag: redundant-or

-- Table is always truthy
local c = {} or {}
--        ^ diag: redundant-or

-- true is always truthy
local d = true or false
--        ^ diag: redundant-or

-- Function is always truthy
local e = _use or print
--        ^ diag: redundant-or

-- Variable with known truthy type
---@type number
local num
local f = num or 0
--        ^ diag: redundant-or

-- String variable
---@type string
local str
local g = str or ""
--        ^ diag: redundant-or

-- ── redundant-and: LHS always falsy ──────────────────────────────────────────

-- nil is always falsy
local h = nil and 1
--        ^ diag: redundant-and

-- false is always falsy
local i = false and "hello"
--        ^ diag: redundant-and

-- ── No diagnostic: LHS can be falsy (or) ────────────────────────────────────

-- nil|number — not guaranteed truthy
---@type number?
local maybeNum
_use(maybeNum or 0)

-- boolean — could be false
---@type boolean
local maybeBool
_use(maybeBool or "default")

-- Uninitialized local is nil
local uninit
_use(uninit or "fallback")

-- ── No diagnostic: LHS can be truthy (and) ──────────────────────────────────

-- Truthy LHS with and — common idiom, not flagged
_use(2 and "yes")

-- boolean LHS with and — could be true
_use(maybeBool and "yes")

-- ── No diagnostic: permissive types ─────────────────────────────────────────

---@param x any
local function withAny(x)
    _use(x or 0)
    _use(x and "yes")
end
_use(withAny)

---@generic T
---@param x T
---@return T
local function withGeneric(x)
    _use(x or 0)
    return x
end
_use(withGeneric)

-- ── No diagnostic: lateinit (T!) field access ───────────────────────────────

-- A lateinit field is typed non-nil for the LS but can be nil at runtime until
-- first initialized via the `x = x or default` idiom, so `or` is not redundant.
---@class LateInitHolder
---@field cached number!
local holder = {}

function holder.Init()
    holder.cached = holder.cached or 0
    --                            ^ diag: none
end
_use(holder)

-- ── No diagnostic: dictionary/array bracket lookup ──────────────────────────

-- A `table<K, V>` lookup resolves to the element type `V` (non-nil for the LS),
-- but a missing key returns nil at runtime, so `tbl[k] or default` is a valid
-- fallback and `or` is not redundant.
---@type table<string, number>
local dict = {}
_use((dict["missing"] or 9999) < 5)
--                   ^ diag: none

-- Array index can be out of bounds → nil at runtime.
---@type number[]
local arr = {}
_use(arr[10] or 0)
--         ^ diag: none

-- Literal key matching a declared @field resolves to the field type (guaranteed
-- to exist), so `or` IS redundant here — not suppressed.
---@class DictWithField : table<string, number>
---@field name string
---@type DictWithField
local cfg
_use(cfg["name"] or "default")
--                ^ diag: redundant-or

-- Bracket index through a field chain inside a nil-guarded scope (StripNil
-- wrapping): the BracketIndex is wrapped in StripNil by narrowing, but the
-- `or` fallback is still valid for missing dictionary keys.
local private = {
    storage = {
        quantities = nil, ---@type table<string,number>
    },
}
---@type table<string, boolean>
local items = {}
if private.storage.quantities then
    for key in pairs(items) do
        _use(private.storage.quantities[key] or 0)
        --                                    ^ diag: none
    end
end

<<<<<<< HEAD
-- ── No diagnostic: sub-field of narrowed parent ─────────────────────────────

-- When `obj.parent` is narrowed via assert(), accessing `obj.parent.field`
-- on a sub-field typed `string?` must NOT have its nil stripped. The `or`
-- provides a fallback for the nilable sub-field and is not redundant.
---@class SlotInfoData
---@field slotText string?
---@field slotId number

---@class ReagentData
---@field slotInfo SlotInfoData?
---@field required boolean

---@param data ReagentData
local function processReagent(data)
    assert(data.slotInfo)
    -- diag: none needed here because redundant-or is HINT-level (harness exempts HINTs)
    local text = data.slotInfo.slotText or "default"
    --                                  ^ diag: none
    _use(text, data.slotInfo.slotId)
end
_use(processReagent)

-- ── No diagnostic: deferred sibling narrowing must skip guard symbol ─────────
-- When a multi-return function is a forward-declared field access, sibling
-- narrowing is deferred to the fixpoint. The deferred path must skip the
-- guard symbol (the symbol whose truthiness triggers the `and`) to avoid
-- rewriting its own LHS reference to the narrowed type.

local helper = {}

---@param side string
---@return number
function helper.getVal(side)
    return 1
end

-- Forward reference: helper.split is used before its definition, so sibling
-- narrowing is deferred to the resolve phase.
local function doOffsets()
    local a, b = helper.split()
    local x = a and ((a == "X" and 1 or -1) * helper.getVal(a)) or 0
    --                                                             ^ diag: none
    local y = b and ((b == "Y" and 1 or -1) * helper.getVal(b)) or 0
    --                                                             ^ diag: none
    return x, y
end
_use(doOffsets)

-- Defined after usage — triggers OverloadCheck::Deferred for sibling narrowing.
function helper.split()
    if true then
        return "X", "Y"
    elseif true then
        return nil, "Y"
    elseif true then
        return "X", nil
    else
        return nil, nil
    end
end

-- ── No diagnostic: unannotated param with `or` default ──────────────────────

-- Backward inference adds nil to the inferred type when the param is used as
-- the LHS of `or`, so `redundant-or` does not fire.
---@param n number
local function _takesNum(n) end

local function orDefault(val)
    val = val or 42
    _takesNum(val)
end
_use(orDefault)

-- But if a parameter IS explicitly annotated as a truthy type, the `or` IS
-- redundant since the caller is declaring the type contract.
---@param val number
---@return number
local function withAnnotated(val)
    val = val or 42
    --        ^ diag: redundant-or
    return val
end
_use(withAnnotated(10))

-- ── Suppression ─────────────────────────────────────────────────────────────

---@diagnostic disable-next-line: redundant-or
local s1 = 2 or 0

---@diagnostic disable-next-line: redundant-and
local s2 = nil and 1
