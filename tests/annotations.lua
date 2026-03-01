-- Test: annotation-driven type resolution
-- Tests @param, @return, @type, @class, @field, @alias, optional params

---@param name string
---@param count number
---@return boolean
function check(name, count)
--       ^ hover: check: fun(name: string, count: number): boolean  def: local
    return true
end

---@type string
local greeting = nil
--    ^ hover: greeting: string  def: local

---@param x number
---@param y number
---@return number
local function add(x, y)
    return x + y
end

local result = add(1, 2)
--    ^ hover: result: number  def: local
local ok = check("hi", 5)
--    ^ hover: ok: boolean  def: local

---@class Widget
---@field width number

---@class Frame : Widget
---@field name string
---@field visible boolean

---@alias Anchor "TOPLEFT" | "TOP" | "TOPRIGHT"

---@class MyAddon
---@field version string
local MyAddon = {}
--    ^ hover: MyAddon: MyAddon  def: local

---@param point Anchor
function MyAddon:SetPosition(point)
end

---@type Frame
local f = nil
--    ^ hover: f: Frame  def: local

---@param name? string
---@return number numSites
function optionalTest(name)
    return 1
end

local optResult = optionalTest("hi")
--    ^ hover: optResult: number  def: local
