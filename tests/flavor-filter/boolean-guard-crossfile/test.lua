-- Cross-file boolean flavor guard: ns.isRetail is defined in defs.lua
-- with `@flavor-narrows retail` and used here to guard API calls.
local _, ns = ...

-- Unguarded call warns.
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- Cross-file boolean guard narrows to retail in then-branch.
if ns.isRetail then
    PlayerGetTimerunningSeasonID()
else
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- Cross-file classic_era guard.
if ns.isClassicEra then
    AbandonQuest()
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- Cross-file flavor guard defined inside an if block (regression test).
if ns.nestedRetail then
    PlayerGetTimerunningSeasonID()
end

-- Cross-file unannotated field guard (inferred from its comparison initializer).
if ns.isRetailInferred then
    PlayerGetTimerunningSeasonID()
else
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

if ns.isNotRetailInferred then
    AbandonQuest()
end

-- Cross-file unannotated global guard.
if IS_CLASSIC_ERA_INFERRED then
    AbandonQuest()
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- A flag widened after its comparison in defs.lua is no guard here either.
if ns.useSeasonUI then
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end
