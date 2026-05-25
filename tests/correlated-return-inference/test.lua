---@diagnostic disable: undefined-global
-- Test: correlated return-only overload inference
-- With `inference.correlated_return_overloads: true`, functions that have no
-- @return annotations and whose return statements form a clean all-set-or-all-nil
-- pattern get a synthesized return-only overload. Sibling narrowing then propagates
-- through the existing return-only overload pipeline.

local function _consume(...) end

local cond = true

-- ── Basic correlation: 2-tuple, all-set vs all-nil ──────────────────────

local function pair()
    if cond then
        return "alice", 42
    else
        return nil, nil
    end
end

local a1, b1 = pair()
local _ = a1
--        ^ hover: (local) a1: string?
local _ = b1
--        ^ hover: (local) b1: number?

if a1 then
    local _ = a1
    --        ^ hover: (local) a1: string
    local _ = b1
    --        ^ hover: (local) b1: number
end

-- ── 3-tuple correlation ─────────────────────────────────────────────────

local function triple()
    if cond then
        return "name", 7, true
    end
    return nil, nil, nil
end

local n3, l3, ok3 = triple()
if n3 then
    local _ = n3
    --        ^ hover: (local) n3: string
    local _ = l3
    --        ^ hover: (local) l3: number
    local _ = ok3
    --        ^ hover: (local) ok3: true
end

-- ── Skip: function has @return annotations ──────────────────────────────

---@return string?
---@return number?
local function annotated()
    if cond then
        return "x", 1
    end
    return nil, nil
end

local an_a, an_b = annotated()
if an_a then
    -- @return present → no synthesized overload → no sibling narrowing.
    -- Without the overload, b stays optional inside the guard.
    local _ = an_b
    --        ^ hover: (local) an_b: number?
end

-- ── Skip: only one return statement ─────────────────────────────────────

local function single()
    return "always", 1
end

local s_a, s_b = single()
if s_a then
    -- Only one return → no synthesized overload.
    -- s_b's natural type doesn't include nil since it was always set.
    local _ = s_b
    --        ^ hover: (local) s_b: number
end

-- ── Mismatched arity: shorter return padded with nil ────────────────────
-- `return nil` (arity 1) is padded to `(nil, nil)` to match the max arity 2.
-- Synthesized overloads: `(string, number) | (nil, nil)`.
-- Before the guard, mm_b is `number | nil` (union across both overloads).
-- After `if mm_a then`, sibling narrowing strips the `(nil, nil)` overload,
-- leaving only `(string, number)` → mm_b narrows to `number`.

local function mismatched()
    if cond then
        return "x", 1
    end
    return nil
end

local mm_a, mm_b = mismatched()
local _ = mm_b
--        ^ hover: (local) mm_b: number?
if mm_a then
    local _ = mm_b
    --        ^ hover: (local) mm_b: number
end

-- ── Mixed tuple: nil at one position still synthesizes ──────────────────

local function mixed()
    if cond then
        return "x", nil
    end
    return nil, nil
end

local mx_a, mx_b = mixed()
if mx_a then
    -- Two distinct signatures `(string, nil)` and `(nil, nil)` both have nil
    -- at pos 1, so even with the `(string, nil)` overload surviving the
    -- pos-0 strip-falsy narrowing, mx_b stays nil — same observable
    -- behavior as the pre-relaxation "skip mixed tuples" branch.
    local _ = mx_b
    --        ^ hover: (local) mx_b: nil
end

-- ── Skip: every tuple is all-nil ────────────────────────────────────────

local function alwaysNil()
    if cond then
        return nil, nil
    end
    return nil, nil
end

local an2_a, an2_b = alwaysNil()
if an2_a then
    -- Every tuple is all-nil → no useful narrowing → no synthesis.
    local _ = an2_b
    --        ^ hover: (local) an2_b: nil
end

-- ── Skip: arity == 1 (single value) ─────────────────────────────────────

local function single1()
    if cond then
        return "x"
    end
    return nil
end

local s1 = single1()
-- Arity 1 → no synthesis (nothing to correlate). The base return type still
-- unions the if-branch `"x"` and the body-level `nil`, so s1 is `string | nil`.
local _ = s1
--        ^ hover: (local) s1: string?

