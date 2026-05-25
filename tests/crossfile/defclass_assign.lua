-- Cross-file defclass test: non-local assignment with chained method calls
-- NOTE: undefined-field on AddDep is a true positive — MyService inherits from
-- ObjBase (generic constraint), not MyComp (which defines AddDep).
local addonName, ns = ...

---@diagnostic disable-next-line: undefined-field
ns.MyService = DefineClass("MyService"):AddDep("a"):AddDep("b")

local svc = ns.MyService
--    ^ hover: (local) svc: MyService

-- Method call on addon namespace field where class has the method
ns.Lib:GetName()
--     ^ hover: (method) function MyLib:GetName()  def: external
