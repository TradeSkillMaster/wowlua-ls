---@diagnostic disable: unused-local, unused-function
-- Redeclaring an alias with literal `---|` members extends the existing alias
-- (a stub's or another file's) instead of replacing it. The spaced `--- |`
-- continuation form is a member line like `---|`.

--- @alias AceGUIWidgetType
--- | "ClickedFrame"

---@alias XaMode
---| "a"

---@alias XaUnit string
---| "x"

---@alias (opaque) XoUnit string
---| "x"

-- The declaring file sees the stub's members along with its own.
local AceGUI = LibStub("AceGUI-3.0")
local w = AceGUI:Create("ClickedFrame")
--                       ^ comp: BlizOptionsGroup, Button, CheckBox, ClickedFrame, ColorPicker, Dropdown, DropdownGroup, EditBox, Frame, Heading, Icon, InlineGroup, InteractiveLabel, Keybinding, Label, MultiLineEditBox, OtherFrame, ScrollFrame, SimpleGroup, Slider, TabGroup, TreeGroup, Window
