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
--               ^ hover: (method) function Publisher:IgnoreNil()\n-> self<string>

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

-- @return self<V1|V2>: union type args substitute each generic member
---@generic V1, V2
---@param trueValue V1
---@param falseValue V2
---@return self<V1|V2>
function Publisher:ReplaceBooleanWith(trueValue, falseValue) return self end

local pub7 = pub:ReplaceBooleanWith("hello", 5)
--               ^ hover: (method) function Publisher:ReplaceBooleanWith(trueValue: string, falseValue: number)\n-> self<string | number>

-- @return self<T!|V>: NonNil inside union strips nil from T only
---@generic V
---@param value V
---@return self<T!|V>
function Publisher:CoalesceNil(value) return self end

local pub8 = pub:CoalesceNil("")
--    ^ hover: (local) pub8: Publisher<string>

---@type Publisher<number|nil>
local pubNilNum = {}
local pub9 = pubNilNum:CoalesceNil("")
--    ^ hover: (local) pub9: Publisher<number | string>

-- ── Overload with self<R> and generic callback inference ─────────────────────

---@class Stream<T>
local Stream = {}

---@generic R
---@overload fun(map: (fun(value: T): R)): self<R>
---@param map fun(value: T): any
---@return self
function Stream:Map(map) return self end

---@type Stream<string>
local stream = {}

-- Inline function callback: R inferred from body return type
local mapped1 = stream:Map(function(value) return 42 end)
--    ^ hover: (local) mapped1: Stream<number>
--                     ^ hover: (method) function Stream:Map(map: fun(value: string): any)\n-> self\nfunction Stream:Map(map: fun(value: string): number)\n-> self<number>

-- Named function callback
---@param value string
---@return boolean
local function toBool(value) return value ~= "" end
local mapped2 = stream:Map(toBool)
--    ^ hover: (local) mapped2: Stream<boolean>
--                     ^ hover: (method) function Stream:Map(map: fun(value: string): any)\n-> self\nfunction Stream:Map(map: fun(value: string): boolean)\n-> self<boolean>

-- Fallback to @return self when called with non-function (diagnostic expected)
local mapped3 = stream:Map("something")
--                         ^ diag: type-mismatch
--    ^ hover: (local) mapped3: Stream<string>

-- Chain: Map then Map
local mapped4 = stream:Map(function(value) return 42 end):Map(function(value) return value > 0 end)
--    ^ hover: (local) mapped4: Stream<boolean>

-- Overload self<R> with multi-param callback
---@generic A, R
---@overload fun(map: (fun(value: T, arg: A): R), arg?: A): self<R>
---@param map (fun(value: T, arg: A): any)|string|number|table
---@return self
function Stream:MapWithArg(map, arg) return self end

local mapped5 = stream:MapWithArg(function(value, extra) return 42 end)
--    ^ hover: (local) mapped5: Stream<number>

-- Overload self<R> with table<T,R> lookup table
---@generic R
---@overload fun(map: table<T,R>): self<R>
---@param map fun(value: T): any
---@return self
function Stream:MapLookup(map) return self end

local LOOKUP = {
    ["alpha"] = 100,
    ["beta"] = 200,
}
local mapped_lookup = stream:MapLookup(LOOKUP)
--    ^ hover: (local) mapped_lookup: Stream<number>

-- Bracket-keyed table with non-literal keys (deferred value_type inference)
---@alias StreamKey string
---@type table<StreamKey, boolean>
local KEYED = {}
KEYED["x"] = true
local mapped_keyed = stream:MapLookup(KEYED)
--    ^ hover: (local) mapped_keyed: Stream<boolean>

-- Overload self<T!> — NonNil stripping in overload return
---@overload fun(): self<T!>
---@return self
function Stream:IgnoreNilOverload() return self end

---@type Stream<string?>
local streamNullable = {}
local mapped6 = streamNullable:IgnoreNilOverload()
--    ^ hover: (local) mapped6: Stream<string>
--                              ^ hover: (method) function Stream:IgnoreNilOverload()\n-> self\nfunction Stream:IgnoreNilOverload()\n-> self<string>

