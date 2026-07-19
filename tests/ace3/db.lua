-- AceDB-3.0: `AceDB:New(name, defaults)` threads the `defaults` table's shape
-- into the returned object (typed `Defaults & AceDBObject-3.0`), so the sections
-- and fields you declare in `defaults` are typed on the DB — `db.profile.myOption`
-- completes and hovers with its default's type — while the AceDBObject methods
-- (`SetProfile`, `GetCurrentProfile`, …) stay available on the same object.
--
-- Section access must not trip need-check-nil: the sections are non-nil `table`
-- on AceDBObject-3.0 (this file's dir enables need-check-nil), and the typed
-- default shape takes precedence over that `table` fallback.
---@diagnostic disable: unused-local

local AceDB = LibStub("AceDB-3.0")

local db = AceDB:New("MyAddonDB", {
    profile = {
        enabled = true,
        threshold = 5,
    },
    global = {
        version = 1,
    },
})

-- Leaf fields of a typed default section resolve through the intersection, both
-- directly and when a section is copied into a local first.
local threshold = db.profile.threshold
--    ^ hover: (local) threshold: number
local version = db.global.version
--    ^ hover: (local) version: number

local profile = db.profile
local pe = profile.enabled
--                 ^ comp: enabled, threshold

-- Member completion on the DB offers a section's typed default fields.
local pt = db.profile.threshold
--                    ^ comp: enabled, threshold

-- AceDBObject methods resolve on the same object...
local cur = db:GetCurrentProfile()
--    ^ hover: (local) cur: string
db:SetProfile("Default")

-- ...and are reachable by colon completion (the intersection's second member).
db:SetProfile("Default")
--     ^ comp: SetProfile
