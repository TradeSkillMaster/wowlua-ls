-- Cross-file chain test: uses Include to get a class from another file,
-- then exercises method chains with @return self and resolves the final type.
-- Tests: auto-created class tables from pre_globals + external expr cycle detection.
local Component = DefineClass("ChainTestComponent")
local Schema = Component:Include("ChainSchema")
--     ^ hover: (global) Schema: ChainSchema {

-- Long method chain with repeated @return self calls.
-- This tests that external expr cycle detection doesn't break the chain.
local db = Schema:AddField("name"):AddNumberField("count"):AddField("label"):Commit()
--    ^ hover: (global) db: ChainSchemaResult {

-- Method on the result of the chain should resolve
db.Query()
-- ^ diag: none

-- Chain via From():Include() (3-part chain)
local Schema2 = Component:From("ChainTestComponent"):Include("ChainSchema")
--     ^ hover: (global) Schema2: ChainSchema {  diag: unused-local

-- Field initially nil, reassigned from a method chain (tests extra_exprs in field resolution)
---@class ChainPrivate
---@field myDB ChainSchemaResult
local private = {}
private.myDB = Schema:AddField("x"):AddNumberField("y"):Commit()

-- Hover on the reassigned field resolves through @field annotation
local r = private.myDB
--    ^ hover: (global) r: ChainSchemaResult {  diag: unused-local

-- Method hover on a field resolved via annotation (resolve_identifier_to_table path)
private.myDB:Query()
--           ^ hover: (method) function ChainSchemaResult:Query()  diag: none

-- ── @builds-field builder pattern ────────────────────────────────────────

-- Builder chain accumulates fields, CreateInstance returns the built type
local inst = Schema:AddTypedString("label"):AddTypedNumber("count"):AddTypedBool("active"):CreateInstance()

local lbl = inst.label
--    ^ hover: (global) lbl: string

local cnt = inst.count
--    ^ hover: (global) cnt: number | nil

local act = inst.active
--    ^ hover: (global) act: boolean

-- @return built : Parent — built type inherits from ChainBuiltBase
local inst2 = Schema:AddTypedString("name"):CreateInstanceWithParent()

local nm = inst2.name
--    ^ hover: (global) nm: string

-- Inherited method from ChainBuiltBase
inst2:GetValue("x")
-- ^ diag: none

-- Non-literal field name: graceful degradation (no crash, treated as regular @return self)
local varName = "dynamic"
local inst3 = Schema:AddTypedString(varName):CreateInstance()
--    ^ diag: unused-local
