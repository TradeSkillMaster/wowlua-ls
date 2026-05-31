-- Test: class type param substitution into call_func (@overload) resolution

-- ── Basic: parameterized class with returns<F> projection ───────────────────

---@class Iter<F>
---@overload fun(): returns<F>
local Iter = {}

---@type Iter<fun(): number, string>
local iter1 = {}

local a, b = iter1()
--    ^ hover: (local) a: number
--       ^ hover: (local) b: string

-- ── For-in loop: callable table with returns<F> ─────────────────────────────

---@type Iter<fun(): string, number>
local iter2 = {}
for k, v in iter2 do
    k = k
--  ^ hover: (local) k: string
    v = v
--  ^ hover: (local) v: number
end

-- ── Generic function returning parameterized callable ───────────────────────

---@generic F
---@param func F
---@return Iter<F>
---@diagnostic disable-next-line: return-mismatch
local function wrapIter(func) return {} end

---@param tbl table
---@return string
---@return number
local function myNext(tbl) return "", 0 end

local iter3 = wrapIter(myNext)
local c, d = iter3()
--    ^ hover: (local) c: string
--       ^ hover: (local) d: number

-- ── For-in with generic-inferred callable ───────────────────────────────────

local iter4 = wrapIter(myNext)
for k2, v2 in iter4 do
    k2 = k2
--  ^ hover: (local) k2: string
    v2 = v2
--  ^ hover: (local) v2: number
end

-- ── Non-generic call_func (no type params, no projection) ───────────────────

---@class SimpleCallable
---@overload fun(): boolean
local SimpleCallable = {}

---@type SimpleCallable
local sc = {}
local e = sc()
--    ^ hover: (local) e: boolean

-- ── Typed varargs in fun() return: ...string ────────────────────────────────

---@type Iter<fun(): number, ...string>
local iter5 = {}
for k5, v5, v5b in iter5 do
    k5 = k5
--  ^ hover: (local) k5: number
    v5 = v5
--  ^ hover: (local) v5: string
    v5b = v5b
--  ^ hover: (local) v5b: string
end

-- ── Bare varargs in fun() return: ... ───────────────────────────────────────

---@type Iter<fun(): number, ...>
local iter6 = {}
for k6, v6, v6b in iter6 do
    k6 = k6
--  ^ hover: (local) k6: number
    v6 = v6
--  ^ hover: (local) v6: any
    v6b = v6b
--  ^ hover: (local) v6b: any
end

-- ── Fewer loop variables than returns ───────────────────────────────────────

---@type Iter<fun(): string, number, boolean>
local iter7 = {}
for only7 in iter7 do
    only7 = only7
--  ^ hover: (local) only7: string
end

-- ── Direct call with more return bindings than F declares ───────────────────

---@type Iter<fun(): boolean>
local iter8 = {}
---@diagnostic disable-next-line: unbalanced-assignments
local f, g = iter8()
--    ^ hover: (local) f: boolean
--       ^ hover: (local) g: nil

-- ── Direct call with vararg F and excess bindings ──────────────────────────

---@type Iter<fun(): number, ...string>
local iter8v = {}
local fv, gv, hv = iter8v()
--    ^ hover: (local) fv: number
--        ^ hover: (local) gv: string
--            ^ hover: (local) hv: string

-- ── Table-constructor field inheriting parameterized callable ───────────────

local holder = {
    ---@type Iter<fun(): string, number>
    myIter = {},
}
for hk, hv in holder.myIter do
    hk = hk
--  ^ hover: (local) hk: string
    hv = hv
--  ^ hover: (local) hv: number
end

-- ── params<F> with varargs on callable class ────────────────────────────────

---@class Emitter<F>
local Emitter = {}

---@param ... params<F>
function Emitter:Emit(...) end

---@type Emitter<fun(name: string, count: number)>
local em1 = {}
em1:Emit("hello", 42)
--       ^ diag: none

em1:Emit(123, 42)
--       ^ diag: type-mismatch

-- ── @requires T constraint gating + @return self<X> re-parameterization ──────

---@class Wrap<T>
local Wrap = {}

---@requires T: boolean
---@return self<boolean>
function Wrap:Invert() return self end

-- Constraint satisfied: no diagnostic, returns re-parameterized self.
---@type Wrap<boolean>
local wbool = {}
local invOk = wbool:Invert()
--    ^ hover: (local) invOk: Wrap<boolean>

-- Constraint violated: receiver T is string, not boolean.
-- The return is still re-parameterized to Wrap<boolean> via @return self<boolean>.
---@type Wrap<string>
local wstr = {}
local invBad = wstr:Invert()
--    ^ hover: (local) invBad: Wrap<boolean>
--                  ^ diag: param-constraint-mismatch

-- ── @return self<T!> re-parameterization (non-nil stripping) ────────────────

---@class Publisher<T>
local Publisher = {}

---@return self
function Publisher:Filter() return self end

---@return self<T!>
function Publisher:IgnoreNil() return self end

---@return self<boolean>
function Publisher:ToBoolean() return self end

-- T is nilable: IgnoreNil() strips nil from the type param
---@type Publisher<string?>
local pub = {}
local pub2 = pub:IgnoreNil()
--    ^ hover: (local) pub2: Publisher<string>

-- Chain: Filter preserves nilability, IgnoreNil strips it
local pub3 = pub:Filter():IgnoreNil()
--    ^ hover: (local) pub3: Publisher<string>

-- Already non-nil: T! is a no-op
---@type Publisher<number>
local pubNum = {}
local pub4 = pubNum:IgnoreNil()
--    ^ hover: (local) pub4: Publisher<number>

-- T! with union containing nil: strips nil from union
---@type Publisher<number|string|nil>
local pubUnion = {}
local pub5 = pubUnion:IgnoreNil()
--    ^ hover: (local) pub5: Publisher<number | string>

-- ToBoolean after IgnoreNil: chains compose correctly
local pub6 = pub:IgnoreNil():ToBoolean()
--    ^ hover: (local) pub6: Publisher<boolean>
