---@diagnostic disable: unused-local
-- Consumer re-declares `@class LibStubHost` (as every addon module re-declares
-- its addon-namespace `@class`). The `self.db` field assigned in lib.lua from
-- `LibStub("AceDB-3.0"):New(...)` is typed here as lib.lua types it: the call's
-- `Defaults & AceDBObject-3.0`, the typed default sections included.

---@class LibStubHost
local H = {}

function H:Use()
    local d = self.db
    --              ^ hover: (field) db: { profile: { enabled: boolean, threshold: number } } & AceDBObject-3.0 {
    local t = self.db.profile.threshold
    --    ^ hover: (local) t: number
    -- The AceDBObject methods resolve on the cross-file field...
    self.db:GetCurrentProfile()
    --       ^ comp: GetCurrentProfile, GetNamespace, GetProfiles
end
