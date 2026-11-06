local _, ns = ...
-- Namespace fields an author types `table` (`table<T>` names no map), which its
-- constructor doesn't override.
---@class LedgerRaid
---@field boss number

---@type table<LedgerRaid>
ns.raidInfo = {
--  ^ hover: (field) raidInfo: table<LedgerRaid>
    { boss = 1 },
    { boss = 2 },
}
