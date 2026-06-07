-- Cross-file chain (hop 2): a no-@return method whose body returns the result
-- of *another* file's deferred method (HopRepo:GetItem in hop_a.lua). Resolving
-- Fetch's return forces the lazy resolver to recurse cross-file.
---@class HopService
---@field repo HopRepo
local HopService = {}

function HopService:Fetch(key)
    return self.repo:GetItem(key)
end
