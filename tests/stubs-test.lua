-- Test: external globals from stubs

local t = setmetatable({}, {})
local s = type("hello")
local ok = pcall(print, "hi")

---@type Frame
local f = nil
