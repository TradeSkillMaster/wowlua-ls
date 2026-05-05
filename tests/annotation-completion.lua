-- Annotation completion tests

-- ── Tag completions ──────────────────────────────────────────────────────────

-- All tags after ---@ (no context: shows everything)
---@
--  ^ comp: param, return, type, class, field, alias, enum, event, overload, defclass, generic, cast, as, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, accessor, meta, diagnostic, type-narrows, flavor-narrows, correlated, see

-- Partial prefix: "re" → return
---@re
--    ^ comp: return

-- Partial prefix: "se" → see
---@se
--    ^ comp: see

-- Partial prefix: "p" → param, private, protected
---@p
--   ^ comp: param, private, protected

-- Partial prefix: "cl" → class
---@cl
--    ^ comp: class

-- Partial prefix: "co" → constructor, correlated
---@co
--    ^ comp: constructor, correlated

-- Partial prefix: "fl" → flavor-narrows
---@fl
--    ^ comp: flavor-narrows

-- ── Context-aware filtering ────────────────────────────────────────────────

-- Function context: after @param, only function-applicable tags appear
---@param x number
---@
--  ^ comp: param, return, overload, defclass, generic, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, diagnostic, type-narrows, flavor-narrows, see
function ctxFuncTest(x) end

-- Class context: after @class, only class-applicable tags appear
---@class CtxClassTest
---@
--  ^ comp: field, accessor, correlated, overload, constructor, deprecated, nodiscard, private, protected, diagnostic, see

-- Function context inferred from function below (no prior tags)
---@
--  ^ comp: param, return, overload, defclass, generic, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, diagnostic, type-narrows, flavor-narrows, see
function ctxInferredFunc() end

-- ── Param name completions ───────────────────────────────────────────────────

-- Partial param name prefix "a"
---@param a
--         ^ comp: alpha
function paramTest(alpha, beta)
end

-- Method params (colon syntax), partial "x"
---@param x
--         ^ comp: x
function SomeTable:myMethod(x, y)
end

-- ── Type completions ─────────────────────────────────────────────────────────

-- After @return with prefix "nu"
---@return nu
--           ^ comp: number
function typeTest() return 1 end

-- After @return with prefix "s" → self, string
---@return s
--          ^ comp: self, string
function typeTest2() return "" end

-- After @type with prefix "b"
---@type b
--        ^ comp: boolean
local typeTestVar

-- ── Dot/colon completions with partial names ───────────────────────────────

---@class CompTestTable
---@field alpha number
---@field beta string
local compTest = {}

---@return nil
function compTest:doAction()
end

---@return nil
function compTest:doOther()
end

-- Colon completion with partial method name typed
compTest:do
--          ^ comp: doAction, doOther

-- Dot completion with partial field name typed
compTest.al
--         ^ comp: alpha
