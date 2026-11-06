---@diagnostic disable: create-global
-- A class declared in several files (`---@class LedgerCore` on a local each binds
-- to the global): a field one file writes a value the scan can't type reads as
-- that file reads it in the others, which declare the class too.
local function parseRevision(text)
    return tonumber(text)
end

---@class LedgerCore
LedgerCore = {}
LedgerCore.revision = parseRevision("1")
