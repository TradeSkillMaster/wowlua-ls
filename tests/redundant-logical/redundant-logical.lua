-- Test: redundant-or and redundant-and diagnostics
---@diagnostic disable: unused-local, unused-function, undefined-global, shadowed-local

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
end
_use(holder)

-- Lateinit field in an `and` chain: `x.field and true or false` — the `or`
-- applies to the `and` result, and since the `and` LHS is a lateinit field
-- that can be nil at runtime, the `and` result can be falsy too.
---@class LateInitRow
---@field _timeLeft number!
---@type LateInitRow
local row

local function hasRawData()
    return row._timeLeft and true or false
end
_use(hasRawData)

-- Dynamic index in an `and` chain: `tbl[k] and v or default` — the bracket
-- lookup may return nil at runtime for missing keys.
---@type table<string, number>
local scores = {}
local function getScore(key)
    return scores[key] and scores[key] > 0 or false
end
_use(getScore)

-- Unannotated param in an `and` chain: `param and true or false` — the param
-- may be nil/missing at runtime despite backward inference typing it non-nil.
---@param n number
local function _takesNum2(n) end

local function checkParam(val)
    _takesNum2(val)
    return val and true or false
end
_use(checkParam)

-- Field on unclassed table in an `and` chain: `obj.field and true or false` —
-- the field is inferred from writes but may be nil at runtime.
local function checkUnclassed()
    ---@type table[]
    local pool = {}
    local obj = table.remove(pool) or {}
    obj.ready = true
    return obj.ready and 1 or 0
end
_use(checkUnclassed)

-- Deeper and-chain: `a and b and c or d` — all operands are checked.
---@class DeepChainObj
---@field enabled number!
---@field level number!
---@type DeepChainObj
local dco

local function deepChainCheck()
    return dco.enabled and dco.level and true or false
end
_use(deepChainCheck)

-- And-chain where the RHS (not LHS) is the suppressible operand:
-- `cond and tbl[k] or default`.
---@type table<string, string>
local labels = {}
local function andRhsSuppressed(key)
    return key ~= "" and labels[key] or "unknown"
end
_use(andRhsSuppressed)

-- ── No diagnostic: dictionary/array bracket lookup ──────────────────────────

-- A `table<K, V>` lookup resolves to the element type `V` (non-nil for the LS),
-- but a missing key returns nil at runtime, so `tbl[k] or default` is a valid
-- fallback and `or` is not redundant.
---@type table<string, number>
local dict = {}
_use((dict["missing"] or 9999) < 5)

-- Array index can be out of bounds → nil at runtime.
---@type number[]
local arr = {}
_use(arr[10] or 0)

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
    end
end

-- ── No diagnostic: field on bare (non-@class) table ─────────────────────────

-- On a table without @class, field existence is inferred from writes, not
-- guaranteed by a schema. `tbl.field = tbl.field or default` is the standard
-- idiom for initializing fields on reused tables (e.g. object pool recycling).
local function poolExample()
    ---@type table[]
    local pool = {}
    local obj = table.remove(pool) or {}
    obj.data = obj.data or {}
    obj.data.count = 1
    _use(obj)
end
_use(poolExample)

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
    -- slotText is string?, so the 'or' is meaningful (no redundant-or)
    local text = data.slotInfo.slotText or "default"
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

-- ── No diagnostic: nil-initialized variable with or-default in loop ─────────

-- A variable initialized to nil that gets assigned via `x = x or default`
-- inside a loop body: on the first iteration x is nil, so the `or` is not
-- redundant even though fixpoint resolution merges the type to table.
local function loopInit()
    local groups = nil
    local items = nil
    for _, part in ipairs({"a","b","c"}) do
        if part == "a" then
            groups = groups or {}
            groups[part] = true
        elseif part == "b" then
            items = items or {}
            groups = groups or {}
            groups[part] = true
            items[part] = "x"
        end
    end
    return items, groups
end
_use(loopInit)

-- Uninitialized local (implicitly nil) — same suppression applies.
local function loopUninit()
    local result
    for _, part in ipairs({"a","b"}) do
        result = result or {}
        result[part] = true
    end
    return result
end
_use(loopUninit)

-- Variable initialized to false (falsy but non-nil) inside a loop.
local function loopFalseInit()
    local found = false
    for _, part in ipairs({"a","b"}) do
        found = found or (part == "a")
    end
    return found
end
_use(loopFalseInit)

-- But a variable initialized to a table is genuinely always truthy — the `or`
-- IS redundant regardless of the loop.
local function loopAlreadyInit()
    local tbl = {}
    for _, part in ipairs({"a","b"}) do
        tbl = tbl or {}
        --      ^ diag: redundant-or
        tbl[part] = true
    end
    return tbl
end
_use(loopAlreadyInit)

-- ── Suppression ─────────────────────────────────────────────────────────────

---@diagnostic disable-next-line: redundant-or
local s1 = 2 or 0

---@diagnostic disable-next-line: redundant-and
local s2 = nil and 1

-- ── No diagnostic: unresolved LHS in `X or nil` ────────────────────────────

-- When the LHS of `or` is unresolved (None), the `or nil` pattern must not
-- collapse the whole expression to `nil`. The variable assigned from such an
-- expression should be unknown, not guaranteed falsy.
local tbl = {}
function tbl.doStuff(key, value, context, path)
    local info = context.tableLookupFunc and context.tableLookupFunc(value) or nil
    if info and context.depth <= context.maxDepth then
        print(info)
    end
    local info2 = context.tableLookupFunc and context.tableLookupFunc(value) or false
    if info2 and context.depth <= context.maxDepth then
        print(info2)
    end
