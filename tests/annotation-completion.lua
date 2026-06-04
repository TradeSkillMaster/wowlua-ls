---@diagnostic disable: create-global, unused-function, unused-local
-- Annotation completion tests

-- ── Tag completions ──────────────────────────────────────────────────────────

-- All tags after ---@ (no context: shows everything)
---@
--  ^ comp: param, return, type, class, field, alias, enum, event, overload, defclass, generic, cast, as, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, accessor, meta, diagnostic, type-narrows, flavor-narrows, narrows-arg, requires, correlated, see

-- Partial prefix: "re" → return, requires
---@diagnostic disable-next-line: malformed-annotation
---@re
--    ^ comp: return, requires

-- Partial prefix: "se" → see
---@diagnostic disable-next-line: malformed-annotation
---@se
--    ^ comp: see

-- Partial prefix: "p" → param, private, protected
---@diagnostic disable-next-line: malformed-annotation
---@p
--   ^ comp: param, private, protected

-- Partial prefix: "cl" → class
---@diagnostic disable-next-line: malformed-annotation
---@cl
--    ^ comp: class

-- Partial prefix: "co" → constructor, correlated
---@diagnostic disable-next-line: malformed-annotation
---@co
--    ^ comp: constructor, correlated

-- Partial prefix: "fl" → flavor-narrows
---@diagnostic disable-next-line: malformed-annotation
---@fl
--    ^ comp: flavor-narrows

-- Non-matching prefix: no tag starts with "xyz" → empty, no fallback to globals
---@diagnostic disable-next-line: malformed-annotation
---@xyz
--     ^ comp: none

-- ── Context-aware filtering ────────────────────────────────────────────────

-- Function context: after @param, only function-applicable tags appear
---@param x number
---@
--  ^ comp: param, return, overload, defclass, generic, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, diagnostic, type-narrows, flavor-narrows, narrows-arg, requires, see
function ctxFuncTest(x) end

-- Class context: after @class, only class-applicable tags appear
---@class CtxClassTest
---@
--  ^ comp: field, accessor, correlated, overload, constructor, deprecated, nodiscard, private, protected, diagnostic, see

-- Function context inferred from function below (no prior tags)
---@
--  ^ comp: param, return, overload, defclass, generic, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, diagnostic, type-narrows, flavor-narrows, narrows-arg, requires, see
function ctxInferredFunc() end

-- Function context with params: "Annotate function" appears alongside tags
---@
--  ^ comp: Annotate function, param, return, overload, defclass, generic, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, diagnostic, type-narrows, flavor-narrows, narrows-arg, requires, see
function ctxWithParams(a, b) end

-- ── Param name completions ───────────────────────────────────────────────────

-- Partial param name prefix "a"
---@diagnostic disable-next-line: malformed-annotation
---@param a
--         ^ comp: alpha
function paramTest(alpha, beta)
end

-- Method params (colon syntax), partial "x"
---@diagnostic disable-next-line: malformed-annotation
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
---@diagnostic disable-next-line: cannot-call
compTest:do
--          ^ comp: doAction, doOther

-- Dot completion with partial field name typed
compTest.al
--         ^ comp: alpha

-- ── Generate annotations completions ──────────────────────────────────────

-- Generate annotations for function with parameters
---
-- ^ comp: Annotate function
function genAnnot(x, y)
    return x + y
end

-- No generation when above non-function
---
-- ^ comp: none
local plainVar = 42

-- No generation for parameterless void function
function voidNoArgs()
end
---
-- ^ comp: none
local afterVoid = 1

-- Generate when function has params but void return
---
-- ^ comp: Annotate function
function genParamsOnly(a, b, c)
end

-- No generation when annotation block already has @param
---@param x number
---
-- ^ comp: none
function alreadyAnnotated(x)
    return x
end

-- Method with self parameter (colon syntax) — self is skipped
---@class GenAnnotClass
local genAnnotObj = {}
---
-- ^ comp: Annotate function
function genAnnotObj:myMethod(val)
    return val
end

-- Function assigned to a local variable
---
-- ^ comp: Annotate function
local genAssigned = function(x, y) return x end

-- Varargs function
---
-- ^ comp: Annotate function
function genVarargs(fmt, ...)
end
