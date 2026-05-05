-- Cross-file test: @type table<K,V> on addon namespace fields (no @class on namespace)
local _, ns = ...

---@type table<number, boolean>
ns.indexes = {}

---@type table<string, number>
ns.lookup = {}

---@type { [number]: string }
ns.names = {}
