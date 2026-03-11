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