-- ── Inverse narrowing: `if not x then return end` ───────────────────────

local function pair2()
    if cond then
        return "value", 100
    end
    return nil, nil
end

local function caller()
    local a, b = pair2()
    if not a then return end
    -- After early-exit, a is non-nil → b should also be narrowed.
    local _ = a
    --        ^ hover: (local) a: string
    local _ = b
    --        ^ hover: (local) b: number
end
_consume(caller)

-- ── Mixed-nil shape: (Bool, T, number) | (Bool, nil, nil) ───────────────
-- Real-world shape from a WoW addon's token processor. Under the old
-- "every nil-containing tuple must be all-nil" rule this was rejected
-- entirely; under the relaxed rule each unique tuple (after literal
-- normalization) becomes its own synthesized overload. Three returns below
-- dedupe to two signatures because `(false, nil, nil)` and `(true, nil, nil)`
-- both normalize to `(boolean, nil, nil)`.

local function getVariant() return "variant-x" end

local function getNext()
    if cond then
        return true, getVariant(), 42
    elseif cond then
        return false, nil, nil
    end
    return true, nil, nil
end

local ok2, variant2, idx2 = getNext()
local _ = idx2
--        ^ hover: (local) idx2: number?
-- Narrowing `ok` alone (pos 0) can't discriminate — both overloads have
-- `boolean` at pos 0 — so `idx2` stays optional.
if not ok2 then
    _consume(ok2)
else
    local _ = idx2
    --        ^ hover: (local) idx2: number?
end
-- Narrowing the 2nd return (`variant2`) with a truthy guard filters out the
-- all-nil overload (nil fails strip-falsy at pos 1), leaving only the
-- success overload — so the 3rd return narrows to plain `number`.
if variant2 then
    local _ = variant2
    --        ^ hover: (local) variant2: string
    local _ = idx2
    --        ^ hover: (local) idx2: number
end

-- ── Consistently non-nil position: (T, T, number) | (nil, nil, number) ──
-- Pos 2 is always `number` (never nil) — the old "≥ 1 all-nil tuple"
-- requirement rejected this shape. Under the relaxed rule, pos 2 simply
-- stays `number` in both synthesized overloads, while narrowing pos 0
-- discriminates the success case from the failure case.

local function decodeGroup()
    if cond then
        return "items", "groups", 5
    end
    return nil, nil, 0
end

local function decodeCaller()
    local items, groups, count = decodeGroup()
    if not items then return end
    -- Early-exit strips nil at pos 0; sibling narrowing propagates to pos 1.
    local _ = items
    --        ^ hover: (local) items: string
    local _ = groups
    --        ^ hover: (local) groups: string
    local _ = count
    --        ^ hover: (local) count: number
end
_consume(decodeCaller)

-- ── Always-exiting alt branch → no synthesis ───────────────────────────
-- A single explicit return alongside a guaranteed-exit branch (e.g.
-- `error(...)`) must NOT synthesize. `block_always_exits` keeps
-- `implicit_nil_return = false`, so the effective group count stays 1 and
-- the relaxation doesn't spuriously invent a `(nil, nil)` correlation case.

local function exiting()
    if cond then
        return "x", 1
    end
    error("unreachable")
end

local ex_a, ex_b = exiting()
-- No synthesis → the base type comes from `func.rets` directly, so `ex_a`
-- and `ex_b` are plain string/number with no spurious `| nil` injected.
local _ = ex_a
--        ^ hover: (local) ex_a: string
local _ = ex_b
--        ^ hover: (local) ex_b: number

-- ── Bare return / fall-through counts as implicit all-nil tuple ────────
-- A bare `return` is observationally equivalent to `return nil, nil, ...`
-- from the caller's side, so the synthesizer folds it into the tuple set.
-- Without this, a single explicit multi-return plus a bare early-out would
-- see only one distinct signature and skip synthesis entirely.

local function implicit()
    if cond then
        return "hit", "data", 7
    end
    return  -- bare
end

