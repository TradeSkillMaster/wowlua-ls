-- Cross-file call hierarchy test: library definitions

---@class CHLib
local CHLib = {}

---@param x number
---@return number
function CHLib:Double(x)
    return x * 2
end

---@param text string
---@return number
function CHLib.GetLen(text)
    return #text
end

local addonName, ns = ...
ns.CHLib = CHLib

-- Standalone global function
---@param a number
---@param b number
---@return number
function GlobalAdd(a, b)
    return a + b
end
