-- A constructor entry deep writes extend and a write of the field: other files
-- read the field as this file does, both of them; an entry an annotated write
-- types keeps the annotation.

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
