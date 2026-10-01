---@diagnostic disable: undefined-global
-- Test: unknown-* strict-typing diagnostics (default-disabled; enabled via .wowluarc.json)
--
-- These HINTs mark every value whose type is unknown: no type at all, `any`, or
-- `any?` — what `check`'s type coverage counts as unresolved. That includes explicit
-- `any` annotations (reported as "is annotated"), `_`, `self`, `for` variables,
-- globals, reassignments, and returned values under an `@return` annotation.

---@diagnostic disable: unused-function, unused-local, create-global, missing-return, redundant-return, incomplete-signature-doc

---@diagnostic disable-next-line: unknown-return-type
local function _consume(...) return ... end

-- ── unknown-param-type ───────────────────────────────────────────────────

-- Fires: unannotated, body doesn't constrain the param.
local function passthrough(mystery)
--                         ^ diag: unknown-param-type
    return mystery
    -- ^ diag: unknown-return-type
end
_consume(passthrough)

-- No fire: annotated.
---@param x number
local function annotated(x)
    return x
end
_consume(annotated)

-- Fires: an explicit `any` is as untyped as an inferred one.
---@param x any
local function explicit_any(x)
--                          ^ diag: unknown-param-type ~is annotated `any`
    return x
    -- ^ diag: unknown-return-type ~has type `any`
end
_consume(explicit_any)

-- Fires on `_` like any other name, and on each `_` of a repeated one.
local function ignoresFirst(_, value)
--                          ^ diag: unknown-param-type ~'_'
    return value
    -- ^ diag: unknown-return-type
end
_consume(ignoresFirst)

local function ignoresTwo(_, _, n)
--                        ^ diag: unknown-param-type ~'_'
    return n + 1
end
_consume(ignoresTwo)

-- No fire: backward inference determines the type from body arithmetic.
local function inferred(n)
    return n + 1
end
_consume(inferred)

-- No fire: `self` of a method on a typed table.
local obj = {}
function obj:method()
    return self
end
_consume(obj)

-- Fires: the implicit `self` of a method on a value of unknown type.
local unknownObj = passthrough(nil)
--    ^ diag: unknown-local-type
function unknownObj:method() end
--       ^ diag: unknown-param-type ~implicit parameter 'self'

-- No fire: `...` vararg tokens aren't params in the Parameter-token sense —
-- they have no name to flag and don't appear in Function.args. An unannotated
-- vararg should not trigger unknown-param-type.
local function varargFn(...)
    return select("#", ...)
    -- ^ diag: unknown-return-type
end
_consume(varargFn)

-- Fires on the reassignment of a typed param to an unknown value.
---@param p number
local function reassignsParam(p)
    p = passthrough(nil)
--  ^ diag: unknown-param-type
    return p
    -- ^ diag: unknown-return-type
end
_consume(reassignsParam)

-- ── unknown-local-type ───────────────────────────────────────────────────

-- Fires: RHS has no resolvable type.
local u = passthrough(nil)
--    ^ diag: unknown-local-type
_consume(u)

-- Fires on `_` like any other name.
local _ = passthrough(nil)
--    ^ diag: unknown-local-type

-- No fire: number literal.
local k = 42
_consume(k)

-- Fires: explicit `---@type any`.
---@type any
local anyLocal = passthrough(nil)
--    ^ diag: unknown-local-type ~is annotated `any`
_consume(anyLocal)

-- Fires: a `---@type any` forward declaration, in either position.
---@type any
local fwdAny
--    ^ diag: unknown-local-type ~is annotated `any`
local fwdAnyInline ---@type any
--    ^ diag: unknown-local-type ~is annotated `any`

-- No fire: typed annotation.
---@type string
local strLocal = passthrough(nil)
_consume(strLocal)

-- Fires on a reassignment with the assigned value's type: the annotation says `number`.
---@return any
local function anyValue() end
-- ^ diag: unknown-return-type ~return value 1 is annotated `any`
---@type number
local annotatedNum = 1
annotatedNum = anyValue()
-- ^ diag: unknown-local-type ~has type `any`

-- Fires: an `@as any` cast isn't an annotation of the local.
local cast = 5 --[[@as any]]
--    ^ diag: unknown-local-type ~has type `any`

-- Fires: `any?` narrows to `any`.
---@return any?
local function maybeAny() end
-- ^ diag: unknown-return-type ~is annotated `any?`
local maybe = maybeAny()
--    ^ diag: unknown-local-type ~has type `any?`
_consume(maybe)

-- Fires once, at the declaration: narrowing (`if narrowed then`) isn't a new write.
local narrowed = passthrough(nil)
--    ^ diag: unknown-local-type
if narrowed then _consume(narrowed) end

-- Fires on a reassignment to an unknown value, even after a typed initializer.
local later = 1
later = passthrough(nil)
-- ^ diag: unknown-local-type
_consume(later)

-- No fire: forward declaration (no initializer) with a trailing inline @type.
-- The trailing comment is folded into the local statement node as trivia, so it
-- must still be picked up the same way a preceding ---@type would be.
local fwdLocal ---@type string
_consume(fwdLocal)

-- No fire: same, but the inline @type is on the line above (preceding form).
---@type string
local fwdAbove
_consume(fwdAbove)

