---@diagnostic disable: create-global, missing-return, unused-function, unused-local
---@class ExprState
---@field progress number
---@field active boolean
---@field name string
---@field count number

---@param expr expression<ExprState, boolean>
function checkBool(expr) end

---@param expr expression<ExprState>
function checkAny(expr) end

---@param expr expression<ExprState, number>
function checkNum(expr) end

-- Hover on identifiers inside expression strings
checkBool([[progress == 1 and not active]])
--           ^ hover: (field) progress: number
--                                  ^ hover: (field) active: boolean

-- Undefined variable diagnostic
checkBool([[unknownVar == 1]])
--           ^ diag: undefined-field

-- Expression type mismatch: name is string, expected boolean
checkBool([[name]])
-- ^ diag: type-mismatch

-- No mismatch when return type is unconstrained
checkAny([[name]])
--          ^ hover: (field) name: string

-- Boolean expression matches boolean constraint
checkBool([[progress == 1]])

-- Numeric expression matches number constraint
checkNum([[progress + count]])

-- Numeric expression doesn't match boolean constraint
checkBool([[progress + count]])
-- ^ diag: type-mismatch

-- Completions inside expression string
checkBool([[prog]])
--            ^ comp: progress, active, name, count  diag: undefined-field

-- Regular string delimiter
checkBool("progress == 1")
--           ^ hover: (field) progress: number

-- not operator infers boolean
checkBool([[not active]])

-- Comparison operators infer boolean
checkBool([[count > 0]])

-- Semantic tokens for expression identifiers
checkBool([[progress == 1 and not active]])
--           ^ tok: variable
--                      ^ tok: number
--                        ^ tok: keyword
--                            ^ tok: keyword
--                                ^ tok: variable

-- Boolean/nil literals are `builtinConstant` (the constant color), distinct from the
-- `keyword` color used for the `and`/`or`/`not` logical operators, matching plain Lua.
checkBool([[active and true or false]])
--            ^ tok: variable
--                  ^ tok: keyword
--                      ^ tok: builtinConstant
--                          ^ tok: keyword
--                              ^ tok: builtinConstant

-- Boolean literal in a grouped subexpression with `not` (mirrors the reported case)
checkBool([[(active or active) and true or not active or active]])
--                                   ^ tok: builtinConstant

-- `nil` literal also gets `builtinConstant` (not just `true`/`false`)
checkAny([[active or nil]])
--                    ^ tok: builtinConstant

-- expression<self> resolves to receiver's class
---@class SelfExprState
---@field ready boolean
---@field value number
local SelfExprState = {}

---@param expr expression<self>
function SelfExprState:watch(expr) end

local obj = SelfExprState
obj:watch([[ready]])
--           ^ hover: (field) ready: boolean

obj:watch([[missing]])
--           ^ diag: undefined-field

-- Function calls: undeclared callee names are flagged
checkBool([[progress > 0 and unknownFn(count) > 1]])
--           ^ hover: (field) progress: number
--                             ^ diag: undefined-field

-- Intersection type in expression context: fields from both classes available
---@class ExprFuncs
---@field min fun(a: number, b: number): number
---@field max fun(a: number, b: number): number

---@param expr expression<ExprState & ExprFuncs, number>
function checkIntersect(expr) end

-- Declared via intersection: callee is recognized
checkIntersect([[min(progress, count)]])
--                ^ hover: (field) min: fun(a: number, b: number): number  tok: function
--                    ^ hover: (field) progress: number

-- Hover and def on function from intersected class
checkIntersect([[max(progress, count)]])
--                ^ hover: (field) max: fun(a: number, b: number): number  def: local  tok: function

-- Unknown names still flagged with combined class name
checkIntersect([[badVar + 1]])
--                ^ diag: undefined-field

-- Completions include fields from both classes
checkIntersect([[pro]])
--                ^ comp: progress, active, name, count, min, max  diag: undefined-field

-- expression<self & Funcs> works with intersection
---@class SelfWithFuncs
---@field value number
local SelfWithFuncs = {}