local function implicitCaller()
    local a, b, c = implicit()
    if not a then return end
    local _ = a
    --        ^ hover: (local) a: string
    local _ = b
    --        ^ hover: (local) b: string
    local _ = c
    --        ^ hover: (local) c: number
end
_consume(implicitCaller)

-- ── Hover rendering: synthesized overloads show `cases (inferred):` ─────
-- Distinguishes synthesized overloads from a hand-written tuple-union
-- `@return (A, B) | (C, D)` (which renders as plain `cases:`).

local _ = decodeGroup
--        ^ hover: (local) function decodeGroup()\n  -> string?, string?, number\n  cases (inferred):\n    (string, string, number)\n    (nil, nil, number)

-- ── Literal-bool + concrete-type preservation ───────────────────────────
-- `return true, ...` / `return false, ...` carry discriminative literal
-- booleans in their cases, and non-literal return expressions resolve to
-- concrete types (field-access chains, typed function returns) instead of
-- collapsing to `any`. Together these unlock the same narrowing path as
-- hand-written `@return true` / `@return false` annotations for
-- synthesized overloads.

---@class Color

---@return Color
---@diagnostic disable-next-line: return-mismatch
local function pick() return nil end

local function count() return 42 end

local function helper(flag)
    if flag == true then
        return true, pick(), count()
    end
    if flag == nil then
        return true, nil, nil
    end
    return false, nil, nil
end

local _ = helper
--        ^ hover: (local) function helper(flag)\n  -> boolean, Color?, number?\n  cases (inferred):\n    (true, Color, number)\n    (boolean, nil, nil)

-- Narrowing the sibling `color4` filters the overload set so both `ok4`
-- (sibling, OverloadNarrow) and `n4` (sibling, OverloadNarrow) see only the
-- `(true, Color, number)` case. Literal bool preservation surfaces as
-- `ok4: true` under sibling narrowing (same mechanism as hand-written
-- `@return true` / `@return false` on union-receiver method calls).
local ok4, color4, n4 = helper(true)
if color4 then
    local _ = ok4
    --        ^ hover: (local) ok4: true
    local _ = n4
    --        ^ hover: (local) n4: number
end

-- The inverse: narrowing `n4` via an early-exit strips the nil-tuple cases
-- (nil fails strip-nil at pos 2), so `ok4` narrows to `true` across both
-- surviving overloads.
local function inverseCaller()
    local ok5, _, n5 = helper(true)
    if n5 == nil then return end
    local _ = ok5
    --        ^ hover: (local) ok5: true
    local _ = n5
    --        ^ hover: (local) n5: number
end
_consume(inverseCaller)

-- ── Dedup merge: two non-literal returns land in the same shape ─────────
-- Two `return true, <call>, <call>` statements both produce `(true, Any, Any)`
-- at build time. Dedup collapses them into ONE synthesized overload whose
-- candidate ExprIds are merged per position; refinement then unions every
-- contributing return's resolved type into each slot:
--   pos 1: `pick()` (Color) + `pick2()` (Fish) → `Color | Fish`
--   pos 2: `count()` (number) + `countStr()` (string) → `number | string`
-- Sibling narrowing on `ok6` then strips the final nil-tuple and leaves
-- the unioned types at the other positions.

---@class Fish
---@return Fish
---@diagnostic disable-next-line: return-mismatch
local function pick2() return nil end

local function countStr() return "two" end

local function multiShape(flag)
    if flag == true then
        return true, pick(), count()
    elseif flag == false then
        return true, pick2(), countStr()
    end
    return nil, nil, nil
end

local ok6, color6, n6 = multiShape(true)
if ok6 then
    -- Only the merged `(true, Color|Fish, number|string)` overload survives
    -- strip-falsy at pos 0. `color6`/`n6` carry the UNIONED types from
    -- every dedup'd source — regression guard for candidate-merge.
    local _ = color6
    --        ^ hover: (local) color6: Color | Fish
    local _ = n6
    --        ^ hover: (local) n6: number | string
end

