---@diagnostic disable: unused-local, unused-function

local AceGUI = LibStub("AceGUI-3.0")
local w = AceGUI:Create("ClickedFrame")
--                       ^ comp: BlizOptionsGroup, Button, CheckBox, ClickedFrame, ColorPicker, Dropdown, DropdownGroup, EditBox, Frame, Heading, Icon, InlineGroup, InteractiveLabel, Keybinding, Label, MultiLineEditBox, OtherFrame, ScrollFrame, SimpleGroup, Slider, TabGroup, TreeGroup, Window

-- A type no overload names gets the base signature's return, not every overload's.
local custom = AceGUI:Create("ClickedFrame")
--    ^ hover: (local) custom: AceGUIWidget
local button = AceGUI:Create("Button")
--    ^ hover: (local) button: AceGUIButton
local bogus = AceGUI:Create("Bogus")
--                          ^ diag: type-mismatch

---@param t AceGUIWidgetType
local function widget(t) end
widget("ClickedFrame")
widget("OtherFrame")
widget("Button")
widget("Bogus")
--     ^ diag: type-mismatch

-- `replace.lua` replaces the stub's `ItemInfo`; its stub parameters keep their type.
local exists = C_Item.DoesItemExistByID(12345)
