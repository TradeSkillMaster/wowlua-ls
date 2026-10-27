---@diagnostic disable: unused-local
-- The field holds the call's `Defaults & AceDBObject-3.0`; reading through the
-- inline defaults shape it carries here gives the defaults' types, over the base
-- class's `global: table`, also where only a condition reads them.
local function Log()
    if not StoreHub.Database.global.debugMode then return end
    --                       ^ hover: (field) global: { debugMode: boolean, version: number }
    --                              ^ hover: (field) debugMode: boolean
    return StoreHub.Database.global.version
    --                              ^ hover: (field) version: number
end
-- A local holding the section takes the shape, not the base class's `table` beside it.
local section = StoreHub.Database.global
--    ^ hover: (local) section: { debugMode: boolean, version: number }
---@param label string
local function Show(label) end
Show(StoreHub.Database.global.version)
--   ^ diag: type-mismatch
