---@diagnostic disable: unused-local
-- File that reads named globals created by CreateFrame/CreateFont/CreateFontFamily
-- in another file. These should NOT produce undefined-global.

-- Frame created via CreateFrame (local assignment) in creator.lua
MyAddonFrame:Show()
--           ^ hover: (method) function Frame:Show()

-- Button created via CreateFrame (bare statement) in creator.lua
MyAddonButton:Enable()
--            ^ hover: (method) function Button:Enable()

-- Frame created via CreateFrame (global assignment) in creator.lua
MyAddonPanel:Hide()
--           ^ hover: (method) function Frame:Hide()

-- Font created via CreateFont in creator.lua
local gameFont = MyAddonGameFont
--    ^ hover: (local) gameFont: Font

-- Font created via CreateFontFamily in creator.lua
local fontFamily = MyAddonFontFamily
--    ^ hover: (local) fontFamily: Font

-- Dynamic name was not detected, so it remains undefined
local bad = DynamicFrame
--          ^ diag: undefined-global
