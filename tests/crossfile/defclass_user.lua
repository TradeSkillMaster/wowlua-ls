-- Cross-file defclass test: uses the defclass-created class via DefineClass return type
local x = DefineClass("MyComp")
--    ^ hover: x: MyComp

local y = DefineClass("MyComp"):AddDep("a"):AddDep("b")
--    ^ hover: y: MyComp

-- Call un-annotated method: should not produce redundant-parameter
local z = DefineClass("MyComp"):SetFlag("k", "v")
--    ^ hover: z: MyComp  diag: unused-local

-- Method after an unrelated @class must still resolve to MyComp (not UnrelatedInfo)
local n = DefineClass("MyComp"):GetName("test")
--    ^ hover: n: string  diag: unused-local

-- Query-level: hovering on chained method names must resolve (not just the variable)
DefineClass("MyComp"):AddDep("x")
--                    ^ hover: AddDep: fun(name: string)  def: external

-- Query-level: hover on 2nd method in @return self chain
DefineClass("MyComp"):AddDep("a"):AddDep("b")
--                                ^ hover: AddDep: fun(name: string)  def: external

-- Cross-file method access on defclass instance must not produce undefined-field
local comp = DefineClass("MyComp")
comp:AddDep("test")
-- ^ diag: none
comp.Create("x")
-- ^ diag: none
