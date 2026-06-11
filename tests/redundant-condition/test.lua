-- Test: redundant-condition diagnostic (always-truthy/falsy if/while conditions)
---@diagnostic disable: unused-local, unused-function, undefined-global, empty-block, shadowed-local

local function _use(...) end

-- ── Always truthy: if ────────────────────────────────────────────────────────

-- Number literal
local x = 2
if x then end
-- ^ diag: redundant-condition

-- String literal
local s = "hello"
if s then end
-- ^ diag: redundant-condition

-- Table literal
local t = {}
if t then end
-- ^ diag: redundant-condition

-- true literal
if true then end
-- ^ diag: redundant-condition

-- Function value
local function fn() end
if fn then end
-- ^ diag: redundant-condition

-- Typed variable (number)
---@type number
local num
if num then end
-- ^ diag: redundant-condition

-- Typed variable (string)
---@type string
local str
if str then end
-- ^ diag: redundant-condition

-- ── Always falsy: if ─────────────────────────────────────────────────────────

-- nil
if nil then end
-- ^ diag: redundant-condition

-- false
if false then end
-- ^ diag: redundant-condition

-- ── Always truthy: while ─────────────────────────────────────────────────────

-- `while true` is a common idiom — not flagged
while true do break end

-- Number condition
---@type number
local n
while n do break end
--    ^ diag: redundant-condition

-- ── Always truthy: elseif ────────────────────────────────────────────────────

---@type boolean
local flag
if flag then
    _use(1)
elseif true then
    --   ^ diag: redundant-condition
    _use(2)
end

-- ── Always truthy: repeat...until ─────────────────────────────────────────────

-- `repeat...until true` always exits after one iteration — not flagged
repeat break until true

-- ── repeat...until false ────────────────────────────────────────────────────

-- `repeat...until false` is an infinite loop — not flagged
repeat break until false

-- ── No diagnostic: compound conditions ───────────────────────────────────────

-- `x and y` where x is always truthy — the `and` result type depends on y,
-- so the overall condition is not guaranteed truthy/falsy.
---@type number
local a
---@type boolean
local b
if a and b then end

-- ── No diagnostic: nilable types ─────────────────────────────────────────────

---@type number?
local maybeNum
if maybeNum then end

---@type string?
local maybeStr
if maybeStr then end

-- boolean can be false
---@type boolean
local maybeBool
if maybeBool then end

-- Uninitialized local resolves to `?` (unknown), not flagged
local uninit
if uninit then end

-- ── No diagnostic: permissive types ──────────────────────────────────────────

---@param x any
local function withAny(x)
    if x then end
end
_use(withAny)

---@generic T
---@param x T
---@return T
local function withGeneric(x)
    if x then end
    return x
end
_use(withGeneric)

-- ── No diagnostic: nil-initialized local with conditional assignment ────────
-- When a variable is initialized to nil and conditionally assigned from an
-- unresolved call, the branch merge should yield `any` (not `nil`), so
-- `if x then` is NOT flagged as redundant-condition.

local function conditionalAssign(cond)
    local price = nil
    if cond then
        price = unknownFunc() -- intentionally undefined
    end
    if price then
        return price
    end
end
_use(conditionalAssign)

-- ── No diagnostic: lateinit fields ──────────────────────────────────────────
-- Lateinit (`T!`) fields are typed non-nil for the LS but can be nil at
-- runtime until initialized, so `if obj.field then` is not redundant.

---@class LateinitState
---@field handler fun()!
---@field tracker number!

---@param state LateinitState
local function checkLateinit(state)
    if state.handler then
        state.handler()
    end
    if state.tracker then
        _use(state.tracker)
    end
end
_use(checkLateinit)

-- ── No diagnostic: local assigned from lateinit field ────────────────────────
-- When a local variable is assigned from a lateinit field and then checked,
-- the lateinit uncertainty should propagate through the assignment.

---@param state LateinitState
local function checkLateinitViaLocal(state)
    local h = state.handler
    if h then
        h()
    end
end
_use(checkLateinitViaLocal)

-- ── No diagnostic: local from and/or on lateinit field ───────────────────────
-- `X and true or false` is a common Lua idiom for boolean coercion. When X is
-- a lateinit field the LS resolves it as always-truthy, collapsing the result
-- to literal `true`. The local should not be flagged as redundant-condition.

