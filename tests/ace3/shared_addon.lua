-- Declares the addon `callbacks.lua` fetches with `GetAddon`, so it is an
-- external (workspace) class there.
---@class SharedTimerAddon : AceAddon, AceTimer-3.0
local SharedTimerAddon = LibStub("AceAddon-3.0"):NewAddon("SharedTimerAddon", "AceTimer-3.0")
