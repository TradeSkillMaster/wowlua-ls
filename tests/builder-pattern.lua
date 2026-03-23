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
--    ^ hover: (global) lbl: string

local cnt = inst.count
--    ^ hover: (global) cnt: number | nil

local act = inst.active
--    ^ hover: (global) act: boolean

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
--    ^ hover: (global) nm: string

-- Inherited method from BuiltBase
inst2:GetValue("x")
-- ^ diag: none

-- ── @return built with no prior @builds-field calls ─────────────────

local s3 = Schema
local inst3 = s3:Build()
--    ^ hover: (global) inst3: table

-- ── Non-literal field name: graceful degradation ────────────────────

local varName = "dynamic"
local s4 = Schema:AddString(varName)
local inst4 = s4:Build()
--    ^ hover: (global) inst4: table

-- ── Same field name added twice: last type wins ─────────────────────

local s5 = Schema:AddString("x"):AddNumber("x")
local inst5 = s5:Build()
local dup = inst5.x
--    ^ hover: (global) dup: number | nil

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
--    ^ hover: (global) obj: FieldClass {

local cb = inst6.callback
--    ^ hover: (global) cb: function

local arr = inst6.names
--    ^ hover: (global) arr: table

-- ── Direct chain without intermediate variable ──────────────────────

local directInst = Schema:AddString("key"):AddBool("flag"):Build()
local dk = directInst.key
--    ^ hover: (global) dk: string

local df = directInst.flag
--    ^ hover: (global) df: boolean

-- ── Malformed @builds-field diagnostics ─────────────────────────────

---@builds-field
-- ^ diag: malformed-annotation

---@builds-field abc string
-- ^ diag: malformed-annotation

---@builds-field 0 string
-- ^ diag: malformed-annotation

---@builds-field 1
-- ^ diag: malformed-annotation

-- Valid @builds-field — no diagnostic
---@builds-field 1 string
-- ^ diag: none

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
--    ^ hover: (global) tsLabel: ?

local tsCount = tsInst.count
--    ^ hover: (global) tsCount: ?

-- ── @return built : UndefinedClass ──────────────────────────────────

---@return built : FakeClass123
function Schema:BuildBadParent()
-- ^ diag: undefined-doc-class
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
--    ^ hover: (global) gItem: FieldClass {

local gExtra = gs.extra
--    ^ hover: (global) gExtra: FieldClass | nil

-- ── Generic @builds-field with string literal arg ────────────────────

local gs2 = GenSchema:AddTypedField("strItem", "FieldClass"):AddOptionalTypedField("strExtra", "FieldClass"):Finish()

local gsItem2 = gs2.strItem
--    ^ hover: (global) gsItem2: FieldClass {

local gsExtra2 = gs2.strExtra
--    ^ hover: (global) gsExtra2: FieldClass | nil

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
--    ^ hover: (global) myInst: MyBuiltType {

local myLabel = myInst.label
--    ^ hover: (global) myLabel: string

local myCount = myInst.count
--    ^ hover: (global) myCount: number

-- Use the built name in @param annotation
---@param state MyBuiltType
function useBuiltName(state)
    local x = state.label
    --    ^ hover: (local) x: string
end

-- ── @built-name malformed diagnostics ────────────────────────────────

---@built-name
-- ^ diag: malformed-annotation

---@built-name abc
-- ^ diag: malformed-annotation

---@built-name 0
-- ^ diag: malformed-annotation

-- Valid @built-name — no diagnostic
---@built-name 1
-- ^ diag: none

-- ── @built-extends: schema extension across class hierarchies ────

---@param name string
---@built-name 1
---@built-extends
-- ^ diag: none
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
--    ^ hover: (global) cLabel: string

local cCount = childInst.childCount
--    ^ hover: (global) cCount: number | nil

-- Inherited base fields via parent class
local cBase = childInst.baseName
--    ^ hover: (global) cBase: string

local cActive = childInst.baseActive
--    ^ hover: (global) cActive: boolean

-- Multi-level: grandchild extends child
local GRAND_SCHEMA = CHILD_SCHEMA:Extend("GrandState"):AddString("grandField")

local grandInst = GRAND_SCHEMA:Build()

-- Grandchild's own field
local gField = grandInst.grandField
--    ^ hover: (global) gField: string

-- Inherited from child
local gLabel = grandInst.childLabel
--    ^ hover: (global) gLabel: string

-- Inherited from base (through child)
local gBase = grandInst.baseName
--    ^ hover: (global) gBase: string

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
--    ^ hover: (global) ubtItem: UBTClass | nil

-- Class variable arg — resolved directly from the table type
local ubts2 = UnionBTSchema:AddOptionalClassField("item2", UBTClass):Commit()

local ubtItem2 = ubts2.item2
--    ^ hover: (global) ubtItem2: UBTClass | nil

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
--    ^ hover: (global) childDone: ChildState {

-- Child's own field
local cprop = childDone.childProp
--    ^ hover: (global) cprop: string

-- Inherited field
local bprop = childDone.baseProp
--    ^ hover: (global) bprop: string

-- Passing child type to function expecting parent type should NOT produce type-mismatch
---@param state BaseState
function acceptBaseState(state)
    local x = state.baseProp
    --    ^ hover: (local) x: string
end

acceptBaseState(childDone)
-- ^ diag: none
