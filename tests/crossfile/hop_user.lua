---@diagnostic disable: unused-local
-- Cross-file caller for the 2-hop chain. `Fetch` has no @return; its body returns
-- `self.repo:GetItem(key)`, itself a deferred (no-@return) method in another file.
-- The lazy resolver re-analyzes hop_b, which reads GetItem's deferred return and
-- recurses into hop_a, recovering the precise `HopWidget?` across both hops.

---@class HopService
local HopService = {}

local w = HopService:Fetch("x")
--    ^ hover: (local) w: HopWidget?  def: local
-- `Fetch` returns the call expression `self.repo:GetItem(key)` directly. The
-- lazy resolver recurses through both hops and recovers GetItem's precise
-- single return `HopWidget?` for the caller's `w` and the method signature.
--                   ^ hover: (method) function HopService:Fetch(key)\n-> HopWidget?  def: external
