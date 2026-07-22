-- AceDB-3.0: a `@class` self-field assigned from `AceDB:New` (`self.db = ...`) is
-- a dotted-chain receiver, not a simple local. Member completion must re-resolve
-- the whole `self.db` chain to its `Defaults & AceDBObject-3.0` intersection and
-- gather fields from *every* member — otherwise `self.db.` / `self.db:` offer
-- only the first member (the typed defaults), dropping the AceDBObject methods.
-- Regression: `self.db.` used to complete only `profile`, and `self.db:` nothing.
---@diagnostic disable: unused-local, unused-function

local AceDB = LibStub("AceDB-3.0")

---@class SelfDbAddon
local SelfDbAddon = {}

function SelfDbAddon:OnInitialize()
    self.db = AceDB:New("SelfDbAddonDB", {
        profile = {
            enabled = true,
            threshold = 5,
        },
    })
end

function SelfDbAddon:Read()
    -- Dot completion offers both a typed-defaults section (`profile`, first
    -- member) and an AceDBObject-only field (`profiles`, second member).
    local _p = self.db.profiles
    --                     ^ comp: profile, profiles
    -- Colon completion offers the AceDBObject methods (the second member) — this
    -- returned nothing before the dotted-chain receiver was re-resolved.
    self.db:GetProfiles()
    --         ^ comp: GetCurrentProfile, GetNamespace, GetProfiles
end
