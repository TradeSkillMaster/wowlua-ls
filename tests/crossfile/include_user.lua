-- Cross-file include test: uses :Include to get a class defined in another file
local Component = DefineClass("IncludeTestComponent")
local Svc = Component:Include("IncTestService")
--    ^ hover: (local) Svc: IncTestService {

-- Method defined in include_component.lua should resolve
Svc:GetCount()

-- Field assigned in include_component.lua should not produce undefined-field
local s = Svc.STATUS
--    ^ diag: unused-local
