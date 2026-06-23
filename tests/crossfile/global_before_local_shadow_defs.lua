-- Cross-file test: a global assigned at file scope and *then* shadowed by a
-- same-named `local X = X` further down the file must still be recognized as a
-- genuine cross-file global. This mirrors the FrameXML money-constant pattern
-- (`X = 100` followed by `local X = X` to capture a fast upvalue), where the
-- coarse global scan must compare declaration offsets rather than skipping any
-- name that is ever declared local in the file.
local private = select(2, ...) ---@diagnostic disable-line: unused-local

UNITS_PER_TIER = 100
TIERS_PER_RANK = 100

-- Multi-name paths: table + field + method defined before the local shadow.
Toolkit = {}
Toolkit.FACTOR = 42
function Toolkit:GetFactor() return self.FACTOR end ---@diagnostic disable-line: missing-return

-- Local upvalue capture of the globals declared above. The local declaration
-- only takes effect from here down, so the assignments above remain globals.
local UNITS_PER_TIER = UNITS_PER_TIER ---@diagnostic disable-line: unused-local
local TIERS_PER_RANK = TIERS_PER_RANK ---@diagnostic disable-line: unused-local
local Toolkit = Toolkit ---@diagnostic disable-line: unused-local