-- ── Narrowed branch-assigned vars: late-resolving function returns ──────
-- A variable initialized to nil, assigned from function calls in branches,
-- guarded by `if x then`, and returned alongside nil. The function calls'
-- return types resolve AFTER the StripNil version is first evaluated in
-- the fixpoint loop. Without the fix, strip_nil(Nil) would produce an
-- empty union (Union([])) that gets cached and prevents re-resolution.

---@return string
local function makeLabel() return "label" end

---@return string
local function makeMessage(x) return x end

local function filterMsg(msg)
    local result = nil
    if cond then
        result = makeLabel()
    elseif msg then
        result = makeMessage(msg)
    end
    if result then
        return nil, result
    end
end

local _ = filterMsg
--        ^ hover: (local) function filterMsg(msg)\n  -> nil, string?

local function filterCaller()
    local suppressed, replacement = filterMsg("hello")
    if replacement then
        local _ = replacement
        --        ^ hover: (local) replacement: string
    end
end
_consume(filterCaller)

-- Verify no grouped-return-mismatch on filterMsg's return statement
local function filterMsgExplicit(msg)
    local result = nil
    if cond then
        result = makeLabel()
    end
    if result then
        return nil, result
        -- ^ diag: none
    end
end
_consume(filterMsgExplicit)

-- ── StripFalsy path: variable initialized to `false` ────────────────────
-- Same late-resolution scenario as above but with `false` instead of `nil`.
-- `strip_falsy(Boolean(Some(false)))` produces `Union([])` which must be
-- treated as unresolved, same as the StripNil case.

local function filterMsgFalsy(msg)
    local result = false
    if cond then
        result = makeLabel()
    elseif msg then
        result = makeMessage(msg)
    end
    if result then
        return nil, result
    end
end

local _ = filterMsgFalsy
--        ^ hover: (local) function filterMsgFalsy(msg)\n  -> nil, string?

local function falsyCaller()
    local suppressed, replacement = filterMsgFalsy("hello")
    if replacement then
        local _ = replacement
        --        ^ hover: (local) replacement: string
    end
end
_consume(falsyCaller)

-- ── Dedup: identical resolved overloads collapse ────────────────────────
-- When multiple branches return calls that resolve to the same types, the
-- build-time `(Any, Any)` placeholders were distinct (different ExprIds)
-- but after refinement they produce identical `(boolean, string?)` tuples.
-- Post-refinement dedup collapses them, and since < 2 distinct overloads
-- remain, the synthesized overloads are removed entirely — the plain
-- return type is sufficient.

---@return boolean
local function check() return true end

---@return string?
local function reason() return nil end

local function identical(mode)
    if mode == "a" then
        return check(), reason()
    elseif mode == "b" then
        return check(), reason()
    end
    return check(), reason()
end

-- All three branches resolve to (boolean, string?). After dedup, only one
-- distinct overload remains → removed → no `cases (inferred):` in hover.
local _ = identical
--        ^ hover: (local) function identical(mode)\n  -> boolean, string?

-- ── Single-position merge: 3 cases collapse to 2 ────────────────────────
-- Three return statements of arity 2, 2, 3 (arity-padding makes all 3-tuples):
--   return true, nil                    → (true, nil, nil)        [padded]
--   return false, getCode()             → (false, ?, nil)         [padded]
--   return false, getCode(), getSym()   → (false, ?, ?)
-- After padding, cases 2 and 3 differ only at position 2 (nil vs ?-refined).
-- Post-refinement: `(false, string, nil)` + `(false, string, string)` differ
-- at pos 2 only → merge to `(false, string, string | nil)`.
-- Final 2 cases: `(true, nil, nil)` and `(false, string, string | nil)`.

---@return string
local function getCode() return "E001" end

---@return string
local function getSym() return "sym" end

local function triReturn(ok)
    if ok == true then
        return true, nil
    elseif ok == false then
        return false, getCode()
    end
    return false, getCode(), getSym()
end

local _ = triReturn
--        ^ hover: (local) function triReturn(ok)\n  -> boolean, string?, string?\n  cases (inferred):\n    (true, nil, nil)\n    (false, string, string?)

