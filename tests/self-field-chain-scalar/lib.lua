-- Regression: a chained-funcall self-field whose chain returns a NON-table
-- scalar (a number) must not be parked as a guessed `table`. The coarse scan
-- can't resolve `GetFrame():GetHeight()` (the receiver is itself a call), so the
-- bare scanner registers the field existence-only. Before the fix it used a
-- concrete `table` placeholder, which leaked into cross-file reads: passing the
-- field's value to a `number` parameter false-positived as `type-mismatch`
-- (`got table`). The honest `any` placeholder suppresses `undefined-field`
-- without asserting a shape that breaks assignability.
---@diagnostic disable: unused-local

---@return Frame
local function GetFrame() return UIParent end

---@class ScalarHost
local Host = {}

function Host:Setup()
    -- chained: receiver is a call, so the coarse scan can't resolve the chain.
    -- The real result is a number (Frame:GetHeight()), NOT a table.
    self.baseHeight = GetFrame():GetHeight()
end
