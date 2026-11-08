---@diagnostic disable: unused-local
local private = select(2, ...) ---@class SlotCtorNS
local function update(mapID)
    local scanner = private.Data.Scanner
    scanner.mapID = mapID
end
local mapID = private.Data.Scanner.mapID
--    ^ hover: (local) mapID: number?
