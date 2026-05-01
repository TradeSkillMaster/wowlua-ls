-- Cross-file test: methods defined inside do...end blocks
local addonName, ns = ...

---@class DoBlockClass
local DBC = {}
ns.DBC = DBC

---@return string
function DBC:TopLevel()
    return "top"
end

do
    ---@return number
    function DBC:InsideDo()
        return 42
    end
end

do
    do
        ---@return boolean
        function DBC:NestedDo()
            return true
        end
    end
end

-- Field assignment inside do...end should also be visible cross-file
do
    ---@type string
    DBC.StaticField = "hello"
end
