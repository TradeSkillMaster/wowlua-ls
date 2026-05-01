-- Cross-file overlay test: fields added to CreateFrame result stored in class field
-- Requires: --with-stubs

---@class FrameOverlayHost
local Host = {}

-- No @type annotation needed — CreateFrame's first string arg identifies the class
local textDisplay = CreateFrame("Frame")
textDisplay.customField = 42
textDisplay.handler = function(self) end
textDisplay.Text = textDisplay:CreateFontString(nil, "OVERLAY")

Host.display = textDisplay

-- @type annotation path (for locals initialized from non-pattern-matched sources)
---@class TypeAnnotatedHost
local TypeHost = {}

---@type Frame
local typedFrame = nil
typedFrame.typedField = "hello"

TypeHost.frame = typedFrame
