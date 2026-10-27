---@class StoreHub
local Hub = StoreHub

function Hub:Initialize()
    Hub.Database = LibStub("AceDB-3.0"):New("StoreHubDB", { global = { debugMode = false, version = 1 } }, "Default")
end
