---@diagnostic disable: create-global, shadowed-local, undefined-global, unused-function, unused-local
-- Test: @builds-field builder pattern (single-file)

---@class BuilderSchema
local Schema = {}

---@param name string
---@builds-field 1 string
---@return self
function Schema:AddString(name)
    return self
end

---@param name string
---@builds-field 1 number?
---@return self
function Schema:AddNumber(name)
    return self
end

---@param name string
---@builds-field 1 boolean
---@return self
function Schema:AddBool(name)
    return self
end

---@return built
function Schema:Build()
    return {}
end

-- ── Basic builder chain ─────────────────────────────────────────────

local s = Schema:AddString("label"):AddNumber("count"):AddBool("active")

local inst = s:Build()

local lbl = inst.label
--    ^ hover: (local) lbl: string  def: local

local cnt = inst.count
--    ^ hover: (local) cnt: number?  def: local

local act = inst.active
--    ^ hover: (local) act: boolean  def: local

-- ── With parent class ───────────────────────────────────────────────

---@class BuiltBase
---@field GetValue fun(self, key: string): any

---@return built : BuiltBase
function Schema:BuildWithParent()
    return {}
end

local s2 = Schema:AddString("name")
local inst2 = s2:BuildWithParent()

local nm = inst2.name
--    ^ hover: (local) nm: string  def: local

-- Inherited method from BuiltBase
inst2:GetValue("x")

-- ── @return built with no prior @builds-field calls ─────────────────

local s3 = Schema
local inst3 = s3:Build()
--    ^ hover: (local) inst3: table

-- ── Non-literal field name: graceful degradation ────────────────────

local varName = "dynamic"
local s4 = Schema:AddString(varName)
local inst4 = s4:Build()
--    ^ hover: (local) inst4: table

-- ── Same field name added twice: last type wins ─────────────────────

local s5 = Schema:AddString("x"):AddNumber("x")
local inst5 = s5:Build()
local dup = inst5.x
--    ^ hover: (local) dup: number?

-- ── Complex field types ─────────────────────────────────────────────

---@class FieldClass
---@field value number

---@param name string
---@builds-field 1 FieldClass
---@return self
function Schema:AddClassField(name)
    return self
end

---@param name string
---@builds-field 1 fun(x: number): string
---@return self
function Schema:AddFuncField(name)
    return self
end

---@param name string
---@builds-field 1 string[]
---@return self
function Schema:AddArrayField(name)
    return self
end

local s6 = Schema:AddClassField("obj"):AddFuncField("callback"):AddArrayField("names")
local inst6 = s6:Build()

