---@diagnostic disable: unused-local
-- Cross-file test: access @type table<K,V> fields from addon namespace

local _, ns = ...

-- table<number, boolean> field preserves key/value types
local idx = ns.indexes
--    ^ hover: (local) idx: table<number, boolean>

-- table<string, number> field preserves key/value types
local lu = ns.lookup
--    ^ hover: (local) lu: table<string, number>

-- { [number]: string } field preserves key/value types
local nm = ns.names
--    ^ hover: (local) nm: table<number, string>

-- pairs() iterates with correct key/value types
for k, v in pairs(ns.indexes) do
    local key = k
    --    ^ hover: (local) key: number
    local val = v
    --    ^ hover: (local) val: boolean
end
