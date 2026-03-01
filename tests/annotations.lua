---@param name string
---@param count number
---@return boolean
function check(name, count)
    return true
end

---@type string
local greeting = nil

---@param x number
---@param y number
---@return number
local function add(x, y)
    return x + y
end

local result = add(1, 2)
local ok = check("hi", 5)

---@class Widget
---@field width number

---@class Frame : Widget
---@field name string
---@field visible boolean

---@alias Anchor "TOPLEFT" | "TOP" | "TOPRIGHT"

---@class MyAddon
---@field version string
local MyAddon = {}

---@param point Anchor
function MyAddon:SetPosition(point)
end

---@type Frame
local f = nil

---@param name? string
---@return number numSites
function optionalTest(name)
    return 1
end

local optResult = optionalTest("hi")
