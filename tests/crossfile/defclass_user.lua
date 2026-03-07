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
