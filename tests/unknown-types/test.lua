---@diagnostic disable: undefined-global
-- Test: unknown-* strict-typing diagnostics (default-disabled; enabled via .wowluarc.json)
--
-- These HINTs fire when the resolver couldn't infer a type (resolved_type = None).
-- An explicit `---@type any` / `@param x any` resolves to Some(Any) and is treated as
-- intentional by the user, so the diagnostics skip it.

---@diagnostic disable: unused-function, unused-local, create-global, missing-return, redundant-return, incomplete-signature-doc

local function _consume(...) return ... end

-- ── unknown-param-type ───────────────────────────────────────────────────

-- Fires: unannotated, body doesn't constrain the param.
local function passthrough(mystery)
--                         ^ diag: unknown-param-type
    return mystery
end
_consume(passthrough)

-- No fire: annotated.
---@param x number
local function annotated(x)
--                       ^ diag: none
    return x
end
_consume(annotated)

-- No fire: explicit `any` — user opted in.
---@param x any
local function explicit_any(x)
--                          ^ diag: none
    return x
end
_consume(explicit_any)

-- No fire: backward inference determines the type from body arithmetic.
local function inferred(n)
--                      ^ diag: none
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
--                      ^ diag: none
    return select("#", ...)
end
_consume(varargFn)

-- ── unknown-local-type ───────────────────────────────────────────────────

-- Fires: RHS has no resolvable type.
local u = passthrough(nil)
--    ^ diag: unknown-local-type
_consume(u)

-- No fire: number literal.
local k = 42
--    ^ diag: none
_consume(k)

-- No fire: explicit `---@type any`.
---@type any
local anyLocal = passthrough(nil)
--    ^ diag: none
_consume(anyLocal)

-- No fire: typed annotation.
---@type string
local strLocal = passthrough(nil)
--    ^ diag: none
_consume(strLocal)

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
--  ^ diag: none
end
_consume(returnsTyped)

-- No fire: @return annotation satisfies the expected type even when the body
-- value is unknown (the annotation is the source of truth).
---@return any
local function returnsAny()
    return passthrough(nil)
--  ^ diag: none
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
--  ^ diag: none

-- No fire: @field-declared; annotation is the source of truth.
---@class AnnotatedFieldCls
---@field mystery any
local Ac = {}
Ac.mystery = passthrough(nil)
--  ^ diag: none

_consume(Cls)
_consume(Ac)

-- ── Suppression via @diagnostic disable-next-line ────────────────────────

---@diagnostic disable-next-line: unknown-param-type
local function suppressedParam(mystery)
--                             ^ diag: none
    return mystery
end
_consume(suppressedParam)
