---@class MyWidget
---@field visible boolean
local MyWidget = {}

---@param name string
---@return boolean
function MyWidget:Show(name)
    return true
end

---@return number
function MyWidget:GetWidth()
    return 0
end

---@class MyContainer
---@field items table
local MyContainer = {}

---@param item any
function MyContainer:Add(item)
end
