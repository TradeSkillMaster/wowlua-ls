-- Lists built in addon-namespace fields: the workspace scan's empty table for
-- `ns.items = {}` gives way to the list this file builds.
---@diagnostic disable: unused-local
local _, ns = ...

ns.items = {}
ns.Module = {}
ns.Module.entries = {}

---@param item CrossItem
function ns.AddItem(item)
    tinsert(ns.items, item)
    tinsert(ns.Module.entries, item)
end

function ns.FirstItem()
    local item = ns.items[1]
    --    ^ hover: (local) item: CrossItem
    local entry = ns.Module.entries[1]
    --    ^ hover: (local) entry: CrossItem
    return item, entry
end

-- ...also when a function creates the list.
function ns.Reset()
    ns.pending = {}
end

---@param item CrossItem
function ns.Queue(item)
    ns.pending[#ns.pending + 1] = item
end

function ns.NextPending()
    local pending = ns.pending[1]
    --    ^ hover: (local) pending: CrossItem
    return pending
end

-- A map built in a field keeps the scan's type: reading it back through a key
-- isn't the union of every entry's type.
---@param enabled boolean?
local function setEnabled(enabled) end

local OPTION_A, OPTION_B = "a", "b"
ns.Config = {}
ns.Config.Defaults = {
    [OPTION_A] = false,
    [OPTION_B] = 5,
}

function ns.Config.Get(name)
    return ns.Config.Defaults[name]
end

setEnabled(ns.Config.Get(OPTION_A))