-- ── unknown-local-type: forward declaration assigned later ───────────────
-- A forward declaration (`local x`, no initializer) has no value of its own; the
-- assignments that follow are what give the local its type, so those are checked.

-- No fire: assigned a concrete type in BOTH branches of an if/else.
---@param cond boolean
local function branchAssigned(cond)
    local n
    if cond then
        n = 1
    else
        n = 2
    end
    return n
end
_consume(branchAssigned)

-- No fire: assigned in only ONE branch — the merged type is `number?` (the
-- then-branch number unioned with the fall-through nil), still a resolved type.
---@param cond boolean
local function oneBranch(cond)
    local n
    if cond then
        n = 1
    end
    return n
end
_consume(oneBranch)

-- No fire: assigned unconditionally after the declaration.
local function plainForward()
    local m
    m = 5
    return m
end
_consume(plainForward)

-- No fire: assigned inside a loop body (the numeric loop variable).
local function loopAssigned()
    local picked
    for i = 1, 3 do
        picked = i
    end
    return picked
end
_consume(loopAssigned)

-- Fires on the assignment that gives a forward declaration an unknown type.
local fwdUnknown
fwdUnknown = passthrough(nil)
-- ^ diag: unknown-local-type
_consume(fwdUnknown)

-- Fires: an initialized local whose initializer couldn't be typed, even when a
-- later assignment gives it a concrete type.
local initUnknown = passthrough(nil)
--    ^ diag: unknown-local-type
initUnknown = 5
_consume(initUnknown)

-- Fires on a forward declaration that's narrowed but never assigned.
local function narrowedOnly()
    local fwd
    --    ^ diag: unknown-local-type
    if not fwd then return end
    _consume(fwd)
end
_consume(narrowedOnly)

-- ── unknown-local-type: globals ──────────────────────────────────────────

-- Fires on an implicit global, at file level or inside a function.
MysteryGlobal = passthrough(nil)
-- ^ diag: unknown-local-type ~global 'MysteryGlobal'
local function setsGlobal()
    InnerGlobal = passthrough(nil)
    -- ^ diag: unknown-local-type ~global 'InnerGlobal'
end
_consume(setsGlobal)

-- No fire: a typed global.
TypedGlobal = 1

-- ── unknown-local-type: `for` variables ──────────────────────────────────

-- Fires: an iterator of unknown type yields unknown values.
for key, val in passthrough(nil) do
--  ^ diag: unknown-local-type
    _consume(key, val)
end

-- No fire: a typed iterator.
---@return fun(): number, string
local function typedIter() end
for n, s in typedIter() do
    _consume(n, s)
end

-- Each binding of a repeated name is checked at its own position: only the
-- second `_` is `any` here, and both are unknown in the second loop.
---@return fun(): number, any
local function anyIter() end
for _,
    _ in anyIter() do _consume() end
--  ^ diag: unknown-local-type ~has type `any`
for _,
--  ^ diag: unknown-local-type
    _ in passthrough(nil) do _consume() end
--  ^ diag: unknown-local-type

-- ── unknown-return-type ──────────────────────────────────────────────────

-- Fires: the return expression has no resolvable type.
local function returnsUnknown()
    return passthrough(nil)
--  ^ diag: unknown-return-type
end
_consume(returnsUnknown)

-- No fire: returns a typed value.
local function returnsTyped()
    return 1
end
_consume(returnsTyped)

-- Fires under an `@return` annotation too: the annotation types the function,
-- but the value actually returned is unknown.
---@return string
local function annotatedReturn()
    return passthrough(nil)
--  ^ diag: unknown-return-type
end
_consume(annotatedReturn)

-- Fires on an `@return` slot declared with an unknown type, at its `---@return` line.
---@return string name
---@return any? extra
local function declaresAny()
-- ^ diag: unknown-return-type ~return value 'extra' is annotated `any?`
    return "b", nil
end
_consume(declaresAny)

-- No fire: past the end of a complete `@return` list a call's value is nil, so the
-- second slot `widens` fills from `oneValue()` is typed.
---@return string
local function oneValue() return "a" end
---@return string, number?
local function widens() return oneValue() end
_consume(widens)

-- ── unknown-field-type ───────────────────────────────────────────────────

---@class UnknownFieldCls
local Cls = {}

-- Fires: field assigned to unresolvable value, no @field declaration.
Cls.mystery = passthrough(nil)
--  ^ diag: unknown-field-type

-- No fire: field assigned to a typed value.
Cls.known = 42

-- Fires on a `---@field` declared `any`; a typed `---@field` doesn't.
---@class AnnotatedFieldCls
---@field mystery any
---@field typed string
local Ac = {}
-- ^ diag: unknown-field-type ~is annotated `any`
Ac.mystery = passthrough(nil)
Ac.typed = passthrough(nil)

_consume(Cls)
_consume(Ac)

-- ── Suppression via @diagnostic disable-next-line ────────────────────────

---@diagnostic disable-next-line: unknown-param-type
local function suppressedParam(mystery)
    return mystery
    -- ^ diag: unknown-return-type
end
_consume(suppressedParam)

-- ── unknown-return-type: file-level return ───────────────────────────────

-- Fires: the file's return value is annotated `any`.
---@type any
return Cls
--     ^ diag: unknown-return-type ~is annotated `any`
