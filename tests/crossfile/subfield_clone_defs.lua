-- Cross-file test: sub-field assignments on shared class tables
-- When Obj.SubField = CreateFrame("Frame") and then Obj.SubField.Custom = x,
-- the shared Frame class must be cloned so Custom is visible without undefined-field.
-- Requires: --with-stubs

---@class SubFieldHost : Frame
local Host = {}

local speedDisplay = CreateFrame("Frame")
Host.SpeedDisplay = speedDisplay

local speedCooldown = CreateFrame("Cooldown", Host.SpeedDisplay)
Host.SpeedDisplay.Speed = speedCooldown

local textDisplay = CreateFrame("Frame")
Host.TextDisplay = textDisplay

local textFrame = CreateFrame("Frame")
Host.TextDisplay.Text = textFrame
