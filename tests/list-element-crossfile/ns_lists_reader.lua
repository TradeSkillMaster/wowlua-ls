-- The addon-namespace lists `ns_lists.lua` builds read their elements in other
-- files too; its `[k] = v` settings map keeps the scan's type here as well.
---@diagnostic disable: unused-local
local _, ns = ...

local item = ns.items[1]
--    ^ hover: (local) item: CrossItem
local entry = ns.Module.entries[1]
--    ^ hover: (local) entry: CrossItem
for _, each in ipairs(ns.items) do
    local name = each.name
    --    ^ hover: (local) name: string
end

---@param enabled boolean?
local function setEnabled(enabled) end

---@param name string
local function readDefault(name)
    setEnabled(ns.Config.Defaults[name])
end
