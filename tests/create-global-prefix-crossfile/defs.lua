-- Field writes through a parenthesized prefix expression in the two contexts
-- that ONLY the scan_globals.rs descendants pass scans (the main statement
-- loop handles neither): inside a function body, and a top-level multi-target
-- assignment. The descendants pass's `has_prefix_expr_base` guard must keep
-- the trailing field names (`inFuncField`, `multiTargetField`) from being
-- registered as phantom existence-only globals — otherwise a bare read of
-- them in another file (user.lua) is silently suppressed instead of flagged
-- `undefined-global`.
---@diagnostic disable: unused-local, unused-function

local panelA = {}
local panelB = {}

-- In-function write: the main loop does not descend into function bodies.
local function setup()
    (panelA or panelB).inFuncField = 1
end
setup()

-- Top-level multi-target write: the main loop only handles single-target
-- assignments, so the later prefix-base target is seen only by the
-- descendants pass (its `idents.len() >= 2` branch).
local realTarget = {}
realTarget.ok, (panelA or panelB).multiTargetField = 1, 2
return realTarget
