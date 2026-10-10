-- Another file reaches the methods and fields defs.lua defines through its
-- alias, through its own alias and through the namespace.
local _, ns = ...
local AliasPanel = ns.AliasPanel

function AliasPanel:Close()
	self:Refresh()
	--   ^ hover: (method) function AliasPanel:Refresh()
	self.window:Hide()
	print(self.colors, AliasPanel.colors)
	--         ^ hover: (field) colors: table
end

ns.AliasPanel:Open()
--            ^ hover: (method) function AliasPanel:Open()
ns.AliasPanel:Close()

ns.ElemPanel:Open()
--           ^ hover: (method) function ElemPanel:Open()
