---@diagnostic disable: unused-local
-- Cross-file caller for the mutually-recursive deferred methods (cycle_a/cycle_b).
-- Resolving Foo's return recurses Foo -> Bar -> Foo; the in-progress file guard
-- breaks the back-edge and the type converges to the coarse `any` fallback rather
-- than looping forever.

---@class CycleX
local CycleX = {}

local r = CycleX:Foo("x")
--    ^ hover: (local) r: any  def: local
