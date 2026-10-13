---@diagnostic disable: unused-local
-- A field its class declares `table` keeps that type: what this file inserts
-- doesn't make it a list.
function AuthoredHolder:Init()
    self.items = {}
end

---@param name string
function AuthoredHolder:Add(name)
    tinsert(self.items, name)
end

---@param counts table<string, number>
local function takeCounts(counts) end

function AuthoredHolder:Use()
    takeCounts(self.items)
    local items = self.items
    --    ^ hover: (local) items: table
end
