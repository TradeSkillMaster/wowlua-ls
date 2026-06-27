---@diagnostic disable: unused-local

---@type Frame
local f = CreateFrame("Frame")

-- SetPoint is a built-in stub Frame method. It must still resolve (no
-- undefined-field) — proving the library's `@class Frame` did NOT strip the
-- built-in's fields (stub-name reuse merges additively, never destructively).
f:SetPoint("CENTER")

-- The library's own field is also visible cross-file: the colliding declaration
-- was merged onto the stub rather than replacing it.
local lf = f.libField
--    ^ hover: (local) lf: string