local holder = {
    data = nil, ---@type string!
}

local hadData = holder.data and true or false
if hadData then
-- ^ hover: (local) hadData: boolean
    _use(hadData)
end

-- Lateinit and/or result flowing into a typed context: the widened type
-- (`string?` instead of `string`) must not cause new type-mismatch FPs.
---@param s string
local function acceptString(s) _use(s) end
local val = holder.data and holder.data or "fallback"
--      ^ hover: (local) val: string
acceptString(val)

-- ── No diagnostic: lateinit local (`local x = nil ---@type T!`) ──────────────
-- `T!` on a local variable behaves like lateinit on a field — the static type
-- is non-nil but the runtime value starts as nil and gets initialized lazily.
-- `if not CLASS_LIST then CLASS_LIST = {} end` is the canonical pattern.

local CLASS_LIST = nil ---@type (string[])!
if not CLASS_LIST then
    CLASS_LIST = {}
    tinsert(CLASS_LIST, "all")
end
_use(CLASS_LIST)

local LATE_NUM = nil ---@type number!
if LATE_NUM then _use(LATE_NUM) end

-- Preceding-line `---@type T!` annotation form (different build_ir path
-- from the trailing inline form above).
---@type table!
local PRECEDING_LATE = nil
if not PRECEDING_LATE then PRECEDING_LATE = {} end
_use(PRECEDING_LATE)

-- Transitive: `local y = lateinitLocal` should propagate the uncertainty so
-- `if y then` is not flagged.
local LATE_SRC = nil ---@type string!
local copied = LATE_SRC
if copied then _use(copied) end

-- ── No diagnostic: conditionally-assigned variable resolves to union ─────────
-- After `if cond then x = val end`, the LS merges branches and resolves `x` as
-- `string?` (neither guaranteed-truthy nor guaranteed-falsy), so no diagnostic.

local reassigned = nil
if math.random() > 0.5 then reassigned = "value" end
if reassigned then end

-- ── No diagnostic: variable reassigned inside loop ─────────────────────────

-- "Find exactly one" pattern: variable initialised to nil, checked at the top
-- of the loop body, and reassigned later in the body.
local result = nil
for i = 1, 10 do
    if result then
        result = nil
        break
    end
    result = getItem(i)
end
_use(result)

-- Numeric for with nil-init and reassignment
local found = nil
for k = 1, 5 do
    if found then
        break
    end
    found = lookup(k)
end
_use(found)

-- While loop variant
local hit = nil
while hasNext() do
    if hit then
        break
    end
    hit = fetch()
end
_use(hit)

-- Variable NOT reassigned inside the loop — still diagnose
local neverSet = nil
for j = 1, 3 do
    if neverSet then end
    -- ^ diag: redundant-condition
    _use(j)
end
_use(neverSet)

-- Always-truthy inside a loop, but variable not reassigned — still diagnose
local alwaysNum = 42
for j = 1, 3 do
    if alwaysNum then end
    -- ^ diag: redundant-condition
    _use(j)
end

-- `not` wrapper: variable reassigned inside loop
local notFound = nil
for i = 1, 10 do
    if not notFound then
        notFound = search(i)
    end
end
_use(notFound)

-- `and`/`or` compound condition with reassigned variable
local left = nil
local right = nil
for i = 1, 5 do
    if left and right then
        break
    end
    left = getLeft(i)
    right = getRight(i)
end
_use(left, right)

-- While-loop condition variable reassigned inside body (string param)
---@param name string
local function whileCondReassign(name)
    while name do
        _use(name)
        name = getNext()
    end
end
_use(whileCondReassign)

-- While-loop condition variable reassigned inside body (local)
local function whileCondReassignLocal()
    local item = getFirst()
    while item do
        _use(item)
        item = getNext()
    end
end
_use(whileCondReassignLocal)

-- While-loop condition NOT reassigned — still diagnose
---@param name string
local function whileCondNotReassigned(name)
    while name do break end
    --    ^ diag: redundant-condition
end
_use(whileCondNotReassigned)

-- repeat...until with reassigned variable
local repFound = nil
repeat
    repFound = search()
until repFound
_use(repFound)

-- repeat...until with `not`: variable starts truthy, reassigned inside
local repDone = true
repeat
    repDone = checkDone()
