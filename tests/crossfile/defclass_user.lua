-- Cross-file defclass test: uses the defclass-created class via DefineClass return type
local x = DefineClass("MyComp")
--    ^ hover: x: MyComp
local y = DefineClass("MyComp"):AddDep("a"):AddDep("b")
--    ^ hover: y: MyComp
