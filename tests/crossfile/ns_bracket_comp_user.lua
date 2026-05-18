-- Cross-file test: completions inside bracket-assigned table constructor
-- when the target table's type comes from a namespace field chain
local _, private = ...

local NPCs = private.NPCs

NPCs[148390] = {
    q
--  ^ comp: questID, npcID, classification
}

-- 2-level deep chain: private.Data.Zones
local Zones = private.Data.Zones

Zones[2248] = {
    m
--  ^ comp: mapID, name
}