local tr_ok, tr_code, tr_sym = triReturn(true)
if tr_ok then
    local _ = tr_code
    --        ^ hover: (local) tr_code: nil
    local _ = tr_sym
    --        ^ hover: (local) tr_sym: nil
end
if tr_code then
    -- Narrowing tr_code (pos 1 truthy) strips the (true, nil, nil) overload.
    -- Remaining: (false, string, nil | string). Pos 0 narrows to false.
    local _ = tr_ok
    --        ^ hover: (local) tr_ok: false
    local _ = tr_sym
    --        ^ hover: (local) tr_sym: string?
end

-- ── Implicit generics for pass-through params ─────────────────────────
-- When a return path includes a parameter returned directly (pass-through),
-- the synthesizer creates an implicit generic TypeVariable for it. This
-- replaces the old Any placeholder with a proper generic that gets
-- substituted from the caller's argument type at each call site.
--
-- Previously, the single-position dedup merge would absorb Any into
-- the concrete type, collapsing the overloads and silently dropping the
-- uncertainty — producing `number` at the call site instead of `any`.

local function withUnresolved(param)
--             ^ hover: (local) function withUnresolved(param: T1)\n  -> T1 | number, string\n  cases (inferred):\n    (T1, string)\n    (number, string)
    if cond then
        return param, "a"
    end
    return 42, "b"
end
_consume(withUnresolved)

-- ── Implicit generic substitution at call site ──────────────────────
-- When a parameter is only passed through (returned directly), the body
-- provides no constraining operations. The implicit generic T1 gets
-- bound from the caller's argument type, flowing the concrete type
-- into the synthesized return overloads at each call site.

local function maybeTransform(val)
    if cond then
        return val, "ok"
    end
    return nil, "err"
end

-- Caller passes a number → implicit generic T1 binds to number →
-- synthesized overloads become (number, string) | (nil, string) →
-- at call site a: number?, and after nil guard a: number.
local a, b = maybeTransform(42)
--    ^ hover: (local) a: number?
if a then
    local _ = a
    --        ^ hover: (local) a: number
    local _ = b
    --        ^ hover: (local) b: string
end

-- ── Sibling narrowing with implicit generics ──────────────────────
-- When a pass-through param sits at a sibling position (not the guarded
-- position), resolve_overload_narrow must substitute the implicit generic
-- TypeVariable using the call site's generic bindings. Without this,
-- the sibling would show the raw TypeVariable name instead of the
-- concrete type from the caller's argument.

local function tryProcess(data)
    if cond then
        return true, data
    end
    return false, nil
end

local ok, result = tryProcess(42)
--        ^ hover: (local) result: number?
if ok then
    -- Narrowing on `ok` (pos 0, truthy) strips the (false, nil) overload.
    -- The surviving (true, T1) overload has T1 at pos 1, which must be
    -- substituted with the caller's argument type (number) to produce
    -- `result: number` instead of `result: T1`.
    local _ = result
    --        ^ hover: (local) result: number
end

-- ── Multiple pass-through params ──────────────────────────────────
-- Both `a` and `b` are returned directly; each gets its own implicit
-- generic (T1, T2). Deterministic ordering (BTreeMap by SymbolIndex)
-- ensures the generic names are stable across runs.

local function swap(a, b)
    if cond then
        return a, b
    end
    return nil, nil
end

---@diagnostic disable-next-line: redefined-local
local s1, s2 = swap("hello", 42)
--    ^ hover: (local) s1: string?
if s1 then
    local _ = s1
    --        ^ hover: (local) s1: string
    local _ = s2
    --        ^ hover: (local) s2: number
end

-- ── Annotated param pass-through skips implicit generic ──────────
-- When a parameter has a @param annotation, its type is already known.
-- The pass-through detection must NOT create a TypeVariable for it —
-- otherwise the hover shows T1 in the return while the parameter
-- displays its annotated concrete type (e.g. `symbol: string` but
-- `-> ..., T1`).

---@param symbol string
---@param data table
local function handleFiltered(symbol, data)
--                ^ hover: (local) function handleFiltered(symbol: string, data: table)\n  -> boolean, string?, string?\n  cases (inferred):\n    (true, nil, nil)\n    (false, string, string?)
    if cond then
        return true
    elseif cond2 then
        return false, "err"
    end
    return false, "other_err", symbol
