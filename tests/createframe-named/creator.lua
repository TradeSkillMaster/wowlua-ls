---@diagnostic disable: unused-local
-- File that creates named frames via CreateFrame/CreateFont/CreateFontFamily.
-- The string-literal name arguments create implicit globals.

-- CreateFrame in a local assignment
local frame = CreateFrame("Frame", "MyAddonFrame", UIParent)

-- CreateFrame as a bare statement (no assignment)
CreateFrame("Button", "MyAddonButton", UIParent)

-- CreateFrame in a global assignment where LHS matches the name
MyAddonPanel = CreateFrame("Frame", "MyAddonPanel")
-- ^ diag: create-global

-- CreateFont with a string-literal name
CreateFont("MyAddonGameFont")

-- CreateFontFamily with a string-literal name
CreateFontFamily("MyAddonFontFamily", {})

-- Dynamic names are NOT detected (out of scope)
local name = "DynamicFrame"
CreateFrame("Frame", name, UIParent)

-- CreateFrame inside a function body (not just top-level statements) is detected.
local function InitNestedWidgets()
    CreateFrame("Frame", "MyAddonNestedFrame", UIParent)
end

-- CreateFrame nested as a call argument is detected.
local function wrap(x) return x end
wrap(CreateFrame("Button", "MyAddonWrappedButton"))

-- A virtual template (class) and a frame created from it. The created global's
-- type is harvested from the call's resolved return — CreateFrame's template
-- overload yields `Frame & MyAddonRowTemplate`, so the mixin survives cross-file.
---@class MyAddonRowTemplate
local MyAddonRowTemplate = {}
---@param self MyAddonRowTemplate
function MyAddonRowTemplate:SetRowData() end

CreateFrame("Frame", "MyAddonTemplatedFrame", UIParent, "MyAddonRowTemplate")