until not repDone
_use(repDone)

-- repeat...until: variable NOT reassigned — still diagnose
local repNever = nil
repeat
    _use(1)
until repNever
--    ^ diag: redundant-condition

-- ── No diagnostic: variable reassigned inside loop, checked AFTER loop ─────
-- The variable's post-loop value depends on whether the loop body's
-- conditional assignment ran, so the condition is not redundant.

local properlySorted = true
for i = 1, 10 do
    if check(i) then
        properlySorted = false
        break
    end
end
if properlySorted then
    _use(properlySorted)
end

-- Same pattern with nil init (always-falsy variant)
local match = nil
for i = 1, 5 do
    match = tryMatch(i)
    if match then break end
end
if match then
    _use(match)
end

-- While variant: variable set inside loop, tested after
local ready = false
while hasMore() do
    ready = checkReady()
end
if ready then
    _use(ready)
end

-- For-in loop variant: boolean flags checked after loop
local allReady = true
for _item in items() do
    allReady = false
end
if allReady then end

local anyBad = false
for _item in items() do
    anyBad = true
end
if anyBad then end

-- Multiple flags from same preceding for-in loop
local allGood = true
local anyError = false
for _item in items() do
    allGood = false
    anyError = true
end
if allGood then end
if anyError then end

-- Compound condition referencing preceding-loop variables
local flagA = true
local flagB = false
for _item in items() do
    flagA = false
    flagB = true
end
if flagA and flagB then end

-- `not` wrapper with preceding-loop variable
local done = false
for _item in items() do
    done = true
end
if not done then end

-- Variable whose defining expression references a loop-reassigned variable
-- (one level of transitive expansion)
local picked = nil
for _item in items() do
    picked = getChild()
end
local derived = someFunc() and picked or nil
if derived then end

-- Variable NOT reassigned inside the loop — still diagnose after the loop
local neverModified = 42
for j = 1, 3 do
    _use(j)
end
if neverModified then end
-- ^ diag: redundant-condition

-- Variable reassigned in a loop AFTER the condition — still diagnose
local beforeLoop = true
if beforeLoop then end
-- ^ diag: redundant-condition
for j = 1, 3 do
    beforeLoop = false
end
_use(beforeLoop)

-- Two-level transitive chain — known limitation (only one level is followed)
local loopFlag = true
for _item in items() do
    loopFlag = false
end
local mid = loopFlag
local chained = mid
if chained then end
-- ^ diag: redundant-condition

-- ── No diagnostic: variable reassigned inside conditional block ─────────────
-- A variable initialised from a call, then conditionally reassigned inside
-- an `if not var then` guard, may still be nil after the block.

local function condReassignInGuard(parentElement, frame, getOwner, search)
    local tableKey = search(parentElement, frame)
    if not tableKey then
        local owner = getOwner()
        local ownerKey = owner and search(parentElement, owner) or nil
        local relKey = ownerKey and search(owner, frame) or nil
        if relKey then
            tableKey = ownerKey.."."..relKey
        end
    end
    if tableKey then
        return tableKey
    end
end
_use(condReassignInGuard)

-- Simpler variant: nil-init, conditional assignment in one branch only
local function condReassignSimple(cond, alt)
    local val = nil
    if cond then
        if alt then
            val = alt
        end
    end
    if val then
        return val
    end
end
_use(condReassignSimple)

-- Truthy-to-truthy reassignment → still diagnose (all versions agree)
local function condReassignTruthyToTruthy(cond)
    local x = "hello"
    if cond then
        x = "world"
    end
    if x then end
    -- ^ diag: redundant-condition
    _use(x)
end
_use(condReassignTruthyToTruthy)

-- Falsy-to-falsy reassignment → still diagnose (all versions agree)
local function condReassignFalsyToFalsy(cond)
    local x = nil
    if cond then
        x = nil
    end
    if x then end
    -- ^ diag: redundant-condition
    _use(x)
end
_use(condReassignFalsyToFalsy)

-- ── Negation: `not <always-truthy>` ─────────────────────────────────────────

