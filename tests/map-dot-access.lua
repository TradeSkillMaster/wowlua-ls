---@diagnostic disable: unused-local
-- Dot access on a typed map declares no field to hover: the read is the map's
-- value type, for an explicit `table<K, V>` and for a string-literal-keyed one.
---@type table<string, number>
local counts = {}
local n = counts.foo
--    ^ hover: (local) n: number
--               ^ hover: (field) foo: number

---@alias ScopeName "global"|"realm"
---@type table<ScopeName, string>
local keys = {}
local realm = keys.realm
--    ^ hover: (local) realm: string
--                  ^ hover: (field) realm: string

_G.useMapDotAccess = { counts, n, keys, realm }
