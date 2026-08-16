---@diagnostic disable: unused-local
-- Cross-file `@class` field-type harvesting across a PARTIAL class: usage.
-- `PCS_Split` is declared `@class (partial)` in both partial_class_split_a.lua and
-- partial_class_split_b.lua. The harvest reads EVERY declaring file and unions each
-- field's RHS types, so a field assigned only in the second file is still typed, and a
-- field assigned in one file + cleared to nil in another keeps its nilability.

local addonName, ns = ...

---@type PCS_Split
local s = ns.PCS_Split

-- Assigned only in file A: coarse `any` upgraded to its class. The method call would
-- fire `undefined-field` if the field were mistyped (caught by the exhaustive harness).
local a = s.fromA
--    ^ hover: (local) a: PCS_Bar
s.fromA:Beep()

-- Assigned ONLY in the SECOND partial-decl file: proves the harvest reads every
-- declaring file, not just the first-seen one. Would be `any` under the old behavior.
local b = s.fromB
--    ^ hover: (local) b: number

-- Assigned a class in file A and cleared to nil in file B: the harvest unions both
-- declaring files, so the field's nilability is carried (`PCS_Bar?`), never a spurious
-- non-optional `PCS_Bar` that would false-positive when a caller clears it.
local sh = s.shared
--    ^ hover: (local) sh: PCS_Bar?

-- A co-located single-decl class (declared in file A alongside `@class (partial)
-- PCS_Split`): its coarse-`any` field harvests correctly whether warmed by PCS_Split's
-- read (its whole decl-set {file A} is covered by PCS_Split's {A, B}) or on its own.
---@type PCS_Bar
local bar2 = ns.PCS_Bar
local t = bar2.tag
--    ^ hover: (local) t: number
