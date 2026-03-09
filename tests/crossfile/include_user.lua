-- Cross-file include test: uses :Include to get a class defined in another file
local Component = DefineClass("IncludeTestComponent")
local Svc = Component:Include("IncTestService")
--    ^ hover: (global) Svc: IncTestService {

-- Method defined in include_component.lua should resolve
Svc:GetCount()
-- ^ diag: none

-- Field assigned in include_component.lua should not produce undefined-field
local s = Svc.STATUS
--    ^ diag: unused-local
