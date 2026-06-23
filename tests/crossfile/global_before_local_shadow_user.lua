-- Cross-file test: globals defined before a same-named local shadow in the
-- defs file must resolve here. Regression for FrameXML money constants
-- (COPPER_PER_SILVER etc.) being dropped from the stubs because a trailing
-- `local X = X` made the coarse scan treat the earlier global as a local.
local a = UNITS_PER_TIER ---@diagnostic disable-line: unused-local
--        ^ hover: (global) UNITS_PER_TIER: number = 100

local b = TIERS_PER_RANK ---@diagnostic disable-line: unused-local
--        ^ hover: (global) TIERS_PER_RANK: number = 100

-- Multi-name: field and method defined before the local shadow must resolve
local c = Toolkit.FACTOR ---@diagnostic disable-line: unused-local
--                ^ hover: (field) FACTOR: number
local d = Toolkit.GetFactor ---@diagnostic disable-line: unused-local
--                ^ hover: (field) function GetFactor(self)
