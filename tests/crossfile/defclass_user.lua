---@diagnostic disable: unused-local
-- Cross-file defclass test: uses the defclass-created class via DefineClass return type
local x = DefineClass("MyComp")
--    ^ hover: (local) x: MyComp {

local y = DefineClass("MyComp"):AddDep("a"):AddDep("b")
--    ^ hover: (local) y: MyComp {

-- Call un-annotated method: should not produce redundant-parameter
local z = DefineClass("MyComp"):SetFlag("k", "v")
--    ^ hover: (local) z: MyComp {

-- Method after an unrelated @class must still resolve to MyComp (not UnrelatedInfo)
local n = DefineClass("MyComp"):GetName("test")
--    ^ hover: (local) n: string

-- Query-level: hovering on chained method names must resolve (not just the variable)
DefineClass("MyComp"):AddDep("x")
--                    ^ hover: (method) function MyComp:AddDep(name: string)  def: external

-- Query-level: hover on 2nd method in @return self chain
DefineClass("MyComp"):AddDep("a"):AddDep("b")
--                                ^ hover: (method) function MyComp:AddDep(name: string)  def: external

-- Cross-file method access on defclass instance must not produce undefined-field
local comp = DefineClass("MyComp")
comp:AddDep("test")
comp.Create("x")

-- Dotted defclass field assignment: class-level enum field must be accessible
local cst = comp.COMP_STATUS
--               ^ hover: (field) COMP_STATUS: COMP_STATUS

-- Constructor fields set in __init must be visible cross-file with inferred types
local cs = comp._state
--              ^ hover: (field) _state: string
local cc = comp._count
--              ^ hover: (field) _count: number
local ci = comp._items
--              ^ hover: (field) _items: table
local ca = comp._active
--              ^ hover: (field) _active: boolean
local cn = comp._info
--              ^ hover: (field) _info: UnrelatedInfo
local cm = comp._made
--              ^ hover: (field) _made: UnrelatedInfo
local cb = comp._built
--              ^ hover: (field) _built: SchemaState
-- Inline ---@type annotations should be captured cross-file
local cg = comp._config
--              ^ hover: (field) _config: SchemaState
local cq = comp._query
--              ^ hover: (field) _query: UnrelatedInfo!

-- Cross-file static field assignment (class-level, not constructor)
local cs2 = comp._SCHEMA
--               ^ hover: (field) _SCHEMA: Schema

-- Regression: field set from a non-class local must not have a phantom class type.
-- Before fix, the scanner created a phantom empty class "localHelper" from the
-- field assignment `localHelper.flag = true`, causing undefined-field false positives.
-- The return type of MakeInfo() is now propagated through the local variable.
local ch = comp._helper
--              ^ hover: (field) _helper: UnrelatedInfo {

-- Constructor call: calling class table as function returns class instance
local MyComp2 = DefineClass("MyComp")
local inst = MyComp2()
--    ^ hover: (local) inst: MyComp {

-- Go-to-definition on defclass class name in annotation
---@type MyComp
--       ^ hover: (class) MyComp  def: external
local _comp_typed

-- Constructor call + chained method returns correct type
local chained = MyComp2():AddDep("test")
--    ^ hover: (local) chained: MyComp {

-- Hover on chained method name after constructor call
MyComp2():AddDep("x")
--        ^ hover: (method) function MyComp:AddDep(name: string)  def: external

-- Multi-chain after constructor
MyComp2():AddDep("a"):AddDep("b")
--                     ^ hover: (method) function MyComp:AddDep(name: string)  def: external

-- Regression: __init on __private sub-table with lateinit fields.
-- Reset method directly on class must not shadow the annotated field types
-- with nil (overlay must inherit external field annotations).
local obj = DefineClass("MyObj")
local od = obj._data
--              ^ hover: (field) _data: table<string, number>!
local ol = obj._label
--              ^ hover: (field) _label: string!
local og = obj:GetData()
--    ^ hover: (local) og: table<string, number>

-- Regression: field assigned from a local variable (unresolvable type at scan time)
-- must still be accessible cross-file (no undefined-field false positive).
local lc = comp.LOCAL_CONFIG
