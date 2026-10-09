---@diagnostic disable: unused-local

local db = DeepFieldReplaced.db
--    ^ hover: (local) db: DeepFieldStore
local cache = DeepFieldReplaced.cache
--    ^ hover: (local) cache: any
local kept = DeepFieldReplaced.kept
--    ^ hover: (local) kept: DeepFieldStore

-- The replaced entry's contents don't reach the class.
---@param s DeepFieldStore
local function storeReads(s)
    local stale = s.stale
    --              ^ diag: undefined-field
    local extra = s.extra
    --              ^ diag: undefined-field
    return stale, extra
end
