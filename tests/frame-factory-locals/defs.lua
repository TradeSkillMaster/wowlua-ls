---@diagnostic disable: create-global, unused-local
-- Locals frame factories initialize, read from user.lua. The factory's first
-- argument names the frame type of what the call creates, not a class the local is.

local _, ns = ...

---@class FactoryRowTemplate
local FactoryRowTemplate = {}
function FactoryRowTemplate:SetRowData() end

-- A pool is the pool its call returns, not a `Button`.
local pool = CreateFramePool("Button", UIParent, "UIPanelButtonTemplate")
ns.Pool = pool

-- What is written through a frame is that frame's, not every `Frame`'s.
local plain = CreateFrame("Frame")
plain.myData = 1
function plain:OnMyEvent() end

-- A global assigned a frame is the frame its call creates.
local row = CreateFrame("Frame", nil, UIParent, "FactoryRowTemplate")
FactoryRowGlobal = row

-- A frame assigned to a field carries what is written through it there.
local panel = CreateFrame("Frame", nil, UIParent, "FactoryRowTemplate")
panel.title = "Options"
function panel:Refresh() end
ns.Panel = panel
local child = CreateFrame("Button", nil, panel)
child.count = 1
panel.Child = child

-- This file reads what it writes through a frame created from a template.
panel:Refresh()
--     ^ hover: (method) function Frame:Refresh()  def: local 27:1
local title = panel.title
--    ^ hover: (local) title: string
--                  ^ def: local 26:1

-- A field written through a field assigned a frame keeps the frame.
ns.Display = CreateFrame("Frame")
ns.Display.Speed = CreateFrame("Cooldown")

-- Children pointing back at the frame they belong to hold that frame.
local list = CreateFrame("Frame", nil, UIParent, "FactoryRowTemplate")
list.heading = "Items"
local first = CreateFrame("Button", nil, list)
first.owner = list
list.First = first
local second = CreateFrame("Button", nil, list)
second.owner = list
list.Second = second
ns.List = list

-- A frame assigned to a field of a global class carries what is written through it.
---@class FactoryGlobalClass
FactoryGlobalClass = {}
local labeled = CreateFrame("Frame")
labeled.label = "Name"
FactoryGlobalClass.Labeled = labeled

-- Overriding a template's member on one frame leaves other frames alone.
local overridden = CreateFrame("Frame", nil, UIParent, "FactoryRowTemplate")
overridden.SetRowData = function() end
local other = CreateFrame("Frame")
other:SetRowData()
--    ^ diag: undefined-field
