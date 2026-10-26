-- Lists built on a `@class` addon namespace, read in other files (reader.lua).
---@class NsListsAddon
local addonTable = select(2, ...)

---@class NsListsEntry
---@field id number

addonTable.Search = {}
addonTable.Search.Constants = {}

addonTable.Search.Constants.AllIDs = {}
for id = 1, 3 do
    table.insert(addonTable.Search.Constants.AllIDs, id)
end

addonTable.Transfers = {}

---@param condition fun(): boolean
---@param label string
local function RegisterTransfer(condition, label)
    table.insert(addonTable.Transfers, { condition = condition, label = label })
end

RegisterTransfer(function() return true end, "all")

addonTable.Entries = {}

---@param entry NsListsEntry
function addonTable.AddEntry(entry)
    addonTable.Entries[#addonTable.Entries + 1] = entry
end

-- Cleared to nil in another file (clear.lua).
addonTable.Pending = {}
tinsert(addonTable.Pending, "next")

-- Also built in another file (more.lua).
addonTable.Shared = {}
tinsert(addonTable.Shared, 1)

-- Also written `nil` and `false` in a file scanned before this one (base.lua).
addonTable.Draft = {}
tinsert(addonTable.Draft, "draft")
addonTable.Flagged = {}
tinsert(addonTable.Flagged, "flag")

-- A map keeps the scan's `table`: reading it back through a key isn't the union
-- of every entry's type.
local OPTION_A, OPTION_B = "a", "b"
addonTable.Defaults = {
    [OPTION_A] = false,
    [OPTION_B] = 5,
}
