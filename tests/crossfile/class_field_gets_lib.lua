-- Cross-file class field type test: class with fields assigned from API calls

---@class CFGDisplay : Frame
local CFGDisplay = {}

-- Top-level field assignment from inherited method call
CFGDisplay.animGroup = CFGDisplay:CreateAnimationGroup()
CFGDisplay.texture = CFGDisplay:CreateTexture()

-- Top-level field assignment from global function with string arg
CFGDisplay.frame = CreateFrame("Frame")

-- Self-field assignment from inherited method call inside method body
---@class CFGWidget : Frame
local CFGWidget = {}

function CFGWidget:Init()
    self.anim = self:CreateAnimationGroup()
    self.tex = self:CreateTexture()
end