end
_consume(handleFiltered)

-- ── Forward-referenced functions with partial branch assignment ──────
-- When a variable (errType, errArg) is assigned in some branches but not
-- others (if/elseif without else), the result is a BranchMerge that
-- unions the assigned-branch types with nil. The synthesized overloads
-- should show the concrete types from the called sub-functions (not
-- `any`) even when those sub-functions are defined after the caller.

---@class ErrKind
---@field FIRST ErrKind
---@field SECOND ErrKind
local ErrKind = {}

local helpers = {}

function helpers.process(str, data)
--               ^ hover: (field) function process(str, data)\n  -> boolean, ErrKind?, string?\n  cases (inferred):\n    (false, ErrKind, string?)\n    (true, nil, nil)
    local isFirst = true
    for _, symbol in ipairs({}) do
        local isValid, errType, errArg = nil, nil, nil
        if isFirst then
            isFirst = false
            isValid, errType = helpers.handleFirst(symbol, data)
        elseif symbol == "" then
            isValid = true
        else
            isValid, errType, errArg = helpers.handleOther(symbol, data)
        end
        if not isValid then
            return false, errType, errArg
        end
    end

    if data == 0 then
        return false, ErrKind.SECOND
    end

    return true
end

function helpers.handleFirst(symbol, data)
    if symbol == "bad" then
        return false, ErrKind.FIRST
    end
    return true
end

function helpers.handleOther(symbol, data)
    if symbol == "bad" then
        return false, ErrKind.FIRST, "detail"
    end
    return true
end

-- Caller sees narrowed types via sibling narrowing:
local function processCaller()
    local ok, errType, errArg = helpers.process("x", {})
    if not ok then
        local _ = errType
        --        ^ hover: (local) errType: ErrKind
        local _ = errArg
        --        ^ hover: (local) errArg: string?
    end
end
_consume(processCaller)

-- ── Tail-call return expansion ────────────────────────────────────────
-- When a function has multiple return paths and one path tail-calls another
-- function, the tail call's multi-return should expand to match sibling
-- return arities. Without expansion, the tail call slot is padded with nil.

local private = {}

function private.clearAndReturn()
    if cond then
        return "result", 42
    end
    return nil, nil
end

local ns = {}

function ns.doWork()
    if not cond then
        return nil, "no context"
    end
    return private.clearAndReturn()
end

local r1, r2 = ns.doWork()
local _ = r1
--        ^ hover: (local) r1: string?
local _ = r2
--        ^ hover: (local) r2: string | number | nil

if r1 then
    local _ = r1
    --        ^ hover: (local) r1: string
    local _ = r2
    --        ^ hover: (local) r2: number?
end

-- ── Single-return tail-call passthrough ───────────────────────────────
-- When a function's ONLY return is a tail call to a multi-return function,
-- all return values should propagate through — not just the first slot.

local tailHelpers = {}

function tailHelpers.getResult()
    return "value", 99
end

local tailApi = {}

function tailApi.fetch()
    return tailHelpers.getResult()
end

local v1, v2 = tailApi.fetch()
local _ = v1
--        ^ hover: (local) v1: string
local _ = v2
--        ^ hover: (local) v2: number

-- ── Reassignment after guard: sibling narrowing before reassignment ───────
-- When a multi-return variable is reassigned AFTER a guard scope, the
-- deferred sibling narrowing should still work for the pre-reassignment scope.
-- Regression: reassigning errKind/errDetail after the first guard would cause
-- sibling_was_reassigned() to see the later version, preventing narrowing.

---@return (true, nil, nil)|(false, string errKind, string? detail)
local function validateReassign(x)
    if x then return true, nil, nil end
    return false, "bad", "detail"
end

local function getReassignObj(text)
    local isValid, errKind, errDetail = validateReassign(text)
    if not isValid then
        return nil, errKind, errDetail
    end
    -- Reassignment after the guard — this must not break sibling narrowing above
    errKind = nil
    errDetail = nil
    return text, nil, nil
