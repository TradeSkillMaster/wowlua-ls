-- A `@class` field list another file builds (`holder.lua`): the scan's bare
-- `table` for it gives way to that file's type, so its elements read here too.
---@diagnostic disable: unused-local

---@param holder CrossHolder
---@param key string
local function readHolder(holder, key)
    local entry = holder.entries[1]
    --    ^ hover: (local) entry: CrossItem
    for _, each in ipairs(holder.entries) do
        local eachName = each.name
        --    ^ hover: (local) eachName: string
    end
    local size = #holder.entries
    --    ^ hover: (local) size: number
    local queued = holder.queued[1]
    --    ^ hover: (local) queued: CrossItem

    -- Fields whose elements nothing types keep the scan's `table`.
    local name = holder.names[1]
    --    ^ hover: (local) name: any
    local remembered = holder.byKey[key]
    --    ^ hover: (local) remembered: any
end
