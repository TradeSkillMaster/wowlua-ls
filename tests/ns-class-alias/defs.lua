-- A `@class` declared on an addon-namespace field and defined through a local
-- alias of it. Methods and fields written through the alias, and a `self` field
-- one method writes, resolve in the file's other methods and closures.
local _, ns = ...

---@class AliasPanelTemplate : Frame
---@field child Frame

---@class AliasPanel
ns.AliasPanel = {}
local AliasPanel = ns.AliasPanel

AliasPanel.colors = {}

function AliasPanel:Open()
	self.window = CreateFrame("Frame", nil, UIParent, "AliasPanelTemplate")
	self:Refresh()
	--   ^ hover: (method) function AliasPanel:Refresh()
	C_Timer.After(1, function()
		AliasPanel:Refresh()
		--         ^ hover: (method) function AliasPanel:Refresh()
	end)
end

function AliasPanel:Refresh()
	self.window.child:Show()
	--          ^ hover: (field) child: Frame
	print(self.colors, AliasPanel.colors)
	--         ^ hover: (field) colors: table
end

-- The same through a plain local table.
local Module = {}
---@class AliasModule
Module.Inner = {}
local Inner = Module.Inner

function Inner:Build()
	self.frame = CreateFrame("Frame", nil, UIParent, "AliasPanelTemplate")
end

function Inner:Use()
	self.frame.child:Show()
	--         ^ hover: (field) child: Frame
end

-- An element write through the alias doesn't rebind it.
---@class ElemPanel
ns.ElemPanel = {}
local ElemPanel = ns.ElemPanel
ElemPanel[1] = true
function ElemPanel:Open() end
