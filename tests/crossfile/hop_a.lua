-- Cross-file chain (hop 1): a class-typed field-access return with no @return.
---@class HopWidget
---@field id number
local HopWidget = {}

---@class HopRepo
---@field _items table<string, HopWidget?>
local HopRepo = {}

function HopRepo:GetItem(key)
    return self._items[key]
end
