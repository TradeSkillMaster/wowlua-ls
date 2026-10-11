-- Fields written a call in an `addonRoot` addon, read from user.lua.

local _, ns = ...

---@class MultiAddonRowTemplate
local MultiAddonRowTemplate = {}
function MultiAddonRowTemplate:SetRowData() end

ns.Row = CreateFrame("Frame", nil, UIParent, "MultiAddonRowTemplate")
local panel = CreateFrame("Frame", nil, UIParent, "MultiAddonRowTemplate")
ns.Panel = panel
ns.UI = {}
ns.UI.Button = CreateFrame("Button", nil, UIParent, "MultiAddonRowTemplate")
ns.Dialog = CreateFrame("Frame", nil, UIParent, "MultiAddonRowTemplate")
function ns.Dialog:OnShown() end
