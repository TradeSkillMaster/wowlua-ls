---@diagnostic disable: create-global

---@class AceGlobalItem
---@field id number

GlobalAddon = LibStub("AceAddon-3.0"):NewAddon("GlobalAddon")

function GlobalAddon:OnInitialize()
    self.items = {}
    self.counts = {}
end

---@param item AceGlobalItem
function GlobalAddon:Track(item)
    tinsert(self.items, item)
    self.counts[item.id] = (self.counts[item.id] or 0) + 1
end

function GlobalAddon:Count(id)
    local count = self.counts[id]
    return count
end

function GlobalAddon:FirstId()
    local first = self.items[1]
    return first.id
end
