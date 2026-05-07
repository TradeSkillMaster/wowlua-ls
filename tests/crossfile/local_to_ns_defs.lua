-- Cross-file test: local variable assigned from function call, then assigned to namespace field
local addonName, ns = ...

-- Create a parent frame (CreateFrame returns a known type)
local parent = CreateFrame("Frame")

-- Pattern 1: local assigned from method call, then assigned to ns field
local surgeArc = parent:CreateTexture(nil, "OVERLAY", nil, 1)
ns.SurgeArc = surgeArc

-- Pattern 2: local assigned from global function call, then assigned to ns field
local textDisplay = CreateFrame("Frame", nil, parent)
ns.TextDisplay = textDisplay

-- Pattern 3: chained local — method call on a local that itself came from a call
local textBackground = textDisplay:CreateTexture(nil, "BACKGROUND")
ns.TextBackground = textBackground
