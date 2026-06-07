---@diagnostic disable: unused-local
-- Cross-file caller for the 2-hop chain. `Fetch` has no @return; its body returns
-- `self.repo:GetItem(key)`, itself a deferred (no-@return) method in another file.
-- The lazy resolver re-analyzes hop_b, which reads GetItem's deferred return and
-- recurses into hop_a, recovering the precise `HopWidget?` across both hops.

---@class HopService
local HopService = {}

local w = HopService:Fetch("x")
--    ^ hover: (local) w: HopWidget?  def: local
-- `Fetch` returns the call expression `self.repo:GetItem(key)` directly, so it
-- forwards GetItem's returns as a vararg passthrough (`...HopWidget?`); the first
-- slot still resolves precisely to `HopWidget?` for the caller's `w`.
--                   ^ hover: (method) function HopService:Fetch(key)\n-> ...HopWidget?  def: external
