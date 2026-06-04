---@diagnostic disable: create-global, undefined-global, unused-local
-- Cross-file chain test: uses Include to get a class from another file,
-- then exercises method chains with @return self and resolves the final type.
-- Tests: auto-created class tables from pre_globals + external expr cycle detection.
local Component = DefineClass("ChainTestComponent")
local Schema = Component:Include("ChainSchema")
--     ^ hover: (local) Schema: ChainSchema {

-- Long method chain with repeated @return self calls.
-- This tests that external expr cycle detection doesn't break the chain.
local db = Schema:AddField("name"):AddNumberField("count"):AddField("label"):Commit()
--    ^ hover: (local) db: ChainSchemaResult {

-- Method on the result of the chain should resolve
db.Query()

-- Chain via From():Include() (3-part chain)
local Schema2 = Component:From("ChainTestComponent"):Include("ChainSchema")
--     ^ hover: (local) Schema2: ChainSchema {

-- Field initially nil, reassigned from a method chain (tests extra_exprs in field resolution)
---@class ChainPrivate
---@field myDB ChainSchemaResult
local private = {}
private.myDB = Schema:AddField("x"):AddNumberField("y"):Commit()

-- Hover on the reassigned field resolves through @field annotation
local r = private.myDB
--    ^ hover: (local) r: ChainSchemaResult {

-- Method hover on a field resolved via annotation (resolve_identifier_to_table path)
private.myDB:Query()
--           ^ hover: (method) function ChainSchemaResult:Query()

-- ── @builds-field builder pattern ────────────────────────────────────────

-- Builder chain accumulates fields, CreateInstance returns the built type
local inst = Schema:AddTypedString("label"):AddTypedNumber("count"):AddTypedBool("active"):CreateInstance()

local lbl = inst.label
--    ^ hover: (local) lbl: string

local cnt = inst.count
--    ^ hover: (local) cnt: number?

local act = inst.active
--    ^ hover: (local) act: boolean

-- @return built : Parent — built type inherits from ChainBuiltBase
local inst2 = Schema:AddTypedString("name"):CreateInstanceWithParent()

local nm = inst2.name
--    ^ hover: (local) nm: string

-- Inherited method from ChainBuiltBase
inst2:GetValue("x")

-- Non-literal field name: graceful degradation (no crash, treated as regular @return self)
local varName = "dynamic"
---@diagnostic enable: unused-local
local inst3 = Schema:AddTypedString(varName):CreateInstance()
--    ^ diag: unused-local  diag: undefined-field
---@diagnostic disable: unused-local

-- ── @built-extends type compatibility ──────────────────────────────────────

-- Create named base type via @built-name, then extend it
local BASE = Schema:Create("ChainBaseState"):AddTypedString("baseProp"):AddTypedBool("baseFlag")
local CHILD = BASE:Extend("ChainChildState"):AddTypedString("childProp")

local childInst = CHILD:CreateInstance()
--    ^ hover: (local) childInst: ChainChildState {

-- Child's own field
local cProp = childInst.childProp
--    ^ hover: (local) cProp: string

-- Inherited field via parent
local bProp = childInst.baseProp
--    ^ hover: (local) bProp: string

-- Passing child built type to function expecting parent type — should NOT warn
---@param state ChainBaseState
function acceptChainBase(state)
    local x = state.baseProp
    --    ^ hover: (local) x: string
end

acceptChainBase(childInst)

-- ── Opaque reference chain via addon namespace ──────────────────────────

-- ChainOpaqueApp comes from the addon namespace. From() returns concrete
-- ChainComponentRef, then Include() on that type resolves via generic
-- @return T with backtick param binding to the target class.
local ChainOpaqueApp = select(2, ...).ChainOpaqueApp
local Svc = ChainOpaqueApp:From("ChainOpaqueApp"):Include("ChainOpaqueSvc")
--    ^ hover: (local) Svc: ChainOpaqueSvc {

-- Negative: a plain table without :From() should NOT magically resolve
-- just because the string arg matches a class name.
local PlainTbl = {}
local Schema4 = PlainTbl:From("ChainTestComponent"):Include("ChainSchema")
--     ^ hover: (local) Schema4: ?
