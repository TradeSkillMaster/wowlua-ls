-- Cross-file defclass test: non-local assignment with chained method calls
local addonName, ns = ...

ns.MyService = DefineClass("MyService"):AddDep("a"):AddDep("b")

local svc = ns.MyService
--    ^ hover: svc: MyService

-- TODO: Method access on addon-namespace defclass field doesn't resolve yet
-- (ns.MyService resolves to MyService but method calls on dotted addon fields
-- aren't handled in the query-level field chain resolver)
-- local name = ns.MyService:GetName("test")
-- --    ^ hover: name: string  diag: unused-local