-- `not t` where t is always truthy → condition always false (user's case)
local tbl = {}
if not tbl then end
--     ^ diag: redundant-condition

-- `not n` where n is a number → always false
---@type number
local nn
if not nn then end
--     ^ diag: redundant-condition

-- `not s` where s is nilable → NOT flagged
---@type string?
local maybeS
if not maybeS then end

-- ── Equality with nil ────────────────────────────────────────────────────────

-- `x == nil` where x is non-nil → always false
local nonNil = {}
if nonNil == nil then end
--    ^ diag: redundant-condition

-- `x ~= nil` where x is non-nil → always true
local nonNil2 = {}
if nonNil2 ~= nil then end
--    ^ diag: redundant-condition

-- `x == nil` where x is nilable → NOT flagged
---@type string?
local maybeNil
if maybeNil == nil then end

-- ── Disjoint-type equality ───────────────────────────────────────────────────

-- string vs number literal → always false
---@type string
local strv
if strv == 5 then end
--    ^ diag: redundant-condition

-- number vs number literal → NOT flagged (we don't model numeric ranges)
---@type number
local numv
if numv == 5 then end

-- literal-union miss: `v == "c"` where v is "a"|"b" → always false
---@type "a"|"b"
local choice
if choice == "c" then end
--    ^ diag: redundant-condition

-- literal-union hit: `v == "a"` where v is "a"|"b" → NOT flagged
---@type "a"|"b"
local choice2
if choice2 == "a" then end

-- ── Two-literal comparisons ──────────────────────────────────────────────────

if 1 == 2 then end
-- ^ diag: redundant-condition

if "a" == "a" then end
-- ^ diag: redundant-condition

if 3 < 2 then end
-- ^ diag: redundant-condition

if 2 <= 2 then end
-- ^ diag: redundant-condition

-- ── Self-comparison ──────────────────────────────────────────────────────────

-- `x < x` → always false (NaN-safe)
---@type number
local sc
if sc < sc then end
--    ^ diag: redundant-condition

-- `x == x` → NOT flagged (NaN-check idiom)
---@type number
local sc2
if sc2 == sc2 then end

-- `x <= x` → NOT flagged (NaN: `NaN <= NaN` is false)
---@type number
local sc3
if sc3 <= sc3 then end

-- ── Loop-reassignment suppression for comparisons ────────────────────────────

-- `== nil` on a variable reassigned inside the loop → NOT flagged
local seek = nil
for i = 1, 10 do
    if seek == nil then
        seek = getItem(i)
    end
end
_use(seek)

-- ── No diagnostic: open literal-union @param (enum-style annotation) ────────
-- `@param` literal unions are open contracts (caller can pass unlisted values),
-- so the final comparison is not redundant and hover narrows down the chain.

---@param mode "A"|"B"|"C"|"D"
local function handleMode(mode)
    _use(mode)
    --   ^ hover: (param) mode: "A" | "B" | "C" | "D"
    if mode == "A" then
        _use(mode)
        --   ^ hover: (param) mode: "A"
        return 1
    elseif mode == "B" then
    --     ^ hover: (param) mode: "B" | "C" | "D"
        return 2
    elseif mode == "C" then
    --     ^ hover: (param) mode: "C" | "D"
        return 3
    elseif mode == "D" then
    --     ^ hover: (param) mode: "D"
        return 4
    else
        error("invalid")
    end
end
_use(handleMode)

-- Number literal union param: open-contract semantics (no strip narrowing for
-- numbers yet, but no false-positive redundant-condition either).
---@param level 1|2|3
local function handleLevel(level)
    if level == 1 then return "low"
    elseif level == 2 then return "mid"
    elseif level == 3 then return "high"
    else error("bad level") end
end
_use(handleLevel)

-- Boolean literal union param: same open-contract semantics.
---@param flag true|false
local function handleFlag(flag)
    if flag == true then return "on"
    elseif flag == false then return "off"
    else error("bad flag") end
end
_use(handleFlag)

-- Sequential early-return guards narrow the same way as an if/elseif chain.
-- After exhaustive guards the union is empty (all members stripped forward),
-- producing nil — a correct open-contract consequence (no listed member remains).
---@param mode "A"|"B"|"C"
local function handleModeEarly(mode)
    _use(mode)
    --   ^ hover: (param) mode: "A" | "B" | "C"
    if mode == "A" then return end
    if mode == "B" then return end
    if mode == "C" then return end
    -- ^ hover: (param) mode: "C"
    _use(mode)
    --   ^ hover: (param) mode: nil
end
_use(handleModeEarly)

-- ── No diagnostic: enum-typed field compared against enum values ─────────────
-- Enum classes are tables in the type system but numbers/strings at runtime.
-- Comparisons against specific enum member values are valid and should not be
-- flagged as disjoint (regression: Table vs NumberLiteral was seen as disjoint).

-- Number enum: == comparison in if/elseif chain
---@enum ItemKind
local ItemKind = {
    Weapon = 0,
    Armor = 1,
    Consumable = 2,
}

---@class ItemData
---@field kind ItemKind

---@param data ItemData
local function processItem(data)
    if data.kind == ItemKind.Weapon then
        _use("weapon")
    elseif data.kind == ItemKind.Armor then
        _use("armor")
    elseif data.kind == ItemKind.Consumable then
        _use("consumable")
    end
end
_use(processItem)

-- Number enum: ~= comparison
---@param data ItemData
local function filterItem(data)
    if data.kind ~= ItemKind.Weapon then
        _use("not weapon")
    end
end
_use(filterItem)

-- String enum: == comparison in if/elseif chain
---@enum Color
local Color = {
    Red = "red",
    Green = "green",
    Blue = "blue",
}

---@class PaintJob
---@field color Color

---@param job PaintJob
local function applyPaint(job)
    if job.color == Color.Red then
        _use("red")
    elseif job.color == Color.Green then
        _use("green")
    elseif job.color == Color.Blue then
        _use("blue")
    end
end
_use(applyPaint)

-- Nilable enum: ItemKind? compared against enum value
---@param kind ItemKind?
local function maybeProcess(kind)
    if kind == ItemKind.Weapon then
        _use("weapon")
    end
end
_use(maybeProcess)

-- type() guard on enum-typed field: `type(x) == "number"` should not be
-- flagged when x has enum type (enum values are numbers at runtime).
---@param data ItemData
local function checkEnumType(data)
    if type(data.kind) == "number" then
        _use(data.kind)
    end
end
_use(checkEnumType)

-- ── No diagnostic: exit-else defensive guard pattern ────────────────────────
-- When the last elseif in an exhaustive type-check chain is "always true"
-- (all other union members have been eliminated by prior branches) but the
-- chain has an `else` block that always exits (error/return), the condition
-- is intentional and should not be flagged.

-- Closed literal-union: last elseif is "always true" after narrowing, with exit-else error
---@type "A"|"B"|"C"
local exitElseKind
if exitElseKind == "A" then
    _use(1)
elseif exitElseKind == "B" then
    _use(2)
elseif exitElseKind == "C" then
    -- suppressed: "always true" (narrowed to "C") but else exits
    _use(3)
else
    error("unexpected kind")
end

-- With exit-else return
---@type "A"|"B"|"C"
local exitElseKind2
if exitElseKind2 == "A" then
    _use(1)
elseif exitElseKind2 == "B" then
    _use(2)
elseif exitElseKind2 == "C" then
    -- suppressed: else returns
    _use(3)
else
    return
end

-- Still flag when there is NO else block at all
---@type "A"|"B"|"C"
local noElseKind
if noElseKind == "A" then
    _use(1)
elseif noElseKind == "B" then
    _use(2)
elseif noElseKind == "C" then
    -- ^ diag: redundant-condition
    _use(3)
end

-- Still flag when the else block does NOT always exit
---@type "A"|"B"|"C"
local nonExitElseKind
if nonExitElseKind == "A" then
    _use(1)
elseif nonExitElseKind == "B" then
    _use(2)
elseif nonExitElseKind == "C" then
    -- ^ diag: redundant-condition
    _use(3)
else
    _use("fallback")  -- does not exit
end

-- Always-false conditions are still flagged even with an exit-else
---@type string
local sv
if sv == "A" then
    _use(1)
elseif sv == 5 then
    -- ^ diag: redundant-condition
    _use(2)
else
    error("bad")
end

-- ── Loop variable passed as call argument ───────────────────────────────────
-- The loop counter `i` is reassigned each iteration, but it's only used as an
-- argument to a function call whose return type is fixed by annotation. The
-- truthiness of the call's return doesn't depend on `i`'s value, so the
-- diagnostic should still fire.
---@class CallArgInfo
---@field data string

---@param idx number
---@return CallArgInfo
local function fetchInfo(idx) return {data = tostring(idx)} end

local function checkLoopArg()
    for i = 1, 10 do
        if not fetchInfo(i) then
        --     ^ diag: redundant-condition
            return false
        end
    end
    return true
end
_use(checkLoopArg)

-- ── Suppression ──────────────────────────────────────────────────────────────

---@type number
local suppressed
---@diagnostic disable-next-line: redundant-condition
if suppressed then end

-- ── Field assigned from nilable function is NOT always-truthy ───────────────
-- Regression: assigning a nilable function-return to a field used to narrow
-- the field to non-nil, so a subsequent `if not field` was flagged as
-- always-false. The narrowing must be conditional on RHS nilability.

---@class RcFrame
---@field id number

---@class RcState
---@field future RcFrame?

---@return RcFrame?
local function maybeFrame() return nil end

---@param state RcState
local function testNilableFieldAssign(state)
    state.future = maybeFrame()
    if not state.future then
        _use("nil")
    end
end
_use(testNilableFieldAssign)

-- A non-nil function-return assigned to a field DOES narrow, so the
-- subsequent `if not field` IS always-false (current narrowing behavior).

---@return RcFrame
local function alwaysFrame() return { id = 1 } end

---@param state RcState
local function testNonNilFieldAssign(state)
    state.future = alwaysFrame()
    if not state.future then
        -- ^ diag: redundant-condition
        _use("nil")
    end
end
_use(testNonNilFieldAssign)

-- ── No diagnostic: type() guard on conditionally-reassigned variable ─────────
-- `type(x) == "string"` must not be flagged when `x` was initialised as nil
-- and conditionally reassigned to an unknown-type value inside a preceding
-- block. The conditional reassignment makes the static nil type unreliable.

local function typeGuardConditionallyReassignedVariable(cond, getData)
    local status, value = nil, nil
    if cond then
        status, value = getData()
    end
    if status == false then
        if type(value) == "string" then
            _use(value)
        end
    end
end
_use(typeGuardConditionallyReassignedVariable)

-- ── No diagnostic: recursive / mutually-recursive function return ───────────
-- A function that returns boolean (true/false) via both direct returns and
-- recursive or mutually-recursive tail calls should not trigger
-- redundant-condition when its result is used in `if not func() then`.

local priv = {}

function priv.processNode(tree, node)
    if not tree then return true end
    for child in children(node) do
        if not priv.processNode(tree, child) then
            return false
        end
    end
    return priv.processHelper(tree, node)
end

function priv.processHelper(tree, node)
    if not node then return false end
    return priv.processNode(tree, node)
end

-- Non-tail-position recursive call (result stored in local before return)
function priv.processIndirect(tree, node)
    if not tree then return true end
    local result = priv.processIndirect(tree, node)
    return result
end

local function useProcess(tree)
    if not priv.processNode(tree, root()) then
        return false
    end
    if not priv.processIndirect(tree, root()) then
        return false
    end
    return true
end
_use(priv, useProcess)

-- ── No diagnostic: loop-carried variable reassigned inside loop body ────────
-- These document pre-existing correct behavior of `has_uncertain_reassignment`
-- in the redundant-condition pass (not the redundant-and fix), serving as
-- regression tests for that path.

-- While loop condition referencing a variable set to false in the body.
local function _whileLoopCondFlag(queue)
    local active = true
    while active do
        active = false
        for i = 1, #queue do
            if queue[i] > 0 then
                active = true
            end
        end
    end
end
_use(_whileLoopCondFlag)

-- While loop condition with `not x` where x is reassigned to a truthy value.
local function _whileNotLoopCond(items)
    local done = false
    while not done do
        done = true
        for _, v in ipairs(items) do
            if v < 0 then done = false end
        end
    end
end
_use(_whileNotLoopCond)

-- Still diagnostic: variable never reassigned in the loop body — the
-- condition IS genuinely always truthy.
local function _whileConstTruthy()
    local running = true
    ---@diagnostic disable-next-line: empty-block
    while running do
    --    ^ diag: redundant-condition
    end
end
_use(_whileConstTruthy)

-- ── No diagnostic: lazy-init bare self-field (cross-file) ────────────────────
-- Regression test for this pattern lives in
-- tests/redundant-condition-crossfile/ which exercises the workspace-scan
-- → bare_inferred_field_names → from_scan suppression path.
