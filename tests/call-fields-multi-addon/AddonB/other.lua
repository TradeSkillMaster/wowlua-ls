-- Another addon's namespace writes, which make each addon's table its own copy.

local _, ns = ...

ns.Other = CreateFrame("Frame")
ns.UI = {}
ns.UI.Label = "other"
