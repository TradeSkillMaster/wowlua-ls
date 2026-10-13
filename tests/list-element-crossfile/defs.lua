-- Lists and maps returned to other files (see user.lua).
local _, ns = ...

---@class CrossItem
---@field name string
local CrossItem = {}

function CrossItem:Describe() end

---@type CrossItem[]
local all = {}

function ns.GetAll()
    return all
end

---@type table<string, CrossItem>
local byName = {}

function ns.GetByName()
    return byName
end

---@param item CrossItem
function ns.Collect(item)
    local collected = {}
    tinsert(collected, item)
    return collected
end
