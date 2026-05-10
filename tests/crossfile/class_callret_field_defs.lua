-- Cross-file test: fields on @class assigned from locals initialized by method calls
-- Requires: --with-stubs

---@class WidgetPanel : Frame
local Panel = CreateFrame("Frame", "TestPanel", UIParent)

-- Local assigned from a method call, then stored as a class field
local bgTexture = Panel:CreateTexture(nil, "BACKGROUND")
Panel.Background = bgTexture

local overlayTexture = Panel:CreateTexture(nil, "OVERLAY")
Panel.Flash = overlayTexture

-- Direct method call result (already works without this fix)
Panel.DirectTexture = Panel:CreateTexture(nil, "ARTWORK")
