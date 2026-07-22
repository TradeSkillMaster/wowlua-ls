---@diagnostic disable: unused-local
-- Consumer re-declares `@class LibStubHost` (as every addon module re-declares
-- its addon-namespace `@class`). The `self.db` field assigned in lib.lua from
-- `LibStub("AceDB-3.0"):New(...)` must be typed `AceDBObject-3.0` here — the
-- `Lib("Name"):Method(...)` idiom resolves cross-file, so this is no longer the
-- bare `any` placeholder a general chained receiver falls back to.

---@class LibStubHost
local H = {}

function H:Use()
    local d = self.db
    --              ^ hover: (field) db: AceDBObject-3.0 {
    -- The AceDBObject methods resolve on the cross-file field...
    self.db:GetCurrentProfile()
    --       ^ comp: GetCurrentProfile, GetNamespace, GetProfiles
end
