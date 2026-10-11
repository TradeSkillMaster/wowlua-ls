---@diagnostic disable: unused-local
-- Cross-file reads through a namespace this file declares a class, whose table
-- holds its own copies of the namespace's fields.

---@class CallFieldNS
local ns = select(2, ...)

local row = ns.Row
--    ^ hover: (local) row: Frame & CallFieldRowTemplate
ns.Panel:SetRowData()
ns.Dialog:SetRowData()
ns.Dialog:OnShown()
--        ^ hover: (method) function Frame:OnShown()