local obj = inst6.obj
--    ^ hover: (local) obj: FieldClass {

local cb = inst6.callback
--    ^ hover: (local) function cb(x: number)\n-> string

local arr = inst6.names
--    ^ hover: (local) arr: string[]

-- ── Direct chain without intermediate variable ──────────────────────

local directInst = Schema:AddString("key"):AddBool("flag"):Build()
local dk = directInst.key
--    ^ hover: (local) dk: string  def: local

local df = directInst.flag
--    ^ hover: (local) df: boolean  def: local

-- ── Malformed @builds-field diagnostics ─────────────────────────────

---@builds-field
-- ^ diag: malformed-annotation

---@builds-field abc string
-- ^ diag: malformed-annotation

---@builds-field 0 string
-- ^ diag: malformed-annotation

---@builds-field 1
-- ^ diag: malformed-annotation

-- Valid @builds-field syntax — but orphaned (not above a function)
---@builds-field 1 string
-- ^ diag: doc-func-no-function

-- ── @return ClassName instead of @return self ───────────────────────
-- Methods returning their own class name should produce diagnostics

---@class TypedSchema
local TypedSchema = {}

---@param name string
---@builds-field 1 string
---@return TypedSchema
function TypedSchema:AddStr(name)
-- ^ diag: builds-field-not-self
    return self
end

---@param name string
---@builds-field 1 number
---@return TypedSchema
function TypedSchema:AddNum(name)
-- ^ diag: builds-field-not-self
    return self
end

---@return TypedSchema
function TypedSchema:Commit()
-- ^ diag: return-self-class-name
    return self
end

---@return built
function TypedSchema:Create()
    return {}
end

local ts = TypedSchema:AddStr("label"):AddNum("count"):Commit()
local tsInst = ts:Create()

local tsLabel = tsInst.label
--    ^ hover: (local) tsLabel: ?

local tsCount = tsInst.count
--    ^ hover: (local) tsCount: ?

-- ── @return built : UndefinedClass ──────────────────────────────────

---@return built : FakeClass123
function Schema:BuildBadParent()
-- ^ diag: undefined-doc-name
    return {}
end

-- ── Generic @builds-field ─────────────────────────────────────────

---@class GenSchema
local GenSchema = {}

---@generic T: FieldClass
---@param name string
---@param fieldType `T`
---@builds-field 1 T
---@return self
function GenSchema:AddTypedField(name, fieldType)
    return self
end

---@generic T: FieldClass
---@param name string
---@param fieldType `T`
---@builds-field 1 T?
---@return self
function GenSchema:AddOptionalTypedField(name, fieldType)
    return self
end

---@return built
function GenSchema:Finish()
    return {}
end

local gs = GenSchema:AddTypedField("item", FieldClass):AddOptionalTypedField("extra", FieldClass):Finish()

local gItem = gs.item
--    ^ hover: (local) gItem: FieldClass {

local gExtra = gs.extra
--    ^ hover: (local) gExtra: FieldClass?

-- ── Generic @builds-field with string literal arg ────────────────────

local gs2 = GenSchema:AddTypedField("strItem", "FieldClass"):AddOptionalTypedField("strExtra", "FieldClass"):Finish()

local gsItem2 = gs2.strItem
--    ^ hover: (local) gsItem2: FieldClass {

local gsExtra2 = gs2.strExtra
--    ^ hover: (local) gsExtra2: FieldClass?

-- ── @built-name: naming the built type ───────────────────────────────

---@class BNSchema2
local BNSchema2 = {}

---@built-name 1
---@return self
function BNSchema2.Create(name)
    return BNSchema2
end

---@param key string
---@builds-field 1 string
---@return self
function BNSchema2:AddStr(key)
    return self
end

---@param key string
---@builds-field 1 number
---@return self
function BNSchema2:AddNum(key)
    return self
end

---@return self
function BNSchema2:Commit()
    return self
end

---@return built
function BNSchema2:Done()
    return {}
end

local MY_BUILT = BNSchema2.Create("MyBuiltType")
    :AddStr("label")
    :AddNum("count")
    :Commit()

local myInst = MY_BUILT:Done()
--    ^ hover: (local) myInst: MyBuiltType {  def: local

local myLabel = myInst.label
--    ^ hover: (local) myLabel: string  def: local

local myCount = myInst.count
--    ^ hover: (local) myCount: number  def: local

-- Use the built name in @param annotation
---@param state MyBuiltType
function useBuiltName(state)
    local x = state.label
    --    ^ hover: (local) x: string
end

-- ── @correlated on @class whose fields come from builder pattern ────
-- The supplementary @class block has no @field entries; the fields
-- "label" and "count" are created by AddStr/AddNum above.

---@class MyBuiltType
---@correlated label, count

-- ── @built-name malformed diagnostics ────────────────────────────────

---@built-name
-- ^ diag: malformed-annotation

---@built-name abc
-- ^ diag: malformed-annotation

---@built-name 0
-- ^ diag: malformed-annotation

-- Valid @built-name syntax — but orphaned (not above a function)
---@built-name 1
-- ^ diag: doc-func-no-function

-- ── @built-extends: schema extension across class hierarchies ────

---@param name string
---@built-name 1
---@built-extends
---@return self
function Schema:Extend(name)
    return self
end

-- Base schema
local BASE_SCHEMA = Schema:AddString("baseName"):AddBool("baseActive")

-- Extend creates a new named type that inherits from the base's built type
local CHILD_SCHEMA = BASE_SCHEMA:Extend("ChildState"):AddString("childLabel"):AddNumber("childCount")

local childInst = CHILD_SCHEMA:Build()

-- Child's own fields
local cLabel = childInst.childLabel
--    ^ hover: (local) cLabel: string  def: local

local cCount = childInst.childCount
--    ^ hover: (local) cCount: number?  def: local

-- Inherited base fields via parent class
local cBase = childInst.baseName
--    ^ hover: (local) cBase: string  def: local

local cActive = childInst.baseActive
--    ^ hover: (local) cActive: boolean

-- Multi-level: grandchild extends child
local GRAND_SCHEMA = CHILD_SCHEMA:Extend("GrandState"):AddString("grandField")

local grandInst = GRAND_SCHEMA:Build()

-- Grandchild's own field
local gField = grandInst.grandField
--    ^ hover: (local) gField: string

-- Inherited from child
local gLabel = grandInst.childLabel
--    ^ hover: (local) gLabel: string

-- Inherited from base (through child)
local gBase = grandInst.baseName
--    ^ hover: (local) gBase: string

-- ── Backtick generic in union param (T|`T`) ─────────────────────────

---@class UnionBTSchema
local UnionBTSchema = {}

---@generic T
---@param key string
---@param class T|`T`
---@builds-field 1 T?
---@return self
function UnionBTSchema:AddOptionalClassField(key, class)
    return self
end

---@return built
function UnionBTSchema:Commit()
    return {}
end

---@class UBTClass
---@field val number
local UBTClass = {}

-- String literal arg — resolved via backtick class lookup
local ubts = UnionBTSchema:AddOptionalClassField("item", "UBTClass"):Commit()

local ubtItem = ubts.item
--    ^ hover: (local) ubtItem: UBTClass?

-- Class variable arg — resolved directly from the table type
local ubts2 = UnionBTSchema:AddOptionalClassField("item2", UBTClass):Commit()

local ubtItem2 = ubts2.item2
--    ^ hover: (local) ubtItem2: UBTClass?

-- ── @built-extends child assignable to parent type ──────────────────

-- Named base schema using BNSchema2 which has @built-name
local NAMED_BASE = BNSchema2.Create("BaseState")
    :AddStr("baseProp")
    :Commit()

-- Child extends the base
---@param name string
---@built-name 1
---@built-extends
---@return self
function BNSchema2:Extend(name)
    return self
end

local NAMED_CHILD = NAMED_BASE:Extend("ChildState"):AddStr("childProp"):Commit()

local childDone = NAMED_CHILD:Done()
--    ^ hover: (local) childDone: ChildState {

-- Child's own field
local cprop = childDone.childProp
--    ^ hover: (local) cprop: string

-- Inherited field
local bprop = childDone.baseProp
--    ^ hover: (local) bprop: string

-- Passing child type to function expecting parent type should NOT produce type-mismatch
---@param state BaseState
function acceptBaseState(state)
    local x = state.baseProp
    --    ^ hover: (local) x: string
end

acceptBaseState(childDone)

-- ── Assigning to optional built-table field should NOT trigger field-type-mismatch ──

---@class OptSchema
local OptSchema = {}

---@generic T
---@param key string
---@param class T|`T`
---@builds-field 1 T?
---@return self
function OptSchema:AddOptField(key, class)
    return self
end

---@return built
function OptSchema:Done()
    return {}
end

---@class OptVal
---@field x number
local OptVal = {}

local optInst = OptSchema:AddOptField("thing", "OptVal"):Done()
local optRead = optInst.thing
--    ^ hover: (local) optRead: OptVal?

-- Assigning a concrete value to an optional field should not trigger field-type-mismatch
optInst.thing = OptVal

-- ── Long builder chain regression test (>100 chained calls) ──────────
-- Previously hit the 200-depth recursion limit in resolve_expr.
-- The iterative chain resolution should handle this without error.

local longChain = Schema
    :AddString("f001"):AddString("f002"):AddString("f003"):AddString("f004"):AddString("f005")
    :AddString("f006"):AddString("f007"):AddString("f008"):AddString("f009"):AddString("f010")
    :AddString("f011"):AddString("f012"):AddString("f013"):AddString("f014"):AddString("f015")
    :AddString("f016"):AddString("f017"):AddString("f018"):AddString("f019"):AddString("f020")
    :AddString("f021"):AddString("f022"):AddString("f023"):AddString("f024"):AddString("f025")
    :AddString("f026"):AddString("f027"):AddString("f028"):AddString("f029"):AddString("f030")
    :AddString("f031"):AddString("f032"):AddString("f033"):AddString("f034"):AddString("f035")
    :AddString("f036"):AddString("f037"):AddString("f038"):AddString("f039"):AddString("f040")
    :AddString("f041"):AddString("f042"):AddString("f043"):AddString("f044"):AddString("f045")
    :AddString("f046"):AddString("f047"):AddString("f048"):AddString("f049"):AddString("f050")
    :AddString("f051"):AddString("f052"):AddString("f053"):AddString("f054"):AddString("f055")
    :AddString("f056"):AddString("f057"):AddString("f058"):AddString("f059"):AddString("f060")
    :AddString("f061"):AddString("f062"):AddString("f063"):AddString("f064"):AddString("f065")
    :AddString("f066"):AddString("f067"):AddString("f068"):AddString("f069"):AddString("f070")
    :AddString("f071"):AddString("f072"):AddString("f073"):AddString("f074"):AddString("f075")
    :AddString("f076"):AddString("f077"):AddString("f078"):AddString("f079"):AddString("f080")
    :AddString("f081"):AddString("f082"):AddString("f083"):AddString("f084"):AddString("f085")
    :AddString("f086"):AddString("f087"):AddString("f088"):AddString("f089"):AddString("f090")
    :AddString("f091"):AddString("f092"):AddString("f093"):AddString("f094"):AddString("f095")
    :AddString("f096"):AddString("f097"):AddString("f098"):AddString("f099"):AddString("f100")
    :AddNumber("f101"):AddNumber("f102"):AddNumber("f103"):AddNumber("f104"):AddNumber("f105")
    :AddNumber("f106"):AddNumber("f107"):AddNumber("f108"):AddNumber("f109"):AddNumber("f110")
    :AddNumber("f111"):AddNumber("f112"):AddNumber("f113"):AddNumber("f114"):AddNumber("f115")
    :AddNumber("f116"):AddNumber("f117"):AddNumber("f118"):AddNumber("f119"):AddNumber("f120")
    :AddBool("f121"):AddBool("f122"):AddBool("f123"):AddBool("f124"):AddBool("f125")
    :AddBool("f126"):AddBool("f127"):AddBool("f128"):AddBool("f129"):AddBool("f130")
    :AddBool("f131"):AddBool("f132"):AddBool("f133"):AddBool("f134"):AddBool("f135")
    :AddBool("f136"):AddBool("f137"):AddBool("f138"):AddBool("f139"):AddBool("f140")
    :AddBool("f141"):AddBool("f142"):AddBool("f143"):AddBool("f144"):AddBool("f145")
    :AddBool("f146"):AddBool("f147"):AddBool("f148"):AddBool("f149"):AddBool("f150")

-- No safety-limit error should be emitted

-- ── @built-name class: call-site diagnostics re-run after class discovery ──
-- Regression test: when a function param is typed as a @built-name class,
-- calls whose args reference that param resolved to None on their first pass
-- (the class was registered mid-fixpoint via clone_table_with_built_name).
-- Without the fixpoint re-check, need-check-nil never fires for those args.

---@class BNSchema
local BNSchema = {}

---@param name string
---@built-name 1
---@return BNSchema
function BNSchema.Create(name) return BNSchema end

---@param key string
---@builds-field 1 string?
---@return self
function BNSchema:AddOpt(key) return self end

---@return self
function BNSchema:Commit() return self end

local _ = BNSchema.Create("BNState")
    :AddOpt("label")
    :Commit()

---@param s string
local function needsStr(s) return s end

---@param state BNState
local function bnReader(state)
    needsStr(state.label)
    -- ^ diag: need-check-nil
end

local longInst = longChain:Build()

local longFirst = longInst.f001
--    ^ hover: (local) longFirst: string

local longMiddle = longInst.f075
--    ^ hover: (local) longMiddle: string

local longNum = longInst.f110
--    ^ hover: (local) longNum: number?

local longBool = longInst.f150
--    ^ hover: (local) longBool: boolean

-- ── Lateinit builder fields (@builds-field with !) ────────────────

---@class LateinitSchema
local LSchema = {}

---@param name string
---@builds-field 1 SomeHandler!
---@return self
function LSchema:AddHandler(name)
    return self
end

---@return built
function LSchema:Commit()
    return {}
end

---@class SomeHandler
---@field Cancel fun(self)
local SomeHandler = {}

local ls = LSchema:AddHandler("myHandler"):Commit()

-- Lateinit field allows nil assignment (no field-type-mismatch)
if ls.myHandler then
    ls.myHandler:Cancel()
    ls.myHandler = nil
end

-- ── inject-field on @param of built type with mixed @field + built fields ─
-- When a class has explicit @field declarations AND builder-added fields,
-- assigning to a built field should NOT fire inject-field.

---@class MixedBuilt
---@field staticField string
local MixedBuiltSchema = {}

---@param key string
---@builds-field 1 number
---@return self
function MixedBuiltSchema:AddNum(key)
    return self
end

---@built-name 1
---@return self
function MixedBuiltSchema.Create(name)
    return MixedBuiltSchema
end

---@return built
function MixedBuiltSchema:Done()
    return {}
end

local MIXED = MixedBuiltSchema.Create("MixedState")
    :AddNum("dynamicField")
    :Done()

---@param state MixedState
function assignMixedBuilt(state)
    state.dynamicField = 42
end

-- ── @class overlay on @built-name types ─────────────────────────────
-- A @class declaration that re-uses a @built-name class name should
-- merge its @field annotations with the builder-pattern fields.

---@class OverlaySchema
local OSchema = {}

---@built-name 1
---@return self
function OSchema.Create(name)
    return OSchema
end

---@param key string
---@builds-field 1 string
---@return self
function OSchema:AddStr(key)
    return self
end

---@param key string
---@builds-field 1 table
---@return self
function OSchema:AddTable(key)
    return self
end

---@return self
function OSchema:Finish()
    return self
end

---@return built
function OSchema:Done()
    return {}
end

local OV_SCHEMA = OSchema.Create("OverlayTarget")
    :AddStr("builtLabel")
    :AddTable("items")
    :Finish()

-- @class overlay: override "items" type from table → number[]!,
-- add a new typed field "extra". Note: must be on its own (not preceding
-- a local statement) so it's treated as an overlay, not a variable type.
---@class OverlayTarget
---@field items number[]!
---@field extra boolean

-- Using the built type with overlay fields merged
local ovInst = OV_SCHEMA:Done()
--    ^ hover: (local) ovInst: OverlayTarget {  def: local

-- Built field preserved via merge (not overridden by overlay)
local ovLabel = ovInst.builtLabel
--    ^ hover: (local) ovLabel: string  def: local

-- Overlay field added by @class
local ovExtra = ovInst.extra
--    ^ hover: (local) ovExtra: boolean

-- Overlay field overriding built type (number[]! instead of table)
local ovItems = ovInst.items
--    ^ hover: (local) ovItems: number[]

-- No inject-field for built fields
ovInst.builtLabel = "x"

-- No inject-field for overlay fields
ovInst.extra = true

-- No inject-field for overridden fields
ovInst.items = {}

-- Inject-field still fires for truly undefined fields
ovInst.unknownField = 1
--     ^ hover: (field) unknownField: number
-- ^ diag: inject-field

-- Use overlay type in @param
---@param target OverlayTarget
function useOverlayTarget(target)
    local lb = target.builtLabel
    --    ^ hover: (local) lb: string
    local ex = target.extra
    --    ^ hover: (local) ex: boolean
    local it = target.items
    --    ^ hover: (local) it: number[]
end
