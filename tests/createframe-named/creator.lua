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
