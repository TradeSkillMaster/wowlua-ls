---@diagnostic disable: unused-local
-- A table other files fill, written a call's bare `table` and reset to `nil`
-- here: the workspace's table stands.
local _, ns = ...
ns.Panel = LedgerCopy({})
local function ResetPanel()
    ns.Panel = nil
end
local width = ns.Panel.width
--    ^ hover: (local) width: number
