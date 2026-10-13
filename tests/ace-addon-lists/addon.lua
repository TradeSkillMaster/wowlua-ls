-- Lists built in the fields of Ace addon objects. `NewAddon`/`NewModule` with a
-- literal name bind the variable to the class at build time, so `self` writes in
-- its methods land on it and what's inserted reads back. (A global addon's class
-- comes from the stub-aware workspace scan: `ace_addon_scan_classes`.)
---@diagnostic disable: unused-local

---@class AceListItem
---@field id number

local LocalAddon = LibStub("AceAddon-3.0"):NewAddon("LocalAddon", "AceEvent-3.0")

function LocalAddon:OnInitialize()
    self.queue = {}
end

---@param item AceListItem
function LocalAddon:Enqueue(item)
    self.queue[#self.queue + 1] = item
end

function LocalAddon:Peek()
    local queued = self.queue[1]
    --    ^ hover: (local) queued: AceListItem
    return queued.id
end

local ListModule = LocalAddon:NewModule("ListModule")

function ListModule:OnEnable()
    self.rows = {}
end

---@param item AceListItem
function ListModule:AddRow(item)
    tinsert(self.rows, item)
end

function ListModule:FirstRow()
    local row = self.rows[1]
    --    ^ hover: (local) row: AceListItem
    return row
end