end

local ro_a, ro_b, ro_c = getReassignObj("x")
if not ro_a then
    local _ = ro_b
    --        ^ hover: (local) ro_b: string
    local _ = ro_c
    --        ^ hover: (local) ro_c: string?
end

-- ── Namespace method: direct enum returns, mixed arity ────────────────
-- A table-field function with returns at arity 1, 2, and 3. The two false-
-- returning paths differ only in whether they include a detail string.
-- Single-position merge should collapse (false, ErrKind, nil) and
-- (false, ErrKind, string) into (false, ErrKind, string?), yielding
-- exactly two final overloads.

---@class FilterError
---@field INVALID FilterError
---@field MISSING FilterError
---@field OVERFLOW FilterError
local FilterError = {}

local filter = {}

function filter.validate(str, data)
    if str == "bad" then
        return false, FilterError.INVALID, "unexpected char"
    end
    if str == "missing" then
        return false, FilterError.MISSING
    end
    return true
end

local _ = filter.validate
--               ^ hover: (field) function validate(str, data)\n-> boolean, FilterError?, string?\ncases (inferred):\n(false, FilterError, string?)\n(true, nil, nil)

local function validateCaller()
    local ok, errKind, detail = filter.validate("x", {})
    if not ok then
        local _ = errKind
        --        ^ hover: (local) errKind: FilterError
        local _ = detail
        --        ^ hover: (local) detail: string?
    end
end
_consume(validateCaller)

-- ── Namespace method: enum returns from sub-function calls ────────────
-- Same as above but the false-returning paths come from forward-referenced
-- helper functions. The errType/errArg locals are NOT branch-merged (each
-- return statement uses direct sub-function returns or direct values).

local parser = {}

function parser.run(str, data)
    for _, tok in ipairs({}) do
        local ok, err, detail = parser.handleToken(tok, data)
        if not ok then
            return false, err, detail
        end
    end
    if data == 0 then
        return false, FilterError.OVERFLOW
    end
    return true
end

local _ = parser.run
--               ^ hover: (field) function run(str, data)\n-> boolean, FilterError?, string?\ncases (inferred):\n(false, FilterError, string?)\n(true, nil, nil)

function parser.handleToken(tok, data)
    if tok == "bad" then
        return false, FilterError.INVALID, "detail"
    end
    return true
end

local function parserCaller()
    local ok, errKind, detail = parser.run("x", {})
    if not ok then
        local _ = errKind
        --        ^ hover: (local) errKind: FilterError
        local _ = detail
        --        ^ hover: (local) detail: string?
    end
end
_consume(parserCaller)

-- ── Branch-assigned locals with sub-function calls (ParseStr pattern) ─
-- This is the real-world pattern that keeps regressing: inside a loop,
-- locals (isValid, errType, errArg) are assigned from different helper
-- calls depending on branches (if/elseif without else). The locals are
-- then guarded and returned. Because the if/elseif has no else branch,
-- the branch merge produces nil-optional types for errType/errArg. But
-- the correlated return inference should recognize that the return
-- `false, errType, errArg` is only reachable when errType is non-nil.
--
-- Expected cases: (false, FilterError, string?) | (true, nil, nil)
-- Current broken: (false, FilterError?, string?) | (false, FilterError, nil) | (true, nil, nil)

local parseNs = {}

function parseNs.parseStr(str, data)
--               ^ hover: (field) function parseStr(str, data)\n-> boolean, FilterError?, string?\ncases (inferred):\n(false, FilterError, string?)\n(true, nil, nil)
    local isFirst = true
    for _, symbol in ipairs({}) do
        local isValid, errType, errArg = nil, nil, nil
        if isFirst then
            isFirst = false
            isValid, errType = parseNs.handleFirst(symbol, data)
        elseif symbol == "" then
            isValid = true
        else
            isValid, errType, errArg = parseNs.handleOther(symbol, data)
        end
        if not isValid then
            return false, errType, errArg
        end
    end
    if data == 0 then
        return false, FilterError.OVERFLOW
    end
    return true
end

