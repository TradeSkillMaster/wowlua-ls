-- The same base TOC lists this file as `Classic.lua [ExcludeLoadGameType mainline]`,
-- leaving Classic + Classic Era. The addon itself targets all flavors
-- (.wowluarc.json), so the exclusion is what narrows this file.

-- PlayerGetTimerunningSeasonID is retail-only — should warn.
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- AbandonQuest is classic + classic_era only. It must NOT warn: that only holds
-- if the exclusion actually removed retail from this file's mask, rather than
-- being stripped off the path and leaving the addon's full breadth behind.
AbandonQuest()
