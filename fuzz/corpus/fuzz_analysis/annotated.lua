---@class Point
---@field x number
---@field y number
local Point = {}
Point.__index = Point

---@param x number
---@param y number
---@return Point
function Point.new(x, y)
    return setmetatable({ x = x, y = y }, Point)
end

---@return number
function Point:length()
    return math.sqrt(self.x * self.x + self.y * self.y)
end

local p = Point.new(3, 4)
local len = p:length()
