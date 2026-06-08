---@diagnostic disable: unused-local
-- Cross-file caller for the correlated-return "cases" fix. The method hover should
-- show precise `(number, number)` / `(nil, nil)` cases (not `(any, any)`), and
-- sibling narrowing (`if lo then`) should narrow `lo` to `number`.

---@class CorrSource
local CorrSource = {}

local lo, hi = CorrSource:Range(5)
--    ^ hover: (local) lo: number?
--                        ^ hover: (method) function CorrSource:Range(key)\n  -> number?, number?\n  cases:\n    (number, number)\n    (nil, nil)  def: external
if lo then
    local got = lo
    --          ^ hover: (local) lo: number
end
