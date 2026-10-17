---@diagnostic disable: unused-local
-- Cross-file reads of the fields defs.lua writes inside functions.

local _, ns = ...

local row = ns.Row
--    ^ hover: (local) row: Frame & InFuncRowTemplate
local lazyRow = ns.LazyRow
--    ^ hover: (local) lazyRow: Frame & InFuncRowTemplate
local panel = ns.Panel
--    ^ hover: (local) panel: Frame & InFuncRowTemplate
ns.Panel:SetRowData()
--       ^ hover: (method) function InFuncRowTemplate:SetRowData()
local button = ns.UI.Button
--    ^ hover: (local) button: Button & InFuncRowTemplate
local selfRow = ns.SelfRow
--    ^ hover: (local) selfRow: Frame & InFuncRowTemplate
local topRow = ns.TopRow
--    ^ hover: (local) topRow: Frame & InFuncRowTemplate

ns.Options:Refresh()
--         ^ hover: (method) function Frame:Refresh()
local heading = ns.Options.heading
local optionsLabel = ns.Options.label
--                              ^ diag: undefined-field
local statusLabel = ns.Status.label

ns.Cache:Lookup()
--       ^ hover: (method) function InFuncCacheMixin:Lookup()
ns.MixedRow:Lookup()

ns.Owner.frame:SetText("x")
--             ^ diag: undefined-field
local width = InFuncEntries.width
--    ^ hover: (local) width: number
local label = InFuncEntries.label
--    ^ hover: (local) label: string
local directWidget = ns.DirectWidget
--    ^ hover: (local) directWidget: { custom: number }
local localWidget = ns.LocalWidget
--    ^ hover: (local) localWidget: { custom: number }
local made = ns.Made
--    ^ hover: (local) made: any
local madeTop = ns.MadeTop
--    ^ hover: (local) madeTop: any
local startTime = ns.StartTime
--    ^ hover: (local) startTime: number
local factoryRow = ns.FactoryRow
--    ^ hover: (local) factoryRow: Frame & InFuncRowTemplate
