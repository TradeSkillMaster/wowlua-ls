---@diagnostic disable: create-global, unused-local
-- Fields written a call, read from user.lua and class_user.lua in other files.

local _, ns = ...

---@class CallFieldRowTemplate
local CallFieldRowTemplate = {}
function CallFieldRowTemplate:SetRowData() end

---@class CallFieldOtherTemplate
local CallFieldOtherTemplate = {}
function CallFieldOtherTemplate:SetOtherData() end

-- A frame created from a template keeps the template's members: written directly,
-- through a local, as a nested field, or on a global table.
ns.Row = CreateFrame("Frame", nil, UIParent, "CallFieldRowTemplate")
local panel = CreateFrame("Frame", nil, UIParent, "CallFieldRowTemplate")
ns.Panel = panel
ns.UI = {}
ns.UI.Button = CreateFrame("Button", nil, UIParent, "CallFieldRowTemplate")
CallFieldWidgets = {}
CallFieldWidgets.Row = CreateFrame("Frame", nil, UIParent, "CallFieldRowTemplate")
ns.Backdrop = CreateFrame("Frame", nil, UIParent, "BackdropTemplate")

-- A field methods are defined through keeps them along with the template's.
ns.Dialog = CreateFrame("Frame", nil, UIParent, "CallFieldRowTemplate")
function ns.Dialog:OnShown() end
ns.Dialog:SetRowData()

-- Without a template, a frame is its frame type.
ns.Plain = CreateFrame("Frame")

-- Calls the edit tests shift the ones above onto.
ns.Other = CreateFrame("Frame", nil, UIParent, "CallFieldOtherTemplate")
local other = CreateFrame("Frame", nil, UIParent, "CallFieldOtherTemplate")
ns.OtherPanel = other

-- Other files read a field written several values as its calls' type only when it
-- is every value's: not a reassigned local's initializer, nor calls that disagree,
-- nor a call next to another value. Calls that agree (`ns.Same`, in defs_more.lua
-- too) keep it.
local reassigned = CreateFrame("Frame", nil, UIParent, "CallFieldRowTemplate")
reassigned = CreateFrame("Frame", nil, UIParent, "CallFieldOtherTemplate")
ns.Reassigned = reassigned
if IsShiftKeyDown() then
    ns.Either = CreateFrame("Frame", nil, UIParent, "CallFieldRowTemplate")
else
    ns.Either = CreateFrame("Frame", nil, UIParent, "CallFieldOtherTemplate")
end
ns.Mixed = CreateFrame("Frame", nil, UIParent, "CallFieldRowTemplate")
ns.Mixed = UIParent
ns.Same = CreateFrame("Frame", nil, UIParent, "CallFieldRowTemplate")

-- Methods defined through a field written a call belong to the object the call
-- returns, which keeps its members, here as in the other files: on a table of
-- this file or a local one, and through a local the call initializes.
---@class CallFieldFormatter
CallFieldFormatter = {}
---@return string
function CallFieldFormatter:Format() return "" end

CallFieldWidgets.Formatter = CreateFromMixins(CallFieldFormatter)
---@return number
function CallFieldWidgets.Formatter:GetUnits() return 1 end
local units = CallFieldWidgets.Formatter:GetUnits()
--    ^ hover: (local) units: number
local formatted = CallFieldWidgets.Formatter:Format()
--    ^ hover: (local) formatted: string
CallFieldWidgets.Formatter:GetSpan()
--                         ^ diag: undefined-field
local panels = {}
panels.Frame = CreateFrame("Frame")
function panels.Frame:OnShown() end
panels.Frame:OnShown()
panels.Frame:Show()
local sharedFormatter = CreateFromMixins(CallFieldFormatter)
CallFieldWidgets.Shared = sharedFormatter
function CallFieldWidgets.Shared:GetUnits() return 1 end
sharedFormatter:GetUnits()

-- The defining file reads its own writes.
local ownRow = ns.Row
--    ^ hover: (local) ownRow: Frame & CallFieldRowTemplate
