---@diagnostic disable: unused-local
---@class LedgerCore
local Core = LedgerCore

local revision = Core.revision
--    ^ hover: (local) revision: number?

---@param core LedgerCore
local function Describe(core)
    -- A read only a condition makes, through the narrowed receiver.
    if core and core.revision then
        --           ^ hover: (field) revision: number?
        return true
    end
end
