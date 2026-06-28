---@diagnostic disable: undefined-global
-- Test: unknown-* strict-typing diagnostics (default-disabled; enabled via .wowluarc.json)
--
-- These HINTs fire when the resolver couldn't infer a type (resolved_type = None).
-- An explicit `---@type any` / `@param x any` resolves to Some(Any) and is treated as
-- intentional by the user, so the diagnostics skip it.

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

-- No fire: explicit `any` — user opted in.
---@param x any
local function explicit_any(x)
    return x
end
_consume(explicit_any)

-- No fire on `_`: it's the conventional throwaway name whose type is
-- intentionally ignored. The sibling `value` param still fires as usual.
local function ignoresFirst(_, value)
--                                 ^ diag: unknown-param-type
    return value
    -- ^ diag: unknown-return-type
end
_consume(ignoresFirst)

-- No fire: backward inference determines the type from body arithmetic.
local function inferred(n)
    return n + 1
end
_consume(inferred)

-- No fire: `self` is skipped regardless of inference.
local obj = {}
function obj:method()
    return self
end
_consume(obj)

-- No fire: `...` vararg tokens aren't params in the Parameter-token sense —
-- they have no name to flag and don't appear in Function.args. An unannotated
-- vararg should not trigger unknown-param-type.
local function varargFn(...)
    return select("#", ...)
    -- ^ diag: unknown-return-type
end
_consume(varargFn)

-- ── unknown-local-type ───────────────────────────────────────────────────

-- Fires: RHS has no resolvable type.
local u = passthrough(nil)
--    ^ diag: unknown-local-type
_consume(u)

-- No fire: `_` is the conventional throwaway name; its type is intentionally
-- ignored even when the RHS is unresolvable.
local _ = passthrough(nil)

-- No fire: number literal.
local k = 42
_consume(k)

-- No fire: explicit `---@type any`.
---@type any
local anyLocal = passthrough(nil)
_consume(anyLocal)

-- No fire: typed annotation.
---@type string
local strLocal = passthrough(nil)
_consume(strLocal)

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
-- A forward declaration (`local x`, no initializer) starts as an untyped nil
-- placeholder, so version 0 carries no resolved type. When a later assignment
-- gives the local a concrete type that the LS resolves at every use site, the
-- declaration must not be flagged: the pass consults the later versions and
-- skips when any of them resolved.

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

-- No fire: assigned inside a loop body (the numeric loop variable). Mirrors the
-- `local maxNeedColumns ... for ... do maxNeedColumns = ... end` addon pattern.
local function loopAssigned()
    local picked
    for i = 1, 3 do
        picked = i
    end
    return picked
end
_consume(loopAssigned)

-- Fires: a forward declaration whose ONLY later assignment is itself
-- unresolvable. No later version resolves to a type, so the local is still
-- genuinely unknown and the declaration stays flagged.
local fwdUnknown
--    ^ diag: unknown-local-type
fwdUnknown = passthrough(nil)
_consume(fwdUnknown)

-- Fires: an INITIALIZED local whose initializer couldn't be typed still fires,
-- even when a later assignment gives it a concrete type. The relaxation is
-- scoped to initializer-less forward declarations (version 0 keeps its
-- `type_source` here), so `local q = <unknown>` is out of scope.
local initUnknown = passthrough(nil)
--    ^ diag: unknown-local-type
initUnknown = 5
_consume(initUnknown)

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

-- No fire: @return annotation satisfies the expected type even when the body
-- value is unknown (the annotation is the source of truth).
---@return any
local function returnsAny()
    return passthrough(nil)
end
_consume(returnsAny)

-- ── unknown-field-type ───────────────────────────────────────────────────

---@class UnknownFieldCls
local Cls = {}

-- Fires: field assigned to unresolvable value, no @field declaration.
Cls.mystery = passthrough(nil)
--  ^ diag: unknown-field-type

-- No fire: field assigned to a typed value.
Cls.known = 42

-- No fire: @field-declared; annotation is the source of truth.
---@class AnnotatedFieldCls
---@field mystery any
local Ac = {}
Ac.mystery = passthrough(nil)

_consume(Cls)
_consume(Ac)

-- ── Suppression via @diagnostic disable-next-line ────────────────────────

---@diagnostic disable-next-line: unknown-param-type
local function suppressedParam(mystery)
    return mystery
    -- ^ diag: unknown-return-type
end
_consume(suppressedParam)
