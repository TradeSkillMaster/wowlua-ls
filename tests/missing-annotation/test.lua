-- Test: missing-param-annotation / missing-return-annotation (default-disabled;
-- enabled via .wowluarc.json). These fire on functions reachable beyond their
-- file (globals, table methods/fields) that lack @param / @return — even when
-- the function carries no annotations at all. File-local functions (local
-- function, anonymous literals, forward-declared locals) are never flagged.

---@diagnostic disable: create-global, unused-local, unused-function, redundant-return, missing-return, deprecated, undefined-global

---@class Widget
local Widget = {}
WidgetNS = {}

-- ── Global functions: flagged ──────────────────────────────────────────────

-- Each unannotated param flagged; the value-returning body flags the return.
function GlobalAdd(a, b)
--       ^ diag: missing-return-annotation
--               ^ diag: missing-param-annotation
--                  ^ diag: missing-param-annotation
    return a + b
end

-- No return value → no missing-return-annotation, only the param.
function GlobalLog(message)
--                 ^ diag: missing-param-annotation
    print(message)
end

-- Partially annotated: only the unannotated param is flagged.
---@param x number
function GlobalPartial(x, y)
--                       ^ diag: missing-param-annotation
--       ^ diag: missing-return-annotation
    return x + y
end

-- Fully annotated → nothing flagged.
---@param a number
---@param b number
---@return number
function GlobalAnnotated(a, b)
    return a + b
end

-- `_` is a conventional throwaway and is skipped.
function GlobalIgnoreUnderscore(_, value)
--                                 ^ diag: missing-param-annotation
    print(value)
end

-- Vararg without `@param ...` is flagged.
function GlobalVararg(...)
--                    ^ diag: missing-param-annotation
    print(...)
end

-- ── Table methods / fields: flagged ────────────────────────────────────────

-- Colon method: implicit `self` is skipped, the real param is flagged.
function Widget:SetValue(value)
--                       ^ diag: missing-param-annotation
    self.value = value
end

-- Dot field on a namespace table.
function WidgetNS.Helper(opts)
--                       ^ diag: missing-param-annotation
--       ^ diag: missing-return-annotation
    return opts
end

-- ── File-local functions: never flagged ────────────────────────────────────

-- `local function` is file-private.
local function localHelper(a, b)
    return a + b
end
localHelper(1, 2)

-- Anonymous literal assigned to a local.
local anon = function(x)
    return x
end
anon(1)

-- Anonymous callback argument.
local function register(cb) return cb end
register(function(event, arg)
    return event
end)

-- Forward-declared local reassigned with `function foo()` is still local.
local forwardLocal
function forwardLocal(value)
    return value
end
forwardLocal(1)
