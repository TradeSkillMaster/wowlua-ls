---@diagnostic disable: unused-function
-- Regression test: field assignments on values read from a bracket-accessed
-- table must not leak back into the table's inferred value_type.

-- Case 1: Fields assigned to bracket-read values don't pollute value_type
local registry = {}
--    ^ hover: (local) registry: table<string, {extraTip: string}>

---@param key string
local function Register(key)
    local entry = {}
    entry.extraTip = "hello"
    registry[key] = entry
end

---@param key string
local function OnCleared(key)
    local reg = registry[key]
    if reg.ignoreOnCleared then
        return
    end
    reg.quantity = nil
    reg.hasItem = nil
end

---@param key string
local function SetItem(key)
    local reg = registry[key]
    reg.hasItem = true
    reg.ignoreOnCleared = true
    reg.quantity = 5
end

-- Case 2: Multiple writers to the same table via bracket write merge correctly
local cache = {}
--    ^ hover: (local) cache: {level: number, name: string}[]

---@param id number
local function AddToCache(id)
    local item = {}
    item.name = "sword"
    item.level = 10
    cache[id] = item
end

---@param id number
local function UpdateCache(id)
    local item = cache[id]
    -- These assignments should NOT appear on cache's value_type
    item.dirty = true
    item.lastUpdated = 0
end

-- Case 3: Fields set on the original entry BEFORE bracket-write DO appear
local store = {}
--    ^ hover: (local) store: table<string, {alpha: number, beta: string}>

---@param key string
local function Populate(key)
    local obj = {}
    obj.alpha = 1
    obj.beta = "two"
    store[key] = obj
end

---@param key string
local function Mutate(key)
    local obj = store[key]
    obj.gamma = true  -- should NOT leak
end

-- Case 4: Array-style tables (numeric key) also protected
local list = {}
--    ^ hover: (local) list: {value: number}[]

---@param i number
local function AppendList(i)
    local item = {}
    item.value = 42
    list[i] = item
end

---@param i number
local function ModifyList(i)
    local item = list[i]
    item.modified = true  -- should NOT leak
end

-- Case 5: Direct table[key].field assignment must not add field to the table itself
local pending = {}
--    ^ hover: (local) pending: {name: string}[]

---@param seq number
local function Start(seq)
    local ctx = {}
    ctx.name = "rpc"
    pending[seq] = ctx
end

---@param seq number
local function Extend(seq)
    -- This assigns a field on a dynamically-indexed element.
    -- "timeout" must NOT appear as a field of `pending` itself.
    pending[seq].timeout = 100
end

-- Case 6: Nested bracket write `t[a][b] = v` must attribute v to the INNER
-- table's element type, not to `t`'s. The outer table is an array of arrays,
-- so the value written into the inner list must not pollute `t`'s value_type.
---@alias MapID number
local continentMaps = {}
--    ^ hover: (local) continentMaps: table[]

---@param a MapID
---@param b MapID
local function SortMaps(a, b)
    return a < b
end

---@param continentID number
---@param mapID MapID
local function AddMap(continentID, mapID)
    continentMaps[continentID] = continentMaps[continentID] or {}
    continentMaps[continentID][#continentMaps[continentID] + 1] = mapID
    -- The inner element is a MapID list, so this sorts fine. The `comp` arg
    -- binds table.sort's generic `T = MapID`, forcing `list` to be `MapID[]`.
    -- Before the fix `mapID` leaked into continentMaps' value_type, making the
    -- element type `table | MapID`; the `MapID` (number) member is not
    -- assignable to `MapID[]`, producing a spurious `type-mismatch` here.
    table.sort(continentMaps[continentID], SortMaps)
end
