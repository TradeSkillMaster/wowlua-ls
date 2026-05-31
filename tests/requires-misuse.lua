-- Test: misuse of @requires and @return self<X>
--
-- @requires gates a method on the receiver's class type parameter, and
-- @return self<X> re-parameterizes the returned self. Using either in the wrong
-- place (not on a function, naming an unknown type param, with a malformed
-- shape, an undefined constraint type, or a non-generic class) must be
-- diagnosed rather than silently producing a dead constraint or useless
-- re-parameterization.

-- ── @requires not attached to a function ────────────────────────────────────

---@requires T: boolean
local notAFunc = 5
-- ^ diag: doc-func-no-function

-- ── @requires names a type param the class doesn't have ─────────────────────

---@class Box<U>
local Box = {}

---@requires T: boolean
function Box:Bad() end
-- ^ diag: malformed-annotation

-- ── @requires on a plain function (no receiver class) ───────────────────────

---@requires T: boolean
local function plainFn() end
-- ^ diag: malformed-annotation
plainFn()

-- ── @requires with an undefined constraint type ─────────────────────────────

---@class Cup<T>
local Cup = {}

---@requires T: NoSuchType
function Cup:Sip() end
-- ^ diag: undefined-doc-name

-- ── @requires with a malformed shape (missing colon) ────────────────────────

---@class Jar<T>
local Jar = {}

---@requires T
function Jar:Open() end
-- ^ diag: malformed-annotation

-- ── @return self<X> on a non-generic class ──────────────────────────────────

---@class Plain
local Plain = {}

---@return self<boolean>
function Plain:Foo() return self end
-- ^ diag: malformed-annotation

-- ── @return self<X> with the wrong number of type arguments ─────────────────

---@class Pair<K>
local Pair = {}

---@return self<string, number>
function Pair:Two() return self end
-- ^ diag: malformed-annotation

-- ── @return self<X> with an undefined type argument ─────────────────────────

---@class Holder<T>
local Holder = {}

---@return self<Nope>
function Holder:Get() return self end
-- ^ diag: undefined-doc-name

-- ── Valid: @requires on the class's own type param, correct self<X> arity ────

---@class Wrap<T>
local Wrap = {}

---@requires T: boolean
---@return self<boolean>
function Wrap:Invert() return self end

---@type Wrap<boolean>
local wb = {}
local ok = wb:Invert()
--    ^ hover: (local) ok: Wrap<boolean>

---@type Wrap<string>
local ws = {}
local bad = ws:Invert()
--    ^ hover: (local) bad: Wrap<boolean>
--             ^ diag: param-constraint-mismatch