function parseNs.handleFirst(symbol, data)
    if symbol == "bad" then
        return false, FilterError.INVALID
    end
    return true
end

function parseNs.handleOther(symbol, data)
    if symbol == "bad" then
        return false, FilterError.INVALID, "detail"
    end
    return true
end

-- Caller-site narrowing should still work even if the function hover
-- shows the branch-merged types:
local function parseStrCaller()
    local ok, errType, errArg = parseNs.parseStr("x", {})
    if not ok then
        local _ = errType
        --        ^ hover: (local) errType: FilterError
        local _ = errArg
        --        ^ hover: (local) errArg: string?
    end
end
_consume(parseStrCaller)

-- ── Multiple direct-value returns at different arities ────────────────
-- Three false-returns at arity 2, plus one at arity 3, plus a true-return
-- at arity 1. The three arity-2 returns all have FilterError at pos 1,
-- and the arity-3 return adds a string at pos 2. After padding and merge,
-- the final cases should be (false, FilterError, string?) | (true, nil, nil).

local multiRet = {}

function multiRet.check(str, data)
    if str == "a" then
        return false, FilterError.INVALID
    end
    if str == "b" then
        return false, FilterError.MISSING
    end
    if str == "c" then
        return false, FilterError.OVERFLOW, "limit exceeded"
    end
    if data == 0 then
        return false, FilterError.OVERFLOW
    end
    return true
end

local _ = multiRet.check
--                 ^ hover: (field) function check(str, data)\n-> boolean, FilterError?, string?\ncases (inferred):\n(false, FilterError, string?)\n(true, nil, nil)

local function multiRetCaller()
    local ok, errKind, detail = multiRet.check("x", {})
    if not ok then
        local _ = errKind
        --        ^ hover: (local) errKind: FilterError
        local _ = detail
        --        ^ hover: (local) detail: string?
    end
end
_consume(multiRetCaller)

-- ── Simple two-return: (true) | (false, string) ──────────────────────
-- Minimal case: one-value success return, two-value failure return.
-- After padding: (true, nil) | (false, string).

local simple = {}

function simple.tryLoad(path)
    if path == "" then
        return false, "empty path"
    end
    return true
end

local _ = simple.tryLoad
--               ^ hover: (field) function tryLoad(path)\n-> boolean, string?\ncases (inferred):\n(false, string)\n(true, nil)

local function tryLoadCaller()
    local ok, err = simple.tryLoad("/tmp")
    if not ok then
        local _ = err
        --        ^ hover: (local) err: string
    end
end
_consume(tryLoadCaller)

-- ── Forward-reference caller: parseStr defined AFTER caller ──────────
-- Exercises the deferred sibling narrowing path (build_ir can't resolve
-- the callee's overloads because the field doesn't exist yet).

local fwd = {}

local function fwdCaller()
    local ok, errType, errArg = fwd.parseStr("x", {})
    wipe({})  -- intermediate call (like TSM's wipe() between assignment and guard)
    if not ok then
        local _ = errType
        --        ^ hover: (local) errType: FilterError
        local _ = errArg
        --        ^ hover: (local) errArg: string?
    end
end
_consume(fwdCaller)

function fwd.parseStr(str, data)
--               ^ hover: (field) function parseStr(str, data)\n-> boolean, FilterError?, string?\ncases (inferred):\n(false, FilterError, string?)\n(true, nil, nil)
    local isFirst = true
    for _, symbol in ipairs({}) do
        local isValid, errType, errArg = nil, nil, nil
        if isFirst then
            isFirst = false
            isValid, errType = fwd.handleFirst(symbol, data)
        elseif symbol == "" then
            isValid = true
        else
            isValid, errType, errArg = fwd.handleOther(symbol, data)
        end
        if not isValid then
            return false, errType, errArg
        end
    end
    if data == 0 then
        return false, FilterError.OVERFLOW
    end
    return true
end

function fwd.handleFirst(symbol, data)
    if symbol == "bad" then
        return false, FilterError.INVALID
    end
    return true
end

function fwd.handleOther(symbol, data)
    if symbol == "bad" then
        return false, FilterError.INVALID, "detail"
    end
    return true
end
