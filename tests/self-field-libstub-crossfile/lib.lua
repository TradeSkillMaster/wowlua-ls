-- Cross-file `self.field = LibStub("Name"):Method(...)` self-field typing.
-- The assignment lives here; the cross-file read is in user.lua on a re-declared
-- `@class`. The `Lib("Name"):Method(...)` idiom (the ubiquitous LibStub/registry
-- accessor pattern) is recoverable by the coarse scan: the funcall self-field
-- scanner emits the `[Name, Method]` chain so build_on_stubs walks class `Name`
-- to `Method`, typing the field cross-file instead of parking it as `any`.
--
-- In THIS defining file the field additionally keeps the richer generic binding
-- from its own assignment (`Defaults & AceDBObject-3.0` — the typed default
-- sections), while user.lua falls back to the base `AceDBObject-3.0`.
---@diagnostic disable: unused-local, unused-function

---@class LibStubHost
local Host = {}

function Host:Setup()
    self.db = LibStub("AceDB-3.0"):New("MyDB", {
        profile = { enabled = true, threshold = 5 },
    })
end

function Host:Use()
    -- Same-file: the typed default sections are threaded through, so leaf fields
    -- of `profile` resolve and complete with their default's type.
    local t = self.db.profile.threshold
    --    ^ hover: (local) t: number
    local p = self.db.profile.enabled
    --                        ^ comp: enabled, threshold
    -- AceDBObject methods stay available on the same object.
    self.db:GetCurrentProfile()
    --       ^ comp: GetCurrentProfile, GetNamespace, GetProfiles
end