---@class SelfExprFuncs
---@field clamp fun(v: number, lo: number, hi: number): number

---@param expr expression<self & SelfExprFuncs, number>
function SelfWithFuncs:compute(expr) end

local sw = SelfWithFuncs
sw:compute([[clamp(value, 0, 100)]])
--            ^ hover: (field) clamp: fun(v: number, lo: number, hi: number): number
--                   ^ hover: (field) value: number

-- ── Class generic type param in expression context ──────────────────────────
-- `expression<T & Builtins, R>` on a method of `Mgr<T: Base>`: the receiver's
-- bound T (a concrete state class) must contribute its fields to the expression
-- context alongside the intersection's other member. Regression: a generic type
-- param like T was looked up as a class name (and never found), collapsing the
-- intersection to just Builtins so the state fields read as undefined.
---@class ExprStateBase
local ExprStateBase = {}

---@class ExprBuiltins
---@field rand fun(): number

---@class StateMgr<T: ExprStateBase>
local StateMgr = {}

---@generic R
---@param key string
---@param expr expression<T & ExprBuiltins, R>
function StateMgr:SetFromExpr(key, expr) end

---@class MgrState : ExprStateBase
---@field flag boolean
---@field level number

---@type StateMgr<MgrState>
local mgr = {}

-- Fields of the bound T (MgrState) are visible in the expression
mgr:SetFromExpr("x", [[flag and level > 0]])
--                     ^ hover: (field) flag: boolean
--                              ^ hover: (field) level: number

-- The other intersection member's fields are also visible
mgr:SetFromExpr("y", [[rand() > 0.5]])
--                     ^ hover: (field) rand: fun(): number

-- Unknown name still flagged
mgr:SetFromExpr("z", [[missing or flag]])
--                     ^ diag: undefined-field

-- ── Compound ValueType in generic substitution ────────────────────────────
-- When T is bound to an intersection (`StateMgr<Foo & Bar>`), the substituted
-- ValueType is `Intersection([Table(a), Table(b)])`. Both components' fields
-- must be visible in the expression context.
---@class MgrPartA : ExprStateBase
---@field health number

---@class MgrPartB : ExprStateBase
---@field mana number

---@type StateMgr<MgrPartA & MgrPartB>
local mgr2 = {}

-- Fields from both intersection members are visible
mgr2:SetFromExpr("a", [[health + mana]])
--                      ^ hover: (field) health: number
--                               ^ hover: (field) mana: number

-- Builtins intersection member is also still present
mgr2:SetFromExpr("b", [[rand() > 0.5]])
--                      ^ hover: (field) rand: fun(): number

-- Unknown name still flagged with compound binding
mgr2:SetFromExpr("c", [[missing]])
--                      ^ diag: undefined-field

-- ── Unbound generic type param in context: suppress, don't false-positive ──
-- When the receiver carries no type argument, the `T` in `expression<T & Builtins>`
-- can't be bound to a concrete class. `Builtins` still resolves, but `T` would have
-- supplied the other fields, so we must NOT flag names as undefined — the context is
-- only partially known. (Real-world trigger: a class field whose generic type arg is
-- lost across files, e.g. a chained-builder result the coarse cross-file scan can't
-- resolve, leaving the field typed `any`.)
---@type StateMgr
local mgrNoArg = {}

-- A name that would be a field of the (unbound) T must not be flagged. The
-- harness fails on any uncovered diagnostic, so the absence of a `diag:` here
-- asserts no false positive is emitted on these names.
mgrNoArg:SetFromExpr("x", [[flag and level > 0]])

-- The resolvable Builtins member is still available for hover
mgrNoArg:SetFromExpr("y", [[rand() > 0.5]])
--                          ^ hover: (field) rand: fun(): number

-- ── Generic result type: R is inferred from the expression and flows to @return ──
---@class ExprSchema<R>
local ExprSchema = {}

local ExprGen = {} ---@class ExprGenerator

