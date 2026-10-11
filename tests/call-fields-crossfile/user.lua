---@diagnostic disable: unused-local
-- Cross-file reads of the fields defs.lua writes.

local _, ns = ...

local row = ns.Row
--    ^ hover: (local) row: Frame & CallFieldRowTemplate
ns.Row:SetRowData()
--     ^ hover: (method) function CallFieldRowTemplate:SetRowData()
local panel = ns.Panel
--    ^ hover: (local) panel: Frame & CallFieldRowTemplate
ns.Panel:SetRowData()
local button = ns.UI.Button
--    ^ hover: (local) button: Button & CallFieldRowTemplate
CallFieldWidgets.Row:SetRowData()
ns.Backdrop:SetBackdrop(nil)
local dialog = ns.Dialog
--    ^ hover: (local) dialog: Frame & CallFieldRowTemplate
ns.Dialog:SetRowData()
ns.Dialog:OnShown()
--        ^ hover: (method) function Frame:OnShown()

local plain = ns.Plain
--    ^ hover: (local) plain: Frame {
ns.Plain:SetRowData()
--       ^ diag: undefined-field
local reassigned = ns.Reassigned
--    ^ hover: (local) reassigned: Frame {
local either = ns.Either
--    ^ hover: (local) either: Frame {
local mixed = ns.Mixed
--    ^ hover: (local) mixed: Frame {
local same = ns.Same
--    ^ hover: (local) same: Frame & CallFieldRowTemplate
