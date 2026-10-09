-- A write of a constructor entry that deep writes extend still replaces it, as
-- it would the entry's bare `table`: a typed value wins, an untyped one is `any`.

---@class DeepFieldStore
---@field count number

---@return DeepFieldStore
function CreateDeepFieldStore()
    return { count = 0 }
end

---@class DeepFieldReplaced
DeepFieldReplaced = { db = {}, cache = {}, kept = { stale = 1 } }
DeepFieldReplaced.db.seen = true
DeepFieldReplaced.db = CreateDeepFieldStore()
DeepFieldReplaced.cache.size = 0
DeepFieldReplaced.kept.extra = 2
---@type DeepFieldStore
DeepFieldReplaced.kept = CreateDeepFieldStore()

function DeepFieldReplaced.Init()
    DeepFieldReplaced.cache = LoadDeepFieldCache()
end
