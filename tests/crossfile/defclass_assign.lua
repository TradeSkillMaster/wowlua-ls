-- Cross-file defclass test: non-local assignment with chained method calls
local addonName, ns = ...

ns.MyService = DefineClass("MyService"):AddDep("a"):AddDep("b")

local svc = ns.MyService
--    ^ hover: svc: MyService

-- Method call on addon namespace field where class has the method
ns.Lib:GetName()
--     ^ hover: GetName: fun()  def: external
