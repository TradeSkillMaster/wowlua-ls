NS = {}

---@class Widget
---@field name string
local Widget = {}

---@return Widget
function MakeThing()
    return Widget
end

---@class Special: Widget
local Special = {}

---@class Other
---@field other string
local Other = {}

---@return Other
function MakeOther()
    return Other
end

---@class Config
---@field handler any
local Config = {}
