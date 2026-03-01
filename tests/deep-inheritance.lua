-- Test: deep class inheritance
-- Hierarchy: Object -> ScriptRegion -> Region -> Frame -> Button

---@type Button
local btn = nil

-- Button's own method
local s1 = btn:GetText()

-- Frame method (parent)
local s2 = btn:GetScript("OnClick")

-- Region method (grandparent)
local a = btn:GetAlpha()

-- ScriptRegion method (great-grandparent)
local ok = btn:CanChangeProtectedState()

-- Object method (great-great-grandparent)
local name = btn:GetDebugName()

-- Also test @field inheritance
---@type Frame
local f = nil
local s3 = f:GetScript("OnClick")
local a2 = f:GetAlpha()
local ok2 = f:CanChangeProtectedState()
