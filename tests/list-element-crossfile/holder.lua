-- A list built in a `@class` field: the workspace scan's bare `table` for
-- `self.entries = {}` gives way to this file's own writes, so the elements
-- inserted read back.
---@class CrossHolder
local CrossHolder = {}

function CrossHolder:Init()
    self.entries = {}
end

---@param entry CrossItem
function CrossHolder:Add(entry)
    tinsert(self.entries, entry)
end

function CrossHolder:FirstName()
    local entry = self.entries[1]
    --    ^ hover: (local) entry: CrossItem
    return entry.name
end

-- With nothing typing its elements (an untyped value inserted, a map's keyed
-- writes), a read reads `any`, as through the scan's `table`.
function CrossHolder:InitOther()
    self.names = {}
    self.byKey = {}
end

function CrossHolder:AddName(name)
    tinsert(self.names, name)
end

function CrossHolder:Remember(key, value)
    self.byKey[key] = value
end

function CrossHolder:Lookup(index, key)
    local name = self.names[index]
    --    ^ hover: (local) name: any
    local remembered = self.byKey[key]
    --    ^ hover: (local) remembered: any
    return name, remembered
end

-- A list created where it's first added to.
---@param entry CrossItem
function CrossHolder:Queue(entry)
    self.queued = self.queued or {}
    tinsert(self.queued, entry)
end

function CrossHolder:NextQueued()
    local queued = self.queued[1]
    --    ^ hover: (local) queued: CrossItem
    return queued
end
