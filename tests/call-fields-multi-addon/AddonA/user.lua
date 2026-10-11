---@diagnostic disable: unused-local
-- Each addon's namespace table holds its own copies of the fields defs.lua writes.

local _, ns = ...

local row = ns.Row
--    ^ hover: (local) row: Frame & MultiAddonRowTemplate
ns.Row:SetRowData()
local panel = ns.Panel
--    ^ hover: (local) panel: Frame & MultiAddonRowTemplate
local button = ns.UI.Button
--    ^ hover: (local) button: Button & MultiAddonRowTemplate
local dialog = ns.Dialog
--    ^ hover: (local) dialog: Frame & MultiAddonRowTemplate
ns.Dialog:OnShown()
--        ^ hover: (method) function Frame:OnShown()
