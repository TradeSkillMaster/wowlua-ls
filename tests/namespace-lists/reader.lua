-- Lists another file builds on the addon namespace (constants.lua) read their
-- elements here, as in that file.
---@diagnostic disable: unused-local
---@class NsListsAddon
local addonTable = select(2, ...)

local ids = addonTable.Search.Constants.AllIDs
--    ^ hover: (local) ids: number[]
for _, id in ipairs(addonTable.Search.Constants.AllIDs) do
    local copy = id
    --    ^ hover: (local) copy: number
end

for _, transfer in ipairs(addonTable.Transfers) do
    local label = transfer.label
    --    ^ hover: (local) label: string
    local ready = transfer.condition()
    --    ^ hover: (local) ready: boolean
end

local entry = addonTable.Entries[1]
--    ^ hover: (local) entry: NsListsEntry
local entryId = addonTable.Entries[1].id
--                                    ^ hover: (field) id: number  comp: id
local missing = addonTable.Entries[1].missing
--                                    ^ diag: undefined-field

-- Another file clears it, so it may be nil.
local pending = addonTable.Pending
--    ^ hover: (local) pending: string[]?

-- Two files build it.
local shared = addonTable.Shared[1]
--    ^ hover: (local) shared: number | string

-- A file scanned before the one building them sets them to `nil` and `false`.
local draft = addonTable.Draft
--    ^ hover: (local) draft: string[]?
local flagged = addonTable.Flagged
--    ^ hover: (local) flagged: table

---@param enabled boolean?
local function setEnabled(enabled) end

---@param name string
local function isEnabled(name)
    local defaults = addonTable.Defaults
    --    ^ hover: (local) defaults: table
    setEnabled(addonTable.Defaults[name])
    local registryDefaults = NsListsRegistry.defaults
    --    ^ hover: (local) registryDefaults: table
    setEnabled(NsListsRegistry.defaults[name])
end

local handler = NsListsRegistry.handlers[1]
--    ^ hover: (local) handler: fun(event: string)
local queued = NsListsQueue.items[1]
--    ^ hover: (local) queued: NsListsEntry
local recent = NsListsRegistry.recent[1]
--    ^ hover: (local) recent: number
