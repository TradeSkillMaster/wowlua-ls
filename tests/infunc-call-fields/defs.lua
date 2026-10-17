---@diagnostic disable: unused-local, create-global
-- Fields written a call inside functions, read from user.lua: typed in other
-- files as the same writes at file level are.

local _, ns = ...

---@class InFuncRowTemplate
local InFuncRowTemplate = {}
function InFuncRowTemplate:SetRowData() end

---@class InFuncCacheMixin
local InFuncCacheMixin = {}
function InFuncCacheMixin:Lookup() end

function ns.Setup()
    -- Written directly, defaulted, through a local the call initializes, and
    -- under a namespace table.
    ns.Row = CreateFrame("Frame", nil, UIParent, "InFuncRowTemplate")
    ns.LazyRow = ns.LazyRow or CreateFrame("Frame", nil, UIParent, "InFuncRowTemplate")
    local panel = CreateFrame("Frame", nil, UIParent, "InFuncRowTemplate")
    ns.Panel = panel
    ns.UI = ns.UI or {}
    ns.UI.Button = CreateFrame("Button", nil, UIParent, "InFuncRowTemplate")

    -- What the function writes through a frame goes where the frame does.
    local frame = CreateFrame("Frame")
    frame.heading = "Options"
    function frame:Refresh() end
    ns.Options = frame

    -- A mixin applied to a frame afterwards is part of it.
    local cache = CreateFrame("Frame")
    Mixin(cache, InFuncCacheMixin)
    ns.Cache = cache
    ns.MixedRow = CreateFrame("Frame")
    Mixin(ns.MixedRow, InFuncCacheMixin)
end

-- `self` is the namespace.
function ns:SetupSelf()
    self.SelfRow = CreateFrame("Frame", nil, UIParent, "InFuncRowTemplate")
end

-- Another function's frame of the same name keeps its own members.
function ns.BuildStatus()
    local frame = CreateFrame("Frame")
    frame.label = "Status"
    ns.Status = frame
end

-- A field cleared in a function keeps its call's type, written in one or at
-- file level.
ns.TopRow = CreateFrame("Frame", nil, UIParent, "InFuncRowTemplate")
function ns.Teardown()
    ns.Row = nil
    ns.TopRow = nil
end

-- Writes that disagree on the subclass leave the declared class.
---@class InFuncOwner
---@field frame Frame
local InFuncOwner = {}
ns.Owner = InFuncOwner
function InFuncOwner:ShowButton() self.frame = CreateFrame("Button") end
function InFuncOwner:ShowSlider() self.frame = CreateFrame("Slider") end
function InFuncOwner:Label()
    self.frame:SetText("x")
    --         ^ diag: undefined-field
end

-- A typed constructor entry keeps its type when a function writes it a call the
-- scan can't type, as when it writes it another value.
InFuncEntries = { width = 0, label = "" }
function InFuncEntries:Init(parent)
    self.width = parent:GetParent():GetWidth()
    self.label = parent
end

-- What a workspace function returns, written directly or through a local.
function ns.MakeWidget() return { custom = 1 } end
function ns.SetupWidgets()
    ns.DirectWidget = ns.MakeWidget()
    local widget = ns.MakeWidget()
    ns.LocalWidget = widget
end

-- A callee rooted at a file local is not the same-named global's (globals.lua).
local InFuncUtil = {}
function InFuncUtil.Make() return { custom = 1 } end
ns.MadeTop = InFuncUtil.Make()
function ns.SetupMade()
    ns.Made = InFuncUtil.Make()
end

ns.Factory = {}
function ns.Factory:MakeRow() return CreateFrame("Frame", nil, UIParent, "InFuncRowTemplate") end

-- This file reads them the same way.
function ns.Use()
    local row = ns.Row
    --    ^ hover: (local) row: Frame & InFuncRowTemplate
    ns.Cache:Lookup()
    ns.MixedRow:Lookup()
end
