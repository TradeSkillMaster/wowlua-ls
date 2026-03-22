-- Annotation completion tests

-- ── Tag completions ──────────────────────────────────────────────────────────

-- All tags after ---@
---@
--  ^ comp: param, return, type, class, field, alias, enum, overload, defclass, generic, cast, as, builds-field, built-name, built-extends, constructor, deprecated, nodiscard, private, protected, accessor, meta, diagnostic, type-narrows

-- Partial prefix: "re" → return
---@re
--    ^ comp: return

-- Partial prefix: "p" → param, private, protected
---@p
--   ^ comp: param, private, protected

-- Partial prefix: "cl" → class
---@cl
--    ^ comp: class

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
