---@diagnostic disable: unused-local
-- Cross-file test: per-field ---@type annotations preserved across files
local _, ns = ...

-- Field with ---@type table<K,V> preserves the annotated type cross-file
local ach = ns.InlineFieldTest.Achievements
--    ^ hover: (local) ach: table<number, InlineTypeAchData>

-- Another annotated field
local maps = ns.InlineFieldTest.Maps
--    ^ hover: (local) maps: table<number, InlineTypeMapData>

-- Field with ---@type ClassName
local scanner = ns.InlineFieldTest.Scanner
--    ^ hover: (local) scanner: InlineTypeScanData {

-- Unannotated field falls back to literal inference
local plain = ns.InlineFieldTest.Plain
--    ^ hover: (local) plain: table
