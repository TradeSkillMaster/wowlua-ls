-- Cross-file defclass test: uses the defclass-created class via DefineClass return type
local x = DefineClass("MyComp")
--    ^ hover: (global) x: MyComp {

local y = DefineClass("MyComp"):AddDep("a"):AddDep("b")
--    ^ hover: (global) y: MyComp {

-- Call un-annotated method: should not produce redundant-parameter
local z = DefineClass("MyComp"):SetFlag("k", "v")
--    ^ hover: (global) z: MyComp {  diag: unused-local

-- Method after an unrelated @class must still resolve to MyComp (not UnrelatedInfo)
local n = DefineClass("MyComp"):GetName("test")
--    ^ hover: (global) n: string  diag: unused-local

-- Query-level: hovering on chained method names must resolve (not just the variable)
DefineClass("MyComp"):AddDep("x")
--                    ^ hover: (method) function MyComp:AddDep(name: string)  def: external

-- Query-level: hover on 2nd method in @return self chain
DefineClass("MyComp"):AddDep("a"):AddDep("b")
--                                ^ hover: (method) function MyComp:AddDep(name: string)  def: external

-- Cross-file method access on defclass instance must not produce undefined-field
local comp = DefineClass("MyComp")
comp:AddDep("test")
-- ^ diag: none
comp.Create("x")
-- ^ diag: none

-- Constructor fields set in __init must be visible cross-file with inferred types
local cs = comp._state
--              ^ hover: (field) _state: string  diag: unused-local
local cc = comp._count
--              ^ hover: (field) _count: number  diag: unused-local
local ci = comp._items
--              ^ hover: (field) _items: table  diag: unused-local
local ca = comp._active
--              ^ hover: (field) _active: boolean  diag: unused-local
local cn = comp._info
--              ^ hover: (field) _info: UnrelatedInfo  diag: unused-local
local cm = comp._made
--              ^ hover: (field) _made: UnrelatedInfo  diag: unused-local
local cb = comp._built
--              ^ hover: (field) _built: SchemaState  diag: unused-local

-- Cross-file static field assignment (class-level, not constructor)
local cs2 = comp._SCHEMA
--               ^ hover: (field) _SCHEMA: Schema  diag: unused-local
