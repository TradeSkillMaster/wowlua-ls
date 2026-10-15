---@diagnostic disable: unused-local
-- Cross-file reads of the frame-factory locals defs.lua writes.

local _, ns = ...

local item = ns.Pool:Acquire()
local pool = ns.Pool
--    ^ hover: (local) pool: ObjectPool {

local data = CreateFrame("Frame").myData
--                                ^ diag: undefined-field
CreateFrame("Frame"):OnMyEvent()
--                   ^ diag: undefined-field
local typed = CreateFrame("Frame").myD
--                                 ^ diag: undefined-field
--                                    ^ comp: none

local row = FactoryRowGlobal
--    ^ hover: (local) row: Frame & FactoryRowTemplate
FactoryRowGlobal:SetRowData()

ns.Panel:Refresh()
--       ^ hover: (method) function Frame:Refresh()  def: external
local title = ns.Panel.title
--    ^ hover: (local) title: string
ns.Panel:SetRowData()
local count = ns.Panel.Child.count
--    ^ hover: (local) count: number

local display = ns.Display
--    ^ hover: (local) display: Frame {
local speed = ns.Display.Speed
--    ^ hover: (local) speed: Cooldown {

local heading = ns.List.First.owner.heading
--    ^ hover: (local) heading: string
ns.List.Second.owner:Show()
local labelText = FactoryGlobalClass.Labeled.label
--    ^ hover: (local) labelText: string
