---@diagnostic disable: create-global, unused-function, unused-local, undefined-doc-name, unknown-diag-code, undefined-doc-class
-- Annotation completion tests

-- ── Tag completions ──────────────────────────────────────────────────────────

-- All tags after ---@ (no context: shows everything)
---@
--  ^ comp: param, return, type, class, field, alias, enum, event, overload, defclass, generic, cast, as, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, accessor, meta, diagnostic, type-narrows, returns-class-name, flavor-narrows, narrows-arg, creates-global, requires, correlated, see

-- Partial prefix: "re" → return, requires, returns-class-name
---@diagnostic disable-next-line: malformed-annotation
---@re
--    ^ comp: return, requires, returns-class-name

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

-- Partial prefix: "cr" → creates-global
---@diagnostic disable-next-line: malformed-annotation
---@cr
--    ^ comp: creates-global

-- Partial prefix: "cor" → correlated
---@diagnostic disable-next-line: malformed-annotation
---@cor
--     ^ comp: correlated

-- Non-matching prefix: no tag starts with "xyz" → empty, no fallback to globals
---@diagnostic disable-next-line: malformed-annotation
---@xyz
--     ^ comp: none

-- ── Context-aware filtering ────────────────────────────────────────────────

-- Function context: after @param, only function-applicable tags appear
---@param x number
---@
--  ^ comp: param, return, cast, overload, defclass, generic, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, diagnostic, type-narrows, returns-class-name, flavor-narrows, narrows-arg, creates-global, requires, see
function ctxFuncTest(x) end

-- Class context: after @class, only class-applicable tags appear
---@class CtxClassTest
---@
--  ^ comp: field, accessor, correlated, overload, constructor, deprecated, nodiscard, private, protected, diagnostic, see

-- Function context inferred from function below (no prior tags)
---@
--  ^ comp: param, return, cast, overload, defclass, generic, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, diagnostic, type-narrows, returns-class-name, flavor-narrows, narrows-arg, creates-global, requires, see
function ctxInferredFunc() end

-- Function context with params: "Annotate function" appears alongside tags
---@
--  ^ comp: Annotate function, param, return, cast, overload, defclass, generic, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, diagnostic, type-narrows, returns-class-name, flavor-narrows, narrows-arg, creates-global, requires, see
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

-- ── Type completions inside generic/compound type expressions ────────────────

---@class TcTheme
---@class TcThemeManager

-- Inside table<...> value position (single arg): partial "TcThe"
---@type table<TcThe
--                 ^ comp: TcTheme, TcThemeManager
local tcGenericVal

-- Inside table<K, V> value position (after comma): partial "TcThe"
---@type table<string, TcThe
--                          ^ comp: TcTheme, TcThemeManager
local tcGenericKV

-- Inside a fun(...) parameter type: partial "TcThe"
---@type fun(cb: TcThe
--                   ^ comp: TcTheme, TcThemeManager
local tcFunParam

-- Inside an intersection type: partial "TcThe"
---@type TcTheme & TcThe
--                     ^ comp: TcTheme, TcThemeManager
local tcIntersection

-- @param with a generic table type: partial "TcThe"
---@diagnostic disable-next-line: malformed-annotation
---@param registry table<string, TcThe
--                                    ^ comp: TcTheme, TcThemeManager
function tcParamGeneric(registry) end

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

-- ── @cast variable name completions ─────────────────────────────────────────

-- @cast completes local variable names in scope
function castCompletionTest()
    local myTarget = 1
    local myOther = "hello"
    ---@diagnostic disable-next-line: malformed-annotation
    ---@cast my
    --         ^ comp: myTarget, myOther
end

-- @cast with multiple locals, partial prefix filters correctly
function castCompletionMulti()
    local xFirst = 1
    local xSecond = "hi"
    local zOther = true
    ---@diagnostic disable-next-line: malformed-annotation
    ---@cast xS
    --          ^ comp: xSecond
end

-- ── @correlated field name completions ──────────────────────────────────────

-- @correlated completes field names from @field declarations
---@class CorrelatedTest
---@field title string?
---@field description string?
---@field count number?
---@diagnostic disable-next-line: malformed-annotation
---@correlated t
--              ^ comp: title

-- @correlated excludes already-listed fields
---@class CorrelatedExclude
---@field first string?
---@field second string?
---@field third string?
---@diagnostic disable-next-line: malformed-annotation
---@correlated first, s
--                     ^ comp: second

-- @correlated with fields declared after the annotation
---@class CorrelatedForward
---@diagnostic disable-next-line: malformed-annotation
---@correlated n
--              ^ comp: name, note
---@field name string?
---@field note string?
---@field value number?

-- ── @diagnostic code completions ────────────────────────────────────────────

-- @diagnostic disable: completes diagnostic codes with prefix "type"
---@diagnostic disable: type
--                           ^ comp: type-mismatch

-- @diagnostic enable: completes codes with prefix "un"
---@diagnostic enable: un
--                        ^ comp: undefined-global, undefined-field, unused-local, unreachable-code, undefined-doc-param, unknown-diag-code, unbalanced-assignments, unused-function, undefined-doc-class, undefined-doc-name, unused-vararg, unknown-param-type, unknown-return-type, unknown-local-type, unknown-field-type, unknown-callback-event, undefined-env-child, unknown-cast-variable, unknown-operator, unnecessary-assert, unused-label

-- @diagnostic disable-next-line: completes codes
---@diagnostic disable-next-line: red
--                                    ^ comp: redefined-local, redundant-return-value, redundant-value, redundant-return, redundant-or, redundant-and, redundant-condition, redundant-parameter, redundant-class-generic

-- @diagnostic disable: after comma, completes next code (excludes already-listed)
---@diagnostic disable: unused-local, type
--                                        ^ comp: type-mismatch

-- ── @class parent type completions ──────────────────────────────────────────

-- @class Foo: offers type names for the parent class
---@class QpBase
---@class QcChild: Qp
--                   ^ comp: QpBase

-- @class with (partial) prefix still offers parent completions
---@diagnostic disable-next-line: malformed-annotation
---@class (partial) QxPartial: Qp
--                               ^ comp: QpBase

-- @class without colon: no type completions (just the class name being defined)
---@diagnostic disable-next-line: malformed-annotation
---@class QnNewClass
--                   ^ comp: none

-- @class with multiple parents after comma
---@diagnostic disable-next-line: malformed-annotation
---@class QmMulti: QpBase, Qx
--                           ^ comp: QxPartial