-- ── Renamed parent type param translated through parameterized parent ────────
-- A subclass that renames its parent's type param (`Box<T>` becomes `TCur` in
-- the child) must translate the parent's `@requires T` / `@return self<R>` onto
-- the child's binding, and preserve trailing child params across `self<R>`.

---@class Box<T>
local Box = {}

---@requires T: boolean
---@return self<boolean>
function Box:Flip() return self end

---@generic R
---@param value R
---@return self<R>
function Box:ReplaceWith(value) return self end

---@return T
function Box:GetValue() return nil end

---@class SharedBox<TCur, TShared> : Box<TCur>
local SharedBox = {}

-- Receiver's TCur=string violates Box's `@requires T: boolean` (translated
-- through the renamed parent binding `Box<TCur>`).
---@type SharedBox<string, string>
local sbStr = {}
sbStr:Flip()
--    ^ diag: param-constraint-mismatch

-- Receiver's TCur=boolean satisfies the constraint: clean.
---@type SharedBox<boolean, string>
local sbBool = {}
sbBool:Flip()

-- Inherited `@return T` with mismatched arity (Box has 1 param, SharedBox has 2)
-- doesn't substitute yet — `bind_receiver_type_args` requires matching arity.
-- TODO: merge class_type_param_subs into generic_subs for inherited returns.
local sbVal = sbStr:GetValue()
--    ^ hover: (local) sbVal: ?

-- `@return self<R>` re-parameterizes only the leading param; the trailing
-- TShared must survive (SharedBox<R, string>, not SharedBox<R>).
local sbReplaced = sbStr:ReplaceWith(123)
--    ^ hover: (local) sbReplaced: SharedBox<number, string>

-- Transitive: a 3-level chain renames the param twice. GrandBox's `@requires`
-- on its own T must reach MidBox<TM> : GrandBox<TM> and Leaf<TL> : MidBox<TL>.
---@class GrandBox<T>
local GrandBox = {}

---@requires T: boolean
---@return self<boolean>
function GrandBox:Toggle() return self end

---@class MidBox<TM> : GrandBox<TM>
local MidBox = {}

---@class Leaf<TL> : MidBox<TL>
local Leaf = {}

---@type Leaf<string>
local leafStr = {}
leafStr:Toggle()
--      ^ diag: param-constraint-mismatch

---@type Leaf<boolean>
local leafBool = {}
leafBool:Toggle()

-- ── Optional keyof-constrained param on a chained method (unbound T) ─────────
-- Regression: `@generic K: keyof T` makes `key? K` resolve to `K | nil`. On a
-- receiver whose class type param T is unbound (here `---@type KChain` with no
-- type arg), calling `(nil, "literalKey")` must NOT collapse `K | nil` to `nil`
-- and emit a bogus `expected nil` type-mismatch. The first `nil` arg binds the
-- sibling generic V; substitution must keep the unbound K so the param type is
-- treated as unresolved and skipped. Each call returns `self` so the chain
-- composes (mirrors a reactive publisher-schema builder chain).

---@class KChain<T>
local KChain = {}

---@generic V, K: keyof T
---@param value V
---@param key? K
---@return self
function KChain:IgnoreIfEquals(value, key) return self end

---@param value any
---@return self
function KChain:ReplaceWith(value) return self end

---@type KChain
local kchain = {}

-- Multi-line chain: each call passes (nil, "literalKey") with no false positive.
kchain
	:IgnoreIfEquals(nil, "alpha")
	:IgnoreIfEquals(nil, "beta")
	:ReplaceWith(nil)

-- Single-line chain resolves to self (the receiver type) the whole way down.
local kchained = kchain:IgnoreIfEquals(nil, "alpha"):IgnoreIfEquals(nil, "beta"):ReplaceWith(nil)
--    ^ hover: (local) kchained: KChain {

-- A single call also resolves to self (returns the receiver type).
local ksingle = kchain:IgnoreIfEquals(nil, "gamma")
--    ^ hover: (local) ksingle: KChain {
