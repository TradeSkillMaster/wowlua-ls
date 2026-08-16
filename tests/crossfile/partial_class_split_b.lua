-- Cross-file `@class` field-type harvesting across a PARTIAL class (file B).
-- The second `@class (partial) PCS_Split` declaration. `fromB` is assigned ONLY here,
-- so a harvest that read only file A (the old single-first-seen-file behavior) would
-- leave it coarse `any`; and `shared` is cleared to nil here, so its nilability is only
-- visible by unioning this file with file A. See partial_class_split_a.lua.

local addonName, ns = ...

---@class (partial) PCS_Split
local Split = {}

function Split:InitB()
    self.fromB = 1 + 2   -- coarse `any` -> number (assigned only in file B)
end

function Split:ClearB()
    self.shared = nil    -- ...and nil here -> nilability carried across files (PCS_Bar?)
end
