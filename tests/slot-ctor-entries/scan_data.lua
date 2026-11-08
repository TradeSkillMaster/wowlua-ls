-- A constructor an entry declares a class of: its keys read as the class
-- declares the fields, as the writes through it do.
local private = select(2, ...) ---@class SlotCtorNS
---@alias SlotMapID number

---@class ScanState
---@field mapID? SlotMapID
---@field count integer
---@field seen table<number, boolean>

private.Data = {
    ---@type ScanState
    Scanner = {
        mapID = nil,
        count = 0,
        seen = {},
    },
}
