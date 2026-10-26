-- AddonA and AddonB (separate addon roots) both build `ns.entries`, each with
-- its own element type; AddonA's other files read only AddonA's (read.lua).
local _, ns = ...

---@class MultiListEntry
---@field id number

ns.entries = {}

---@param entry MultiListEntry
function ns.Add(entry)
    tinsert(ns.entries, entry)
end