---@generic R
---@param expr expression<ExprState, R>
---@return ExprSchema<R>
function ExprGen:Watch(expr) end

---@type ExprGenerator
local gen = {}

-- Numeric expression binds R = number
local numWatch = gen:Watch([[progress + count]])
--    ^ hover: (local) numWatch: ExprSchema<number>

-- Boolean expression binds R = boolean
local boolWatch = gen:Watch([[active and progress > 0]])
--    ^ hover: (local) boolWatch: ExprSchema<boolean>

-- Single-field expression binds R = string
local strWatch = gen:Watch([[name]])
--    ^ hover: (local) strWatch: ExprSchema<string>

-- Undefined field still flagged; R falls back to any when uninferable
local anyWatch = gen:Watch([[unknownThing]])
--    ^ hover: (local) anyWatch: ExprSchema<any>
--                      ^ diag: undefined-field

-- ── Generic R inferred from builder-defined (dynamic) fields ──
-- The context class fields are declared via @builds-field, not @field.
-- R should still be inferred from the expression body and flow to @return.
---@class DynState
local STATE_METHODS = {}

---@generic R
---@param expr expression<self, R>
---@return ExprSchema<R>
function STATE_METHODS:Publisher(expr) end

---@class DynBuilder
local DynBuilder = {}

---@built-name 1
---@return self
function DynBuilder.Create(name) return DynBuilder end

---@param key string
---@builds-field 1 boolean
---@return self
function DynBuilder:AddBoolField(key) return self end

---@param key string
---@builds-field 1 number
---@return self
function DynBuilder:AddNumField(key) return self end

---@return self
function DynBuilder:Commit() return self end

---@return built: DynState
function DynBuilder:CreateState() end

local dynState = DynBuilder.Create("MyDynState")
	:AddBoolField("flag")
	:AddNumField("amount")
	:Commit()
	:CreateState()

-- Builder-defined boolean field binds R = boolean
local flagPub = dynState:Publisher([[flag]])
--    ^ hover: (local) flagPub: ExprSchema<boolean>

-- Builder-defined number field binds R = number
local amountPub = dynState:Publisher([[amount + 1]])
--    ^ hover: (local) amountPub: ExprSchema<number>

-- Works through a param typed as the @built-name class too
---@param st MyDynState
local function useDynParam(st)
	local p = st:Publisher([[flag]])
	--    ^ hover: (local) p: ExprSchema<boolean>
end

-- ── Lateinit (T!) fields include nil in expression R inference ──
---@class LateinitBuilder
local LateinitBuilder = {}

---@built-name 1
---@return self
function LateinitBuilder.Create(name) return LateinitBuilder end

---@generic T
---@param key string
---@param class T|`T`
---@builds-field 1 T!
---@return self
function LateinitBuilder:AddDeferredField(key, class) return self end

---@param key string
---@builds-field 1 number
---@return self
function LateinitBuilder:AddNumField(key) return self end

---@return self
function LateinitBuilder:Commit() return self end

---@return built: DynState
function LateinitBuilder:CreateState() end

local liState = LateinitBuilder.Create("LIState")
	:AddDeferredField("frame", "ExprSchema")
	:AddNumField("count")
	:Commit()
	:CreateState()

-- Lateinit field binds R = ExprSchema? (includes nil)
local liPub = liState:Publisher([[frame]])
--    ^ hover: (local) liPub: ExprSchema<ExprSchema?>

-- Non-lateinit field still binds R without nil
local liNum = liState:Publisher([[count]])
--    ^ hover: (local) liNum: ExprSchema<number>

-- Lateinit on an already-nilable annotation deduplicates nil via make_union
---@param key string
---@builds-field 1 string?!
---@return self
function LateinitBuilder:AddNilableLI(key) return self end

local liState2 = LateinitBuilder.Create("LIState2")
	:AddNilableLI("tag")
	:Commit()
	:CreateState()

local liTag = liState2:Publisher([[tag]])
--    ^ hover: (local) liTag: ExprSchema<string?>
