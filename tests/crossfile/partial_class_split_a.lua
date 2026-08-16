-- Cross-file `@class` field-type harvesting across a PARTIAL class (file A).
--
-- `PCS_Split` is declared `@class (partial)` here AND in partial_class_split_b.lua.
-- The harvester must re-analyze BOTH declaring files and union each field's RHS types,
-- not just the first-seen file. This file assigns `fromA`/`shared`; file B assigns
-- `fromB` and clears `shared`. See partial_class_split_user.lua for the assertions.

local addonName, ns = ...

---@class PCS_Bar
local Bar = {}
ns.PCS_Bar = Bar
function Bar:Beep() end
function Bar:Setup()
    self.tag = 3 + 4     -- coarse `any` -> number; PCS_Bar is a single-decl class
end                      -- co-located with PCS_Split's file A (exercises the
                         -- co-located-sibling accumulate/cache path, whole-file warming)

---@class (partial) PCS_Split
local Split = {}
ns.PCS_Split = Split

---@type PCS_Bar
local bar

function Split:InitA()
    self.fromA = bar     -- coarse `any` -> PCS_Bar (assigned only in file A)
    self.shared = bar    -- assigned PCS_Bar here...
end
