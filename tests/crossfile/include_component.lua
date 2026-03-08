-- Cross-file include test: defines a class via :Init and adds a field via assignment
local Component = DefineClass("IncludeTestComponent")
local Svc = Component:Init("IncTestService")

---@return number
function Svc:GetCount()
    return 0
end

-- Dot-call with string arg: local var should NOT get class type
local MyEnum = Component.NewEnum("SOME_ENUM_NAME")
--     ^ hover: MyEnum: table

-- Assign a field on the class (like AuctioningOperation.RESULT = RESULT)
Svc.STATUS = MyEnum
