-- Cross-file duplicate method test: two definitions with different @param
-- annotations, simulating AceConsole:Print pattern.

---@class DupMethodMixin
local DupMethodMixin = {}

---@param chatframe Frame Custom ChatFrame
---@param ... any Values to print
---@diagnostic disable-next-line: duplicate-set-field
function DupMethodMixin:Print(chatframe, ...) end

---@param ... any Values to print
---@diagnostic disable-next-line: duplicate-set-field
function DupMethodMixin:Print(...) end