end
_use(tbl)

-- ── No diagnostic: field with unresolvable extra_exprs ────────────────────

-- When a field is initialized to nil in a table constructor but reassigned
-- from a callback parameter whose type can't be resolved, the field type
-- should widen to `any?` (not just `nil`), so `and` is not redundant.
local state = {
    enabled = nil,
}

local function setupCallbacks(registerFn)
    registerFn(function(enabled)
        state.enabled = enabled
    end)
end
_use(setupCallbacks)

local function process()
    if state.enabled and state.enabled ~= "skip" then
--           ^ hover: (field) enabled: any
        _use(state.enabled)
    end
end
_use(process)

-- ── No diagnostic: nil-initialized accumulator in loop ──────────────────────

-- A variable initialized to nil and reassigned inside a loop may hold a
-- non-nil value on subsequent iterations. The `x and f(x)` guard is the
-- standard Lua accumulator idiom — not genuinely redundant.
local function _loopAccum(items)
    local total = nil
    for _, v in ipairs(items) do
        total = total and (total + v) or v
    end
    return total
end
_use(_loopAccum)

-- ── No diagnostic: nil-initialized conditional assignment ───────────────────

-- A variable initialized to nil and conditionally assigned in a loop/branch
-- may not be nil at point of use. The `and` guard is a safe-access pattern.
local function _condAssign(items)
    local first, second = nil, nil
    for _, v in ipairs(items) do
        if not first then
            first = v
        elseif not second then
            second = v
        end
    end
    local result = second and (first + second) or first
    return result
end
_use(_condAssign)

-- ── No diagnostic: or-nil tail with unresolved LHS ─────────────────────────

-- When the LHS of `or` is unresolved and the RHS is nil, the expression
-- should stay unresolved — not collapse to nil. This prevents cascading
-- false `redundant-and` diagnostics downstream.
---@param tree any
---@param node any
local function _orNilTail(tree, node)
    local left, right = tree:GetChildren(node)
    local leftKind = tree:GetData(left, "kind")
    local rightKind = tree:GetData(right, "kind")
    local leftVal = tree:GetData(left, "val")
    local rightVal = tree:GetData(right, "val")
    local picked = (leftKind == "CONST" and leftVal) or (rightKind == "CONST" and rightVal) or nil
    -- `picked` must stay unresolved (?) — NOT collapse to nil from the `or nil` tail.
    if picked and picked == 0 then
    -- ^ hover: (local) picked: ?
        return true
    end
    return false
end
_use(_orNilTail)

-- ── No diagnostic: Lua ternary idiom `x and y or z` ─────────────────────────

-- The `x and y or z` pattern is Lua's standard ternary idiom. Even if the LS
-- resolves `x` as always truthy, the `or z` is the intended else-branch and
-- should not be flagged.
---@class SlicePart
---@field id number

---@class SliceParts
---@field CENTER SlicePart
---@field TOP SlicePart
---@field BOTTOM SlicePart

---@type SliceParts
local parts

---@type string
local relFrame
local mapped = relFrame and parts[relFrame] or nil
_use(mapped)

-- Same pattern with a non-nil fallback instead of nil.
---@type string
local key
local val = key and parts[key] or "default"
_use(val)

-- ── Still diagnostic: literal nil in and ────────────────────────────────────

-- A literal nil (not a variable) used directly in `and` is always redundant.
local j = nil and "hello"
--        ^ diag: redundant-and

-- A variable that is nil and was NEVER reassigned is genuinely always nil.
local neverSet = nil
local k = neverSet and "hello"
--        ^ diag: redundant-and

-- ── No diagnostic: boolean in union makes or-chain not always truthy ────────

-- `val or flag` = number | boolean; boolean can be false, so `or false` is reachable.
---@param flag boolean
---@param val number?
local function boolUnionAndTrueOrFalse(flag, val)
    local result = (val or flag) and true or false
    return result
end
_use(boolUnionAndTrueOrFalse)

-- Same pattern with a longer or-chain (regression for the original report).
---@param flag1 boolean
---@param flag2 boolean
---@param sold boolean
local function boolInUnionOr(flag1, flag2, sold)
    local duration = flag1 and 5 or nil
    local keyword = flag2 and "text" or nil
    local bid = nil
    if flag1 then bid = false elseif flag2 then bid = true end
    local hasFilter = (duration or keyword or bid ~= nil or sold) and true or false
    return hasFilter
end
_use(boolInUnionOr)

-- ── No diagnostic: undeclared (inject) field on @class table ───────────────

-- A field access on a @class table where the field is NOT declared via @field.
-- Such fields are inject-fields set by code, not schema-guaranteed to exist.
-- The `x.field = x.field or default` idiom is standard for initializing them.
---@class PickerFrame
---@field SetColorRGB fun(self: PickerFrame, r: number, g: number, b: number)

---@type PickerFrame
local picker

---@diagnostic disable: inject-field
picker.previousValues = picker.previousValues or {}
picker.previousValues.r = 1
picker.previousValues.g = 2
picker.previousValues.b = 3
_use(picker)

-- Inherited @field from a parent class: the child's own table does not directly
-- declare the field, so initialization is not guaranteed on the child instance.
---@class BaseFrame
---@field data table

---@class ChildFrame : BaseFrame
---@type ChildFrame
local child

---@diagnostic disable-next-line: inject-field
child.data = child.data or {}
_use(child)

-- Declared @field on a @class IS guaranteed to exist, so `or` is still redundant.
---@class ConfigHolder
---@field settings table
---@type ConfigHolder
local cfgHolder
local s = cfgHolder.settings or {}
--                            ^ diag: redundant-or
_use(s)
